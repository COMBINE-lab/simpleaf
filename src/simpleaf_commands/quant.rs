use crate::utils::af_utils::*;

use crate::core::{context, exec, index_meta, io, runtime};
use crate::utils::prog_parsing_utils;
use crate::utils::prog_utils;
use crate::utils::prog_utils::ReqProgs;

use anyhow::{Context, bail};
use serde_json::json;
use std::collections::HashMap;
use std::io::{BufRead, BufReader, BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use tracing::{error, info, warn};

use super::MapQuantOpts;
use crate::utils::chem_utils::ExpectedOri;
use crate::utils::constants::{CHEMISTRIES_PATH, NUM_SAMPLE_LINES};

/// Open a permit-list file with transparent compression handling and return a
/// buffered reader over the resulting stream.
fn get_generic_buf_reader(ipath: &Path) -> anyhow::Result<BufReader<Box<dyn Read>>> {
    let (reader, compression) = niffler::from_path(ipath)
        .with_context(|| format!("Could not open requsted file {}", ipath.display()))?;
    match compression {
        niffler::compression::Format::No => info!("found uncompressed file"),
        f => info!("found file compressed using {:?}", f),
    }
    Ok(BufReader::new(reader))
}

struct CBListInfo {
    pub init_file: PathBuf,
    pub final_file: PathBuf,
    pub is_single_column: bool,
}

impl CBListInfo {
    fn new() -> Self {
        CBListInfo {
            init_file: PathBuf::new(),
            final_file: PathBuf::new(),
            is_single_column: true,
        }
    }
    // we iterate the file to see if it only has cb or with affiliated info (by separator \t).
    fn init(&mut self, pl_file: &Path, output: &Path) -> anyhow::Result<()> {
        // open pl_file
        let br = get_generic_buf_reader(pl_file)
            .with_context(|| "failed to successfully open permit-list file.")?;

        // find if there is any "\t"
        let is_single_column = br
            .lines()
            .take(NUM_SAMPLE_LINES) // don't read the whole file in the single-coumn case
            .map(|l| {
                l.with_context(|| format!("Could not open permitlist file {}", pl_file.display()))
            })
            .collect::<anyhow::Result<Vec<_>>>()?
            .into_iter()
            .any(|l| !l.contains('\t'));

        // if single column, we are good. Otherwise, we need to write the first column to the final file
        let final_file: PathBuf;
        if is_single_column {
            final_file = pl_file.to_path_buf();
        } else {
            info!(
                "found multiple columns in the barcode list tsv file, use the first column as the barcodes."
            );

            // create output dir if doesn't exist
            if !output.exists() {
                std::fs::create_dir_all(output)?;
            }
            // define final_cb file and open a buffer writer for it
            final_file = output.join("cb_list.txt");
            let final_f = std::fs::File::create(&final_file).with_context({
                || format!("Could not create final cb file {}", final_file.display())
            })?;
            let mut final_bw = BufWriter::new(final_f);

            // reinitialize the reader
            let br = get_generic_buf_reader(pl_file)
                .with_context(|| "failed to successfully re-open permit-list file.")?;

            // TODO: consider using byte_lines (from bytelines crate) here instead
            for l in br.lines() {
                // find the tab and write the first column to the final file
                writeln!(
                    final_bw,
                    "{}",
                    l?.split('\t').next().with_context({
                        || format!("Could not parse pl file {}", pl_file.display())
                    })?
                )?
            }
        }

        self.init_file = pl_file.to_path_buf();
        self.final_file = final_file;
        self.is_single_column = is_single_column;
        Ok(())
    }

    fn update_af_quant_barcodes_tsv(&self, barcode_tsv: &PathBuf) -> anyhow::Result<()> {
        // if the permit list was single column, then we don't need to do anything
        // if the permit list was not single column, then we need to add the extra columns into the alevin-fry quants_mat_rows.txt file.
        if self.is_single_column {
            return Ok(());
        }

        // if we are here but the init file doesn't exist, then we have a problem
        if !self.init_file.exists() {
            bail!(
                "The CBListInfo struct was not properly initialized. Please report this issue on GitHub."
            );
        }

        // if we cannot find the count matrix column files, then complain
        if !barcode_tsv.exists() {
            bail!(
                "The barcode tsv file {} does not exist. Please report it on GitHub",
                barcode_tsv.display()
            );
        }

        info!("Add barcode affiliate info into count matrix row file");

        // The steps are:
        // 1. read quants_mat_rows.txt as a hashmap
        // 2. Init a vector to store the final rows, which has the same length as the hashmap
        // 3. parse the original whitelist file, if we see the cb in the hashmap, then we add the line to the vector at the corresponding position
        // 4. write the vector to the quants_mat_rows.txt file

        // we read the barcode tsv file as a hashmap where the values are the order of the barcode in the quants_mat_rows.txt file
        let barcodes_br = BufReader::new(std::fs::File::open(barcode_tsv)?);
        let mut barcodes: HashMap<String, usize> = HashMap::new();
        for (lid, l) in barcodes_br.lines().enumerate() {
            let line: String = l.with_context(|| {
                format!(
                    "Could not parse the matrix rows file {}",
                    barcode_tsv.display()
                )
            })?;
            barcodes.insert(line, lid);
        }

        // Then, we update the matrix row file.
        // First, we init a vector to store the rows.
        let mut row_vec: Vec<String> = vec![String::new(); barcodes.len()];

        // read the whitelist file and parse only those in the matrix row file.
        let mut allocated_cb = 0;
        let br = get_generic_buf_reader(&self.init_file)
            .with_context(|| "failed to successfully re-open permit-list file.")?;
        for l in br.lines() {
            // identify the cb
            let line = l?;
            let cb = line.split('\t').next().with_context({
                || format!("Could not parse pl file {}", self.init_file.display())
            })?;

            // if the cb is in the quantified barcodes, then we add the line to the row_vec
            if let Some(rowid) = barcodes.get(cb) {
                row_vec[*rowid] = line;
                allocated_cb += 1;
            }
        }

        // if the number of allocated cb is less than the total number of cb in the quantified matrix, we complain
        if allocated_cb != barcodes.len() {
            bail!(
                "Only {} out of {} quantified barcodes are found in the whitelist. Please report this issue on GitHub.",
                allocated_cb,
                barcodes.len()
            );
        }

        // create a buffer writer to overwrite the quants_mat_rows.txt file
        let mut final_barcodes_bw = BufWriter::new(std::fs::File::create(barcode_tsv)?);

        // write the row_vec to the final barcodes.tsv file
        for l in row_vec {
            writeln!(final_barcodes_bw, "{}", l)?;
        }

        // we remove the intermediate cb_list file we created
        std::fs::remove_file(&self.final_file)?;
        Ok(())
    }
}

fn push_advanced_piscem_options(
    piscem_quant_cmd: &mut std::process::Command,
    opts: &MapQuantOpts,
) -> anyhow::Result<()> {
    if opts.ignore_ambig_hits {
        piscem_quant_cmd.arg("--ignore-ambig-hits");
    } else {
        piscem_quant_cmd
            .arg("--max-ec-card")
            .arg(format!("{}", opts.max_ec_card));
    }

    if opts.no_poison {
        piscem_quant_cmd.arg("--no-poison");
    }

    piscem_quant_cmd
        .arg("--skipping-strategy")
        .arg(&opts.skipping_strategy);

    if opts.struct_constraints {
        piscem_quant_cmd.arg("--struct-constraints");
    }

    piscem_quant_cmd
        .arg("--max-hit-occ")
        .arg(format!("{}", opts.max_hit_occ));

    piscem_quant_cmd
        .arg("--max-hit-occ-recover")
        .arg(format!("{}", opts.max_hit_occ_recover));

    piscem_quant_cmd
        .arg("--max-read-occ")
        .arg(format!("{}", opts.max_read_occ));

    Ok(())
}

fn validate_map_and_quant_opts(opts: &MapQuantOpts) -> anyhow::Result<()> {
    if opts.use_piscem && opts.use_selective_alignment {
        error!(concat!(
            "The `--use-selective-alignment` flag cannot be used with the ",
            "default `piscem` mapper. If you wish to use `--selective-alignment` ",
            "then please pass the `--no-piscem` flag as well (and ensure that ",
            "you are passing a `salmon` index and not a `piscem` index)."
        ));
        bail!("conflicting command line arguments");
    }

    Ok(())
}

#[derive(Debug)]
struct QuantSetup {
    rp: ReqProgs,
    index_type: IndexType,
    t2g_map_file: PathBuf,
    gene_id_to_name_opt: Option<PathBuf>,
    chem: Chemistry,
    ori: ExpectedOri,
    filter_meth: CellFilterMethod,
    threads: u32,
}

#[derive(Debug)]
struct MappingStageOutput {
    sc_mapper: String,
    map_cmd_string: String,
    map_output: PathBuf,
    map_duration: Duration,
}

#[derive(Debug)]
struct QuantStageOutput {
    gpl_output: PathBuf,
    gpl_cmd_string: String,
    collate_cmd_string: String,
    quant_cmd_string: String,
    gpl_duration: Duration,
    collate_duration: Duration,
    quant_duration: Duration,
}

fn resolve_quant_setup(
    af_home_path: &Path,
    opts: &MapQuantOpts,
) -> anyhow::Result<(QuantSetup, CBListInfo)> {
    let mut t2g_map = opts.t2g_map.clone();
    let ctx = context::load_runtime_context(af_home_path)?;
    let rp: ReqProgs = ctx.progs;
    rp.issue_recommended_version_messages();

    let index_meta = index_meta::resolve_quant_index(opts.index.clone(), opts.use_piscem)?;
    if t2g_map.is_none()
        && let Some(t2g_loc) = index_meta.inferred_t2g.clone()
    {
        info!(
            "found local t2g file at {}, will attempt to use this since none was provided explicitly",
            t2g_loc.display()
        );
        t2g_map = Some(t2g_loc);
    }
    let index_type = index_meta.index_type;
    let gene_id_to_name_opt = index_meta.inferred_gene_id_to_name;

    let t2g_map_file = t2g_map.context(
        "A transcript-to-gene map (t2g) file was not provided via `--t2g-map`|`-m` and could \
        not be inferred from the index. Please provide a t2g map explicitly to the quant command.",
    )?;
    prog_utils::check_files_exist(std::slice::from_ref(&t2g_map_file))?;

    match index_type {
        IndexType::Piscem(_) => {
            if rp.piscem.is_none() {
                bail!(
                    "A piscem index is being used, but no piscem executable is provided. Please set one with `simpleaf set-paths`."
                );
            }
        }
        IndexType::Salmon(_) => {
            if rp.salmon.is_none() {
                bail!(
                    "A salmon index is being used, but no salmon executable is provided. Please set one with `simpleaf set-paths`."
                );
            }
        }
        IndexType::NoIndex => {}
    }

    let custom_chem_p = af_home_path.join(CHEMISTRIES_PATH);
    let chem = Chemistry::from_str(&index_type, &custom_chem_p, &opts.chemistry)?;
    let ori = if let Some(o) = &opts.expected_ori {
        ExpectedOri::from_str(o).with_context(|| {
            format!(
                "Could not parse orientation {}. It must be one of the following: {:?}",
                o,
                ExpectedOri::all_to_str().join(", ")
            )
        })?
    } else {
        chem.expected_ori()
    };

    let mut filter_meth_opt = None;
    let mut pl_info = CBListInfo::new();
    if let Some(ref pl_file) = opts.unfiltered_pl {
        if let Some(pl_file) = pl_file {
            if pl_file.is_file() {
                pl_info.init(pl_file, &opts.output)?;
                filter_meth_opt = Some(CellFilterMethod::UnfilteredExternalList(
                    pl_info.final_file.to_string_lossy().into_owned(),
                    opts.min_reads,
                ));
            } else {
                bail!(
                    "The provided path {} does not exist as a regular file.",
                    pl_file.display()
                );
            }
        } else {
            let pl_res = get_permit_if_absent(af_home_path, &chem)?;
            match pl_res {
                PermitListResult::DownloadSuccessful(p) | PermitListResult::AlreadyPresent(p) => {
                    pl_info.init(&p, &opts.output)?;
                    filter_meth_opt = Some(CellFilterMethod::UnfilteredExternalList(
                        pl_info.final_file.to_string_lossy().into_owned(),
                        opts.min_reads,
                    ));
                }
                PermitListResult::UnregisteredChemistry => {
                    bail!(
                        "Cannot automatically obtain an unfiltered permit list for an unregistered chemistry : {}.",
                        chem.as_str()
                    );
                }
            }
        }
    } else {
        if let Some(ref filtered_path) = opts.explicit_pl {
            pl_info.init(filtered_path, &opts.output)?;
            filter_meth_opt = Some(CellFilterMethod::ExplicitList(
                pl_info.final_file.to_string_lossy().into_owned(),
            ));
        };
        if let Some(ref num_forced) = opts.forced_cells {
            filter_meth_opt = Some(CellFilterMethod::ForceCells(*num_forced));
        };
        if let Some(ref num_expected) = opts.expect_cells {
            filter_meth_opt = Some(CellFilterMethod::ExpectCells(*num_expected));
        };
    }
    if opts.knee {
        filter_meth_opt = Some(CellFilterMethod::KneeFinding);
    }

    let (threads, capped_at) = runtime::cap_threads(opts.threads);
    if let Some(max_threads) = capped_at {
        warn!(
            "The maximum available parallelism is {}, but {} threads were requested.",
            max_threads, opts.threads
        );
        warn!("setting number of threads to {}", max_threads);
    }

    let setup = QuantSetup {
        rp,
        index_type,
        t2g_map_file,
        gene_id_to_name_opt,
        chem,
        ori,
        filter_meth: filter_meth_opt.context("No valid filtering strategy was provided!")?,
        threads,
    };
    Ok((setup, pl_info))
}

fn run_mapping_stage(
    opts: &MapQuantOpts,
    setup: &QuantSetup,
) -> anyhow::Result<MappingStageOutput> {
    if let Some(index) = opts.index.clone() {
        let reads1 = opts.reads1.as_ref().context(
            "Mapping against an index was requested, but read1 files were not provided.",
        )?;
        let reads2 = opts.reads2.as_ref().context(
            "Mapping against an index was requested, but read2 files were not provided.",
        )?;
        if reads1.len() != reads2.len() {
            bail!(
                "{} read1 files and {} read2 files were given; Cannot proceed!",
                reads1.len(),
                reads2.len()
            );
        }

        match &setup.index_type {
            IndexType::Piscem(index_base) => {
                let piscem_prog_info =
                    setup.rp.piscem.as_ref().context(
                        "A piscem index is being used, but piscem program info is missing.",
                    )?;
                let mut piscem_quant_cmd =
                    std::process::Command::new(format!("{}", &piscem_prog_info.exe_path.display()));
                let index_path = format!("{}", index_base.display());
                piscem_quant_cmd
                    .arg("map-sc")
                    .arg("--index")
                    .arg(index_path);

                let map_output = opts.output.join("af_map");
                piscem_quant_cmd
                    .arg("--threads")
                    .arg(format!("{}", setup.threads))
                    .arg("-o")
                    .arg(&map_output);

                match prog_utils::check_version_constraints(
                    "piscem",
                    ">=0.7.0, <1.0.0",
                    &piscem_prog_info.version,
                ) {
                    Ok(_piscem_ver) => {
                        push_advanced_piscem_options(&mut piscem_quant_cmd, opts)?;
                    }
                    Err(_) => {
                        info!(
                            r#"
Simpleaf is currently using piscem version {}, but you must be using version >= 0.7.0 in order to use the 
mapping options specific to this, or later versions. If you wish to use these options, please upgrade your 
piscem version or, if you believe you have a sufficiently new version installed, update the executable 
being used by simpleaf"#,
                            &piscem_prog_info.version
                        );
                    }
                }

                let frag_lib_xform = add_or_transform_fragment_library(
                    MapperType::Piscem,
                    setup.chem.fragment_geometry_str(),
                    reads1,
                    reads2,
                    &mut piscem_quant_cmd,
                )?;

                let map_cmd_string = prog_utils::get_cmd_line_string(&piscem_quant_cmd);
                info!("piscem map-sc cmd : {}", map_cmd_string);
                prog_utils::check_piscem_index_files(index_base.as_path())?;
                let mut read_inputs = Vec::new();
                read_inputs.extend_from_slice(reads1);
                read_inputs.extend_from_slice(reads2);
                prog_utils::check_files_exist(&read_inputs)?;

                let map_start = Instant::now();
                exec::run_checked(&mut piscem_quant_cmd, "piscem [mapping phase]")?;
                match frag_lib_xform {
                    FragmentTransformationType::TransformedIntoFifo(xform_data) => {
                        match xform_data.join_handle.join() {
                            Ok(join_res) => {
                                let xform_stats = join_res?;
                                let total = xform_stats.total_fragments;
                                let failed = xform_stats.failed_parsing;
                                info!(
                                    "seq_geom_xform : observed {} input fragments. {} ({:.2}%) of them failed to parse and were not transformed",
                                    total,
                                    failed,
                                    if total > 0 {
                                        (failed as f64) / (total as f64)
                                    } else {
                                        0_f64
                                    } * 100_f64
                                );
                            }
                            Err(e) => {
                                bail!("Thread panicked with {:?}", e);
                            }
                        }
                    }
                    FragmentTransformationType::Identity => {}
                }

                Ok(MappingStageOutput {
                    sc_mapper: String::from("piscem"),
                    map_cmd_string,
                    map_output,
                    map_duration: map_start.elapsed(),
                })
            }
            IndexType::Salmon(index_base) => {
                let salmon_prog_info =
                    setup.rp.salmon.as_ref().context(
                        "A salmon index is being used, but salmon program info is missing.",
                    )?;
                let mut salmon_quant_cmd =
                    std::process::Command::new(format!("{}", salmon_prog_info.exe_path.display()));
                let index_path = format!("{}", index_base.display());
                salmon_quant_cmd
                    .arg("alevin")
                    .arg("--index")
                    .arg(index_path)
                    .arg("-l")
                    .arg("A");

                let frag_lib_xform = add_or_transform_fragment_library(
                    MapperType::Salmon,
                    setup.chem.fragment_geometry_str(),
                    reads1,
                    reads2,
                    &mut salmon_quant_cmd,
                )?;

                let map_output = opts.output.join("af_map");
                salmon_quant_cmd
                    .arg("--threads")
                    .arg(format!("{}", setup.threads))
                    .arg("-o")
                    .arg(&map_output);
                if opts.use_selective_alignment {
                    salmon_quant_cmd.arg("--rad");
                } else {
                    salmon_quant_cmd.arg("--sketch");
                }

                let map_cmd_string = prog_utils::get_cmd_line_string(&salmon_quant_cmd);
                info!("salmon alevin cmd : {}", map_cmd_string);
                let mut input_files = vec![index];
                input_files.extend_from_slice(reads1);
                input_files.extend_from_slice(reads2);
                prog_utils::check_files_exist(&input_files)?;

                let map_start = Instant::now();
                exec::run_checked(&mut salmon_quant_cmd, "salmon [mapping phase]")?;
                match frag_lib_xform {
                    FragmentTransformationType::TransformedIntoFifo(xform_data) => {
                        match xform_data.join_handle.join() {
                            Ok(join_res) => {
                                let xform_stats = join_res?;
                                let total = xform_stats.total_fragments;
                                let failed = xform_stats.failed_parsing;
                                info!(
                                    "seq_geom_xform : observed {} input fragments. {} ({:.2}%) of them failed to parse and were not transformed",
                                    total,
                                    failed,
                                    if total > 0 {
                                        (failed as f64) / (total as f64)
                                    } else {
                                        0_f64
                                    } * 100_f64
                                );
                            }
                            Err(e) => {
                                bail!("Thread panicked with {:?}", e);
                            }
                        }
                    }
                    FragmentTransformationType::Identity => {}
                }

                Ok(MappingStageOutput {
                    sc_mapper: String::from("salmon"),
                    map_cmd_string,
                    map_output,
                    map_duration: map_start.elapsed(),
                })
            }
            IndexType::NoIndex => {
                bail!(
                    "Cannot perform mapping an quantification without known (piscem or salmon) index!"
                );
            }
        }
    } else {
        Ok(MappingStageOutput {
            sc_mapper: String::new(),
            map_cmd_string: String::new(),
            map_output: opts
                .map_dir
                .clone()
                .context("map-dir must be provided, since index, read1 and read2 were not.")?,
            map_duration: Duration::new(0, 0),
        })
    }
}

fn run_quant_stage(
    opts: &MapQuantOpts,
    setup: &QuantSetup,
    mapping: &MappingStageOutput,
    pl_info: &mut CBListInfo,
) -> anyhow::Result<QuantStageOutput> {
    let gpl_output = opts.output.join("af_quant");
    std::fs::create_dir_all(&gpl_output).with_context(|| {
        format!(
            "Failed to create quantification output directory {}",
            gpl_output.display()
        )
    })?;

    let mapping_log = match &setup.index_type {
        IndexType::Piscem(_) => {
            let piscem_map_log_path = mapping.map_output.join("map_info.json");
            prog_parsing_utils::construct_json_from_piscem_log(piscem_map_log_path)?
        }
        IndexType::Salmon(_) => {
            let salmon_log_path = mapping.map_output.join("logs").join("salmon_quant.log");
            prog_parsing_utils::construct_json_from_salmon_log(salmon_log_path)?
        }
        IndexType::NoIndex => {
            serde_json::json!({
                "mapper" : "pre_mapped",
                "num_mapped": 0,
                "num_poisoned": 0,
                "num_reads": 0,
                "percent_mapped": 0.
            })
        }
    };
    let map_info_path = gpl_output.join("simpleaf_map_info.json");
    let map_info_file = std::fs::File::create(map_info_path)?;
    serde_json::to_writer(map_info_file, &mapping_log)?;

    let alevin_fry = setup
        .rp
        .alevin_fry
        .as_ref()
        .context("Alevin-fry program info is missing; please run `simpleaf set-paths`.")?
        .exe_path
        .clone();
    let mut alevin_gpl_cmd = std::process::Command::new(format!("{}", &alevin_fry.display()));
    let gpl_threads = setup.threads.min(8);
    alevin_gpl_cmd.arg("generate-permit-list");
    alevin_gpl_cmd.arg("-i").arg(&mapping.map_output);
    alevin_gpl_cmd.arg("-d").arg(setup.ori.as_str());
    alevin_gpl_cmd.arg("-t").arg(format!("{}", gpl_threads));
    setup.filter_meth.add_to_args(&mut alevin_gpl_cmd);
    alevin_gpl_cmd.arg("-o").arg(&gpl_output);
    let gpl_cmd_string = prog_utils::get_cmd_line_string(&alevin_gpl_cmd);
    info!("alevin-fry generate-permit-list cmd : {}", gpl_cmd_string);
    let input_files = vec![mapping.map_output.clone()];
    prog_utils::check_files_exist(&input_files)?;
    let gpl_start = Instant::now();
    exec::run_checked(&mut alevin_gpl_cmd, "[generate permit list]")?;
    let gpl_duration = gpl_start.elapsed();

    let mut alevin_collate_cmd = std::process::Command::new(format!("{}", &alevin_fry.display()));
    alevin_collate_cmd.arg("collate");
    alevin_collate_cmd.arg("-i").arg(&gpl_output);
    alevin_collate_cmd.arg("-r").arg(&mapping.map_output);
    alevin_collate_cmd
        .arg("-t")
        .arg(format!("{}", setup.threads));
    let collate_cmd_string = prog_utils::get_cmd_line_string(&alevin_collate_cmd);
    info!("alevin-fry collate cmd : {}", collate_cmd_string);
    let input_files = vec![gpl_output.clone(), mapping.map_output.clone()];
    prog_utils::check_files_exist(&input_files)?;
    let collate_start = Instant::now();
    exec::run_checked(&mut alevin_collate_cmd, "[collate]")?;
    let collate_duration = collate_start.elapsed();

    let mut alevin_quant_cmd = std::process::Command::new(format!("{}", &alevin_fry.display()));
    alevin_quant_cmd
        .arg("quant")
        .arg("-i")
        .arg(&gpl_output)
        .arg("-o")
        .arg(&gpl_output);
    alevin_quant_cmd.arg("-t").arg(format!("{}", setup.threads));
    alevin_quant_cmd.arg("-m").arg(setup.t2g_map_file.clone());
    alevin_quant_cmd.arg("-r").arg(&opts.resolution);
    let quant_cmd_string = prog_utils::get_cmd_line_string(&alevin_quant_cmd);
    info!("cmd : {:?}", alevin_quant_cmd);
    let input_files = vec![gpl_output.clone(), setup.t2g_map_file.clone()];
    prog_utils::check_files_exist(&input_files)?;
    let quant_start = Instant::now();
    exec::run_checked(&mut alevin_quant_cmd, "[quant]")?;
    let quant_duration = quant_start.elapsed();

    if let Some(gene_name_path) = &setup.gene_id_to_name_opt {
        let target_path = gpl_output.join("gene_id_to_name.tsv");
        match std::fs::copy(gene_name_path, &target_path) {
            Ok(_) => {
                info!(
                    "successfully copied the gene_name_to_id.tsv file into the quantification directory."
                );
            }
            Err(err) => {
                warn!(
                    "could not successfully copy gene_id_to_name file from {:?} to {:?} because of {:?}",
                    gene_name_path, target_path, err
                );
            }
        }
    }

    let quants_mat_rows_p = gpl_output.join("alevin").join("quants_mat_rows.txt");
    pl_info.update_af_quant_barcodes_tsv(&quants_mat_rows_p)?;

    Ok(QuantStageOutput {
        gpl_output,
        gpl_cmd_string,
        collate_cmd_string,
        quant_cmd_string,
        gpl_duration,
        collate_duration,
        quant_duration,
    })
}

fn write_quant_log(
    opts: &MapQuantOpts,
    mapping: &MappingStageOutput,
    quant_stage: &QuantStageOutput,
    convert_duration: Option<Duration>,
) -> anyhow::Result<()> {
    let af_quant_info_file = opts.output.join("simpleaf_quant_log.json");
    let mut af_quant_info = json!({
        "time_info" : {
        "map_time" : mapping.map_duration,
        "gpl_time" : quant_stage.gpl_duration,
        "collate_time" : quant_stage.collate_duration,
        "quant_time" : quant_stage.quant_duration
    },
        "cmd_info" : {
        "map_cmd" : mapping.map_cmd_string,
        "gpl_cmd" : quant_stage.gpl_cmd_string,
        "collate_cmd" : quant_stage.collate_cmd_string,
        "quant_cmd" : quant_stage.quant_cmd_string
    },
        "map_info" : {
        "mapper" : mapping.sc_mapper,
        "map_cmd" : mapping.map_cmd_string,
        "map_outdir": mapping.map_output.display().to_string()
    }
    });

    if let Some(ctime) = convert_duration {
        af_quant_info["time_info"]["conversion_time"] = json!(ctime);
    }

    io::write_json_pretty_atomic(&af_quant_info_file, &af_quant_info)?;
    Ok(())
}

pub fn map_and_quant(af_home_path: &Path, opts: MapQuantOpts) -> anyhow::Result<()> {
    validate_map_and_quant_opts(&opts)?;
    let (setup, mut pl_info) = resolve_quant_setup(af_home_path, &opts)?;
    let mapping = run_mapping_stage(&opts, &setup)?;
    let quant_stage = run_quant_stage(&opts, &setup, &mapping, &mut pl_info)?;

    let mut convert_duration = None;
    if opts.anndata_out {
        let convert_start = Instant::now();
        let opath = quant_stage.gpl_output.join("alevin").join("quants.h5ad");
        af_anndata::convert_csr_to_anndata(&quant_stage.gpl_output, &opath)?;
        convert_duration = Some(convert_start.elapsed());
    }

    write_quant_log(&opts, &mapping, &quant_stage, convert_duration)
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    use crate::utils::af_utils::RnaChemistry;
    use crate::{Cli, Commands};

    use super::*;

    fn parse_quant_opts(args: &[&str]) -> MapQuantOpts {
        let mut cli_args = vec!["simpleaf"];
        cli_args.extend_from_slice(args);
        match Cli::parse_from(cli_args).command {
            Commands::Quant(opts) => opts,
            cmd => panic!("expected quant command, found {:?}", cmd),
        }
    }

    fn minimal_no_index_setup() -> QuantSetup {
        QuantSetup {
            rp: ReqProgs {
                salmon: None,
                piscem: None,
                alevin_fry: None,
                macs: None,
            },
            index_type: IndexType::NoIndex,
            t2g_map_file: PathBuf::from("/tmp/t2g.tsv"),
            gene_id_to_name_opt: None,
            chem: Chemistry::Rna(RnaChemistry::TenxV3),
            ori: ExpectedOri::Forward,
            filter_meth: CellFilterMethod::KneeFinding,
            threads: 1,
        }
    }

    #[test]
    fn mapping_stage_no_index_uses_map_dir() {
        let opts = parse_quant_opts(&[
            "quant",
            "-c",
            "10xv3",
            "-o",
            "/tmp/out",
            "-r",
            "cr-like",
            "--knee",
            "--map-dir",
            "/tmp/mapped",
        ]);
        let setup = minimal_no_index_setup();
        let stage = run_mapping_stage(&opts, &setup).expect("mapping stage should succeed");
        assert_eq!(stage.map_output, PathBuf::from("/tmp/mapped"));
        assert_eq!(stage.sc_mapper, "");
    }

    #[test]
    fn mapping_stage_no_index_fails_without_map_dir() {
        let mut opts = parse_quant_opts(&[
            "quant",
            "-c",
            "10xv3",
            "-o",
            "/tmp/out",
            "-r",
            "cr-like",
            "--knee",
            "--map-dir",
            "/tmp/mapped",
        ]);
        opts.map_dir = None;
        opts.index = None;

        let setup = minimal_no_index_setup();
        let err = run_mapping_stage(&opts, &setup).expect_err("expected missing map-dir to fail");
        assert!(
            format!("{:#}", err).contains("map-dir must be provided"),
            "unexpected error: {:#}",
            err
        );
    }
}
