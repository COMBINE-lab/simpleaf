use crate::utils::af_utils;
use crate::utils::af_utils::{
    CellFilterMethod, Chemistry, FragmentTransformationType, MapperType, PermitListResult,
};
use crate::utils::prog_utils;
use crate::utils::prog_utils::{CommandVerbosityLevel, ReqProgs};

use anyhow::{bail, Context};
use serde_json::json;
use serde_json::Value;
use std::io::BufReader;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use tracing::{info, warn};

use super::Commands;

enum IndexType {
    Salmon(PathBuf),
    Piscem(PathBuf),
    NoIndex,
}

pub fn map_and_quant(af_home_path: &Path, quant_cmd: Commands) -> anyhow::Result<()> {
    match quant_cmd {
        Commands::Quant {
            index,
            use_piscem,
            map_dir,
            reads1,
            reads2,
            mut threads,
            use_selective_alignment,
            expected_ori,
            knee,
            unfiltered_pl,
            explicit_pl,
            forced_cells,
            expect_cells,
            min_reads,
            resolution,
            mut t2g_map,
            chemistry,
            output,
        } => {
            // Read the JSON contents of the file as an instance of `User`.
            let v: Value = prog_utils::inspect_af_home(af_home_path)?;
            let rp: ReqProgs = serde_json::from_value(v["prog_info"].clone())?;

            // info!("prog info = {:?}", rp);

            let mut had_simpleaf_index_json = false;
            let mut index_type_str = String::new();
            if let Some(index) = index.clone() {
                let index_json_path = index.join("simpleaf_index.json");
                match index_json_path.try_exists() {
                    Ok(true) => {
                        // we have the simpleaf_index.json file, so parse it.
                        let index_json_file =
                            std::fs::File::open(&index_json_path).with_context({
                                || format!("Could not open file {}", index_json_path.display())
                            })?;

                        let index_json_reader = BufReader::new(&index_json_file);
                        let v: Value = serde_json::from_reader(index_json_reader)?;
                        had_simpleaf_index_json = true;
                        index_type_str = serde_json::from_value(v["index_type"].clone())?;
                        // if the user didn't pass in a t2g_map, try and populate it
                        // automatically here
                        if t2g_map.is_none() {
                            let t2g_opt: Option<PathBuf> =
                                serde_json::from_value(v["t2g_file"].clone())?;
                            if let Some(t2g_val) = t2g_opt {
                                let t2g_loc = index.join(t2g_val);
                                info!("found local t2g file at {}, will attempt to use this since none was provided explicitly", t2g_loc.display());
                                t2g_map = Some(t2g_loc);
                            }
                        }
                    }
                    Ok(false) => {
                        had_simpleaf_index_json = false;
                    }
                    Err(e) => {
                        bail!(e);
                    }
                }
            }

            // at this point make sure we have a t2g value
            let t2g_map_file = t2g_map.context("A transcript-to-gene map (t2g) file was not provided via `--t2g-map`|`-m` and could \
                    not be inferred from the index. Please provide a t2g map explicitly to the quant command.")?;
            prog_utils::check_files_exist(&[t2g_map_file.clone()])?;

            // figure out what type of index we expect
            let index_type;
            // only bother with this if we are mapping reads and not if we are
            // starting from a RAD file
            if let Some(index) = index.clone() {
                // if the user said piscem explicitly, believe them
                if !use_piscem {
                    if had_simpleaf_index_json {
                        match index_type_str.as_ref() {
                            "salmon" => {
                                index_type = IndexType::Salmon(index);
                            }
                            "piscem" => {
                                index_type = IndexType::Piscem(index.join("piscem_idx"));
                            }
                            _ => {
                                bail!(
                                    "unknown index type {} present in simpleaf_index.json",
                                    index_type_str,
                                );
                            }
                        }
                    } else {
                        index_type = IndexType::Salmon(index);
                    }
                } else {
                    index_type = IndexType::Piscem(index);
                }
            } else {
                index_type = IndexType::NoIndex;
            }

            // make sure we have an program matching the
            // appropriate index type
            match index_type {
                IndexType::Piscem(_) => {
                    if rp.piscem.is_none() {
                        bail!("A piscem index is being used, but no piscem executable is provided. Please set one with `simpleaf set-paths`.");
                    }
                }
                IndexType::Salmon(_) => {
                    if rp.salmon.is_none() {
                        bail!("A salmon index is being used, but no piscem executable is provided. Please set one with `simpleaf set-paths`.");
                    }
                }
                IndexType::NoIndex => {}
            }

            // do we have a custom chemistry file
            let custom_chem_p = af_home_path.join("custom_chemistries.json");
            let custom_chem_exists = custom_chem_p.is_file();

            let chem = match chemistry.as_str() {
                "10xv2" => Chemistry::TenxV2,
                "10xv3" => Chemistry::TenxV3,
                s => {
                    if custom_chem_exists {
                        // parse the custom chemistry json file
                        let custom_chem_file =
                            std::fs::File::open(&custom_chem_p).with_context({
                                || {
                                    format!(
                                        "couldn't open the custom chemistry file {}",
                                        custom_chem_p.display()
                                    )
                                }
                            })?;
                        let custom_chem_reader = BufReader::new(custom_chem_file);
                        let v: Value = serde_json::from_reader(custom_chem_reader)?;
                        let rchem = match v[s.to_string()].as_str() {
                            Some(chem_str) => {
                                info!("custom chemistry {} maps to geometry {}", s, &chem_str);
                                Chemistry::Other(chem_str.to_string())
                            }
                            None => Chemistry::Other(s.to_string()),
                        };
                        rchem
                    } else {
                        // pass along whatever the user gave us
                        Chemistry::Other(s.to_string())
                    }
                }
            };

            let ori;
            // if the user set the orientation, then
            // use that explicitly
            if let Some(o) = expected_ori {
                ori = o;
            } else {
                // otherwise, this was not set explicitly. In that case
                // if we have 10xv2 or 10xv3 chemistry, set ori = "fw"
                // otherwise set ori = "both"
                match chem {
                    Chemistry::TenxV2 | Chemistry::TenxV3 => {
                        ori = "fw".to_string();
                    }
                    _ => {
                        ori = "both".to_string();
                    }
                }
            }

            let mut filter_meth_opt = None;

            // based on the filtering method
            if let Some(pl_file) = unfiltered_pl {
                // NOTE: unfiltered_pl is of type Option<Option<PathBuf>> so being in here
                // tells us nothing about the inner option.  We handle that now.

                // if the -u flag is passed and some file is provided, then the inner
                // Option is Some(PathBuf)
                if let Some(pl_file) = pl_file {
                    // the user has explicily passed a file along, so try
                    // to use that
                    if pl_file.is_file() {
                        let min_cells = min_reads;
                        filter_meth_opt = Some(CellFilterMethod::UnfilteredExternalList(
                            pl_file.to_string_lossy().into_owned(),
                            min_cells,
                        ));
                    } else {
                        bail!(
                            "The provided path {} does not exist as a regular file.",
                            pl_file.display()
                        );
                    }
                } else {
                    // here, the -u flag is provided
                    // but no file is provided, then the
                    // inner option is None and we will try to get the permit list automatically if
                    // using 10xv2 or 10xv3

                    // check the chemistry
                    let pl_res = af_utils::get_permit_if_absent(af_home_path, &chem)?;
                    let min_cells = min_reads;
                    match pl_res {
                        PermitListResult::DownloadSuccessful(p)
                        | PermitListResult::AlreadyPresent(p) => {
                            filter_meth_opt = Some(CellFilterMethod::UnfilteredExternalList(
                                p.to_string_lossy().into_owned(),
                                min_cells,
                            ));
                        }
                        PermitListResult::UnregisteredChemistry => {
                            bail!(
                                    "Cannot automatically obtain an unfiltered permit list for non-Chromium chemistry : {}.",
                                    chem.as_str()
                                    );
                        }
                    }
                }
            } else {
                if let Some(filtered_path) = explicit_pl {
                    filter_meth_opt = Some(CellFilterMethod::ExplicitList(
                        filtered_path.to_string_lossy().into_owned(),
                    ));
                };
                if let Some(num_forced) = forced_cells {
                    filter_meth_opt = Some(CellFilterMethod::ForceCells(num_forced));
                };
                if let Some(num_expected) = expect_cells {
                    filter_meth_opt = Some(CellFilterMethod::ExpectCells(num_expected));
                };
            }
            // otherwise it must have been knee;
            if knee {
                filter_meth_opt = Some(CellFilterMethod::KneeFinding);
            }

            if filter_meth_opt.is_none() {
                bail!("No valid filtering strategy was provided!");
            }

            // if the user requested more threads than can be used
            if let Ok(max_threads_usize) = std::thread::available_parallelism() {
                let max_threads = max_threads_usize.get() as u32;
                if threads > max_threads {
                    warn!(
                        "The maximum available parallelism is {}, but {} threads were requested.",
                        max_threads, threads
                    );
                    warn!("setting number of threads to {}", max_threads);
                    threads = max_threads;
                }
            }

            // here we must be safe to unwrap
            let filter_meth = filter_meth_opt.unwrap();

            let sc_mapper: String;
            let map_cmd_string: String;
            let map_output: PathBuf;
            let map_duration: Duration;

            // if we are mapping against an index
            if let Some(index) = index {
                let reads1 = reads1.expect(
                    "since mapping against an index is requested, read1 files must be provided.",
                );
                let reads2 = reads2.expect(
                    "since mapping against an index is requested, read2 files must be provided.",
                );
                assert_eq!(
                    reads1.len(),
                    reads2.len(),
                    "{} read1 files and {} read2 files were given; Cannot proceed!",
                    reads1.len(),
                    reads2.len()
                );

                match index_type {
                    IndexType::Piscem(index_base) => {
                        // using a piscem index
                        let mut piscem_quant_cmd = std::process::Command::new(format!(
                            "{}",
                            rp.piscem.unwrap().exe_path.display()
                        ));
                        let index_path = format!("{}", index_base.display());
                        piscem_quant_cmd
                            .arg("map-sc")
                            .arg("--index")
                            .arg(index_path);

                        // location of output directory, number of threads
                        map_output = output.join("af_map");
                        piscem_quant_cmd
                            .arg("--threads")
                            .arg(format!("{}", threads))
                            .arg("-o")
                            .arg(&map_output);

                        // check if we can parse the geometry directly, or if we are dealing with a
                        // "complex" geometry.
                        let frag_lib_xform = af_utils::add_or_transform_fragment_library(
                            MapperType::Piscem,
                            chem.as_str(),
                            &reads1,
                            &reads2,
                            &mut piscem_quant_cmd,
                        )?;

                        map_cmd_string = prog_utils::get_cmd_line_string(&piscem_quant_cmd);
                        info!("piscem map-sc cmd : {}", map_cmd_string);
                        sc_mapper = String::from("piscem");

                        let mut input_files = vec![
                            index_base.with_extension("ctab"),
                            index_base.with_extension("refinfo"),
                            index_base.with_extension("sshash"),
                        ];
                        input_files.extend_from_slice(&reads1);
                        input_files.extend_from_slice(&reads2);

                        prog_utils::check_files_exist(&input_files)?;

                        let map_start = Instant::now();
                        let cres = prog_utils::execute_command(
                            &mut piscem_quant_cmd,
                            CommandVerbosityLevel::Quiet,
                        )
                        .expect("failed to execute piscem [mapping phase]");

                        // if we had to filter the reads through a fifo
                        // wait for the thread feeding the fifo to finish
                        match frag_lib_xform {
                            FragmentTransformationType::TransformedIntoFifo(xform_data) => {
                                // wait for it to join
                                match xform_data.join_handle.join() {
                                    Ok(join_res) => {
                                        let xform_stats = join_res?;
                                        let total = xform_stats.total_fragments;
                                        let failed = xform_stats.failed_parsing;
                                        info!(
                                            "seq_geom_xform : observed {} input fragments. {} ({:.2}%) of them failed to parse and were not transformed",
                                            total, failed, if total > 0 { (failed as f64) / (total as f64) } else { 0_f64 } * 100_f64
                                        );
                                    }
                                    Err(e) => {
                                        bail!("Thread panicked with {:?}", e);
                                    }
                                }
                            }
                            FragmentTransformationType::Identity => {
                                // nothing to do.
                            }
                        }

                        map_duration = map_start.elapsed();

                        if !cres.status.success() {
                            bail!("piscem mapping failed with exit status {:?}", cres.status);
                        }
                    }
                    IndexType::Salmon(index_base) => {
                        // using a salmon index
                        let mut salmon_quant_cmd = std::process::Command::new(format!(
                            "{}",
                            rp.salmon.unwrap().exe_path.display()
                        ));

                        // set the input index and library type
                        let index_path = format!("{}", index_base.display());
                        salmon_quant_cmd
                            .arg("alevin")
                            .arg("--index")
                            .arg(index_path)
                            .arg("-l")
                            .arg("A");

                        // check if we can parse the geometry directly, or if we are dealing with a
                        // "complex" geometry.
                        let frag_lib_xform = af_utils::add_or_transform_fragment_library(
                            MapperType::Salmon,
                            chem.as_str(),
                            &reads1,
                            &reads2,
                            &mut salmon_quant_cmd,
                        )?;

                        // location of output directory, number of threads
                        map_output = output.join("af_map");
                        salmon_quant_cmd
                            .arg("--threads")
                            .arg(format!("{}", threads))
                            .arg("-o")
                            .arg(&map_output);

                        // if the user explicitly requested to use selective-alignment
                        // then enable that
                        if use_selective_alignment {
                            salmon_quant_cmd.arg("--rad");
                        } else {
                            // otherwise default to sketch mode
                            salmon_quant_cmd.arg("--sketch");
                        }

                        map_cmd_string = prog_utils::get_cmd_line_string(&salmon_quant_cmd);
                        info!("salmon alevin cmd : {}", map_cmd_string);
                        sc_mapper = String::from("salmon");

                        let mut input_files = vec![index];
                        input_files.extend_from_slice(&reads1);
                        input_files.extend_from_slice(&reads2);

                        prog_utils::check_files_exist(&input_files)?;

                        let map_start = Instant::now();
                        let cres = prog_utils::execute_command(
                            &mut salmon_quant_cmd,
                            CommandVerbosityLevel::Quiet,
                        )
                        .expect("failed to execute salmon [mapping phase]");

                        // if we had to filter the reads through a fifo
                        // wait for the thread feeding the fifo to finish
                        match frag_lib_xform {
                            FragmentTransformationType::TransformedIntoFifo(xform_data) => {
                                // wait for it to join
                                match xform_data.join_handle.join() {
                                    Ok(join_res) => {
                                        let xform_stats = join_res?;
                                        let total = xform_stats.total_fragments;
                                        let failed = xform_stats.failed_parsing;
                                        info!(
                                            "seq_geom_xform : observed {} input fragments. {} ({:.2}%) of them failed to parse and were not transformed",
                                            total, failed, if total > 0 { (failed as f64) / (total as f64) } else { 0_f64 } * 100_f64
                                        );
                                    }
                                    Err(e) => {
                                        bail!("Thread panicked with {:?}", e);
                                    }
                                }
                            }
                            FragmentTransformationType::Identity => {
                                // nothing to do.
                            }
                        }

                        map_duration = map_start.elapsed();

                        if !cres.status.success() {
                            bail!("salmon mapping failed with exit status {:?}", cres.status);
                        }
                    }
                    IndexType::NoIndex => {
                        bail!("Cannot perform mapping an quantification without known (piscem or salmon) index!");
                    }
                }
            } else {
                map_cmd_string = String::from("");
                sc_mapper = String::from("");
                map_output = map_dir
                    .expect("map-dir must be provided, since index, read1 and read2 were not.");
                map_duration = Duration::new(0, 0);
            }

            let map_output_string = map_output.display().to_string();

            let alevin_fry = rp.alevin_fry.unwrap().exe_path;
            // alevin-fry generate permit list
            let mut alevin_gpl_cmd =
                std::process::Command::new(format!("{}", &alevin_fry.display()));

            alevin_gpl_cmd.arg("generate-permit-list");
            alevin_gpl_cmd.arg("-i").arg(&map_output);
            alevin_gpl_cmd.arg("-d").arg(&ori);

            // add the filter mode
            af_utils::add_to_args(&filter_meth, &mut alevin_gpl_cmd);

            let gpl_output = output.join("af_quant");
            alevin_gpl_cmd.arg("-o").arg(&gpl_output);

            info!(
                "alevin-fry generate-permit-list cmd : {}",
                prog_utils::get_cmd_line_string(&alevin_gpl_cmd)
            );
            let input_files = vec![map_output.clone()];
            prog_utils::check_files_exist(&input_files)?;

            let gpl_start = Instant::now();
            let gpl_proc_out =
                prog_utils::execute_command(&mut alevin_gpl_cmd, CommandVerbosityLevel::Quiet)
                    .expect("could not execute [generate permit list]");
            let gpl_duration = gpl_start.elapsed();

            if !gpl_proc_out.status.success() {
                bail!(
                    "alevin-fry generate-permit-list failed with exit status {:?}",
                    gpl_proc_out.status
                );
            }

            //
            // collate
            //
            let mut alevin_collate_cmd =
                std::process::Command::new(format!("{}", &alevin_fry.display()));

            alevin_collate_cmd.arg("collate");
            alevin_collate_cmd.arg("-i").arg(&gpl_output);
            alevin_collate_cmd.arg("-r").arg(&map_output);
            alevin_collate_cmd.arg("-t").arg(format!("{}", threads));

            info!(
                "alevin-fry collate cmd : {}",
                prog_utils::get_cmd_line_string(&alevin_collate_cmd)
            );
            let input_files = vec![gpl_output.clone(), map_output];
            prog_utils::check_files_exist(&input_files)?;

            let collate_start = Instant::now();
            let collate_proc_out =
                prog_utils::execute_command(&mut alevin_collate_cmd, CommandVerbosityLevel::Quiet)
                    .expect("could not execute [collate]");
            let collate_duration = collate_start.elapsed();

            if !collate_proc_out.status.success() {
                bail!(
                    "alevin-fry collate failed with exit status {:?}",
                    collate_proc_out.status
                );
            }

            //
            // quant
            //
            let mut alevin_quant_cmd =
                std::process::Command::new(format!("{}", &alevin_fry.display()));

            alevin_quant_cmd
                .arg("quant")
                .arg("-i")
                .arg(&gpl_output)
                .arg("-o")
                .arg(&gpl_output);
            alevin_quant_cmd.arg("-t").arg(format!("{}", threads));
            alevin_quant_cmd.arg("-m").arg(t2g_map_file.clone());
            alevin_quant_cmd.arg("-r").arg(resolution);

            info!("cmd : {:?}", alevin_quant_cmd);

            let input_files = vec![gpl_output, t2g_map_file];
            prog_utils::check_files_exist(&input_files)?;

            let quant_start = Instant::now();
            let quant_proc_out =
                prog_utils::execute_command(&mut alevin_quant_cmd, CommandVerbosityLevel::Quiet)
                    .expect("could not execute [quant]");
            let quant_duration = quant_start.elapsed();

            if !quant_proc_out.status.success() {
                bail!("quant failed with exit status {:?}", quant_proc_out.status);
            }

            let af_quant_info_file = output.join("simpleaf_quant_log.json");
            let af_quant_info = json!({
                "time_info" : {
                    "map_time" : map_duration,
                    "gpl_time" : gpl_duration,
                    "collate_time" : collate_duration,
                    "quant_time" : quant_duration
                },
                "cmd_info" : {
                    "map_cmd" : map_cmd_string,
                    "gpl_cmd" : prog_utils::get_cmd_line_string(&alevin_gpl_cmd),
                    "collate_cmd" : prog_utils::get_cmd_line_string(&alevin_collate_cmd),
                    "quant_cmd" : prog_utils::get_cmd_line_string(&alevin_quant_cmd)
                    },
                "map_info" : {
                    "mapper" : sc_mapper,
                    "map_cmd" : map_cmd_string,
                    "map_outdir": map_output_string
                    }
            });

            // write the relevant info about
            // our run to file.
            std::fs::write(
                &af_quant_info_file,
                serde_json::to_string_pretty(&af_quant_info).unwrap(),
            )
            .with_context(|| format!("could not write {}", af_quant_info_file.display()))?;
        }
        _ => {
            bail!("unknown command")
        }
    }
    Ok(())
}
