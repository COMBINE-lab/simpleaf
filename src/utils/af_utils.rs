use anyhow::{anyhow, bail, Result};
use cmd_lib::run_fun;
use phf::phf_map;
use seq_geom_parser::{AppendToCmdArgs, FragmentGeomDesc, PiscemGeomDesc, SalmonSeparateGeomDesc};
use seq_geom_xform::{FifoXFormData, FragmentGeomDescExt};
use std::path::{Path, PathBuf};
use tracing::error;
//use ureq;
//use minreq::Response;

/// The map from pre-specified chemistry types that salmon knows
/// to the corresponding command line flag that salmon should be passed
/// to use this chemistry.
static KNOWN_CHEM_MAP_SALMON: phf::Map<&'static str, &'static str> = phf_map! {
        "10xv2" => "--chromium",
        "10xv3" => "--chromiumV3",
        "dropseq" => "--dropseq",
        "indropv2" => "--indropV2",
        "citeseq" => "--citeseq",
        "gemcode" => "--gemcode",
        "celseq" => "--celseq",
        "celseq2" => "--celseq2",
        "splitseqv1" => "--splitseqV1",
        "splitseqv2" => "--splitseqV2",
        "sciseq3" => "--sciseq3"
};

/// The map from pre-specified chemistry types that piscem knows
/// to the corresponding geometry name that piscem's `--geometry` option
/// should be passed to use this chemistry.
static KNOWN_CHEM_MAP_PISCEM: phf::Map<&'static str, &'static str> = phf_map! {
    "10xv2" => "chromium_v2",
    "10xv3" => "chromium_v3"
};

/// The types of "mappers" we know about
#[derive(Debug, Clone)]
pub enum MapperType {
    Salmon,
    Piscem,
    #[allow(dead_code)]
    MappedRadFile,
}

/// Were the reads fed directly to the mapper, or was
/// it transformed into fifos because they represent a
/// complex fragment library.
#[derive(Debug)]
pub enum FragmentTransformationType {
    Identity,
    TransformedIntoFifo(FifoXFormData),
}

#[derive(Debug, Clone)]
pub enum CellFilterMethod {
    // cut off at this cell in
    // the frequency sorted list
    ForceCells(usize),
    // use this cell as a hint in
    // the frequency sorted list
    ExpectCells(usize),
    // correct all cells in an
    // edit distance of 1 of these
    // barcodes
    ExplicitList(String),
    // barcodes will be provided in the
    // form of an *unfiltered* external
    // permit list
    UnfilteredExternalList(String, usize),
    // use the distance method to
    // automatically find the knee
    // in the curve
    KneeFinding,
}

pub fn add_to_args(fm: &CellFilterMethod, cmd: &mut std::process::Command) {
    match fm {
        CellFilterMethod::ForceCells(nc) => {
            cmd.arg("--force-cells").arg(format!("{}", nc));
        }
        CellFilterMethod::ExpectCells(nc) => {
            cmd.arg("--expect-cells").arg(format!("{}", nc));
        }
        CellFilterMethod::ExplicitList(l) => {
            cmd.arg("--valid-bc").arg(l);
        }
        CellFilterMethod::UnfilteredExternalList(l, m) => {
            cmd.arg("--unfiltered-pl")
                .arg(l)
                .arg("--min-reads")
                .arg(format!("{}", m));
        }
        CellFilterMethod::KneeFinding => {
            cmd.arg("--knee-distance");
        }
    }
}

pub enum Chemistry {
    TenxV2,
    TenxV3,
    Other(String),
}

impl Chemistry {
    pub fn as_str(&self) -> &str {
        match self {
            Chemistry::TenxV2 => "10xv2",
            Chemistry::TenxV3 => "10xv3",
            Chemistry::Other(s) => s.as_str(),
        }
    }
}

pub enum PermitListResult {
    DownloadSuccessful(PathBuf),
    AlreadyPresent(PathBuf),
    UnregisteredChemistry,
}

pub fn extract_geometry(geo: &str) -> Result<FragmentGeomDesc> {
    FragmentGeomDesc::try_from(geo)
}

pub fn add_chemistry_to_args_salmon(chem_str: &str, cmd: &mut std::process::Command) -> Result<()> {
    match KNOWN_CHEM_MAP_SALMON.get(chem_str) {
        Some(v) => {
            cmd.arg(v);
        }
        None => match extract_geometry(chem_str) {
            Ok(frag_desc) => {
                let salmon_desc = SalmonSeparateGeomDesc::from_geom_pieces(
                    &frag_desc.read1_desc,
                    &frag_desc.read2_desc,
                );
                salmon_desc.append(cmd);
            }
            Err(e) => {
                error!("{:?}", e);
                return Err(e);
            }
        },
    }
    Ok(())
}

pub fn add_chemistry_to_args_piscem(chem_str: &str, cmd: &mut std::process::Command) -> Result<()> {
    match KNOWN_CHEM_MAP_PISCEM.get(chem_str) {
        Some(v) => {
            cmd.arg("--geometry").arg(v);
        }
        None => match extract_geometry(chem_str) {
            Ok(frag_desc) => {
                let piscem_desc =
                    PiscemGeomDesc::from_geom_pieces(&frag_desc.read1_desc, &frag_desc.read2_desc);
                piscem_desc.append(cmd);
            }
            Err(e) => {
                error!("{:?}", e);
                return Err(e);
            }
        },
    }
    Ok(())
}

pub fn get_permit_if_absent(af_home: &Path, chem: &Chemistry) -> Result<PermitListResult> {
    let permit_dict_url = "https://raw.githubusercontent.com/COMBINE-lab/simpleaf/dev/resources/permit_list_info.json";
    let permit_dict: serde_json::Value = minreq::get(permit_dict_url)
        .send()?
        .json::<serde_json::Value>()?;
    let opt_chem_file: Option<String>;
    let opt_dl_url: Option<String>;
    match chem {
        Chemistry::TenxV2 => {
            if let Some(ref d) = permit_dict.get("10xv2") {
                opt_chem_file = if let Some(cf) = d
                    .get("filename")
                    .expect("value for filename field should be a string")
                    .as_str()
                {
                    Some(cf.to_string())
                } else {
                    None
                };
                opt_dl_url = if let Some(url) = d
                    .get("url")
                    .expect("value for url field should be a string")
                    .as_str()
                {
                    Some(url.to_string())
                } else {
                    None
                };
            } else {
                bail!(
                    "could not obtain \"10xv2\" key from the fetched permit_dict at {} = {:?}",
                    permit_dict_url,
                    permit_dict
                )
            }
        }
        Chemistry::TenxV3 => {
            if let Some(ref d) = permit_dict.get("10xv3") {
                opt_chem_file = if let Some(cf) = d
                    .get("filename")
                    .expect("value for filename field should be a string")
                    .as_str()
                {
                    Some(cf.to_string())
                } else {
                    None
                };
                opt_dl_url = if let Some(url) = d
                    .get("url")
                    .expect("value for url field should be a string")
                    .as_str()
                {
                    Some(url.to_string())
                } else {
                    None
                };
            } else {
                bail!(
                    "could not obtain \"10xv3\" key from the fetched permit_dict at {} = {:?}",
                    permit_dict_url,
                    permit_dict
                )
            }
        }
        _ => {
            return Ok(PermitListResult::UnregisteredChemistry);
        }
    }

    if let (Some(chem_file), Some(dl_url)) = (opt_chem_file, opt_dl_url) {
        let odir = af_home.join("plist");
        if odir.join(&chem_file).exists() {
            Ok(PermitListResult::AlreadyPresent(odir.join(&chem_file)))
        } else {
            run_fun!(mkdir -p $odir)?;

            let permit_request = minreq::get(dl_url).with_timeout(120).send()?;
            match permit_request.status_code {
                200..=299 => { 
                    // success
                },
                x =>  {
                    bail!("could not obtain the permit list; HTTP status code {}, reason {}", x, permit_request.reason_phrase);
                }
            }

            let mut permit_file = std::fs::File::create(odir.join(&chem_file).to_string_lossy().to_string())?;
            use std::io::Write;
            permit_file.write_all(permit_request.as_bytes())?;

            /*
            let mut dl_cmd = std::process::Command::new("wget");
            dl_cmd
                .arg("-v")
                .arg("-O")
                .arg(odir.join(&chem_file).to_string_lossy().to_string())
                .arg("-L")
                .arg(dl_url);
            let r = dl_cmd.output()?;
            if !r.status.success() {
                return Err(anyhow!("failed to download permit list {:?}", r.status));
            }
            */
            Ok(PermitListResult::DownloadSuccessful(odir.join(&chem_file)))
        }
    } else {
        bail!(
            "could not properly parse the permit dictionary obtained from {} = {:?}",
            permit_dict_url,
            permit_dict
        );
    }
}

/// This function performs the necessary work to register the fragment libraries represented by
/// `reads1` and `reads2` with the quantification command `quant_cmd`. The logic is as follows:
///
/// If the `fragment_geometry_str` is of a known known pre-specified type with respect to the
/// given `mapper_type`, then the reads are passed directly to the mapper along with the
/// appropriate geometry flag, and this function returns Ok(FragmentTransformationType::Identity).
///
/// Otherwise, the `fragment_geometry_str` is parsed in accordance with the fragment specification
/// description.
///
/// * If the `fragment_geometry_str` representes a "complex" geometry (i.e. a description with
/// an anchor or one or more bounded range parts), then the provided reads are passed through
/// the transformation function, and the fragment library is "normalized" to one with fixed
/// length geometry.  The new reads are written to a pair of fifos, and the mapper is provided
/// with the corresponding simplified geometry description.  In this case, the function returns
/// Ok(FragmentTransformationType::TransformedIntoFifo(FifoXFormData)), where the FifoXFormData
/// contains the names of the fifos being populated and a `JoinHandle` for the thread performing
/// the transformation.
///
/// * If the `fragment_geometry_str` represents a "simple" geometry, then the provided reads are
/// given directly to the underlying mapper and `fragment_geometry_str` is transformed into the
/// appropriate argument format for `mapper_type`.  In this case, the function returns
/// Ok(FragmentTransformationType::Identity).
///
/// In any case, if an error occurs, this function returns an anyhow::Error.
pub fn add_or_transform_fragment_library(
    mapper_type: MapperType,
    fragment_geometry_str: &str,
    reads1: &Vec<PathBuf>,
    reads2: &Vec<PathBuf>,
    quant_cmd: &mut std::process::Command,
) -> Result<FragmentTransformationType> {
    let known_chem = match mapper_type {
        MapperType::MappedRadFile => {
            bail!("Cannot add_or_transform_fragment library when dealing with an already-mapped RAD file.");
        }
        MapperType::Piscem => KNOWN_CHEM_MAP_PISCEM.contains_key(fragment_geometry_str),
        MapperType::Salmon => KNOWN_CHEM_MAP_SALMON.contains_key(fragment_geometry_str),
    };

    let frag_geom_opt = if !known_chem {
        Some(FragmentGeomDesc::try_from(fragment_geometry_str)?)
    } else {
        None
    };

    // We have a "complex" geometry, so transform the reads through a fifo
    match frag_geom_opt {
        Some(frag_geom) if frag_geom.is_complex_geometry() => {
            // parse into a "regex" description
            let regex_geo = frag_geom.as_regex()?;
            // the simplified geometry corresponding to this regex geo
            let simp_geo_string = regex_geo.get_simplified_description_string();

            // start a thread to transform our complex geometry into
            // simplified geometry
            let fifo_xform_data = seq_geom_xform::xform_read_pairs_to_fifo(
                regex_geo,
                reads1.clone(),
                reads2.clone(),
            )?;

            let r1_path = std::path::Path::new(&fifo_xform_data.r1_fifo);
            assert!(r1_path.exists());
            let r2_path = std::path::Path::new(&fifo_xform_data.r2_fifo);
            assert!(r2_path.exists());

            quant_cmd
                .arg("-1")
                .arg(fifo_xform_data.r1_fifo.to_string_lossy().into_owned());
            quant_cmd
                .arg("-2")
                .arg(fifo_xform_data.r2_fifo.to_string_lossy().into_owned());

            match mapper_type {
                MapperType::Piscem => {
                    add_chemistry_to_args_piscem(simp_geo_string.as_str(), quant_cmd)?;
                }
                MapperType::Salmon => {
                    add_chemistry_to_args_salmon(simp_geo_string.as_str(), quant_cmd)?;
                }
                MapperType::MappedRadFile => {
                    unimplemented!();
                }
            }
            Ok(FragmentTransformationType::TransformedIntoFifo(
                fifo_xform_data,
            ))
        }
        _ => {
            // just feed the reads directly to the mapper
            match mapper_type {
                MapperType::Piscem => {
                    let reads1_str = reads1
                        .iter()
                        .map(|x| x.to_string_lossy().into_owned())
                        .collect::<Vec<String>>()
                        .join(",");
                    quant_cmd.arg("-1").arg(reads1_str);

                    let reads2_str = reads2
                        .iter()
                        .map(|x| x.to_string_lossy().into_owned())
                        .collect::<Vec<String>>()
                        .join(",");
                    quant_cmd.arg("-2").arg(reads2_str);

                    add_chemistry_to_args_piscem(fragment_geometry_str, quant_cmd)?;
                }
                MapperType::Salmon => {
                    // location of the reads
                    // note: salmon uses space so separate
                    // these, not commas, so build the proper
                    // strings here.

                    quant_cmd.arg("-1");
                    for rf in reads1 {
                        quant_cmd.arg(rf);
                    }
                    quant_cmd.arg("-2");
                    for rf in reads2 {
                        quant_cmd.arg(rf);
                    }

                    // setting the technology / chemistry
                    add_chemistry_to_args_salmon(fragment_geometry_str, quant_cmd)?;
                }
                MapperType::MappedRadFile => {
                    unimplemented!();
                }
            }
            Ok(FragmentTransformationType::Identity)
        }
    }
}
