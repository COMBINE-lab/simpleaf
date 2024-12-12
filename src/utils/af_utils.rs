use anyhow::{bail, Result};
use cmd_lib::run_fun;
use phf::phf_map;
use seq_geom_parser::{AppendToCmdArgs, FragmentGeomDesc, PiscemGeomDesc, SalmonSeparateGeomDesc};
use seq_geom_xform::{FifoXFormData, FragmentGeomDescExt};
use std::fmt;
use std::path::{Path, PathBuf};

use strum_macros::EnumIter;
use tracing::error;

use crate::atac::commands::AtacChemistry;
use crate::utils::prog_utils;
//use ureq;
//use minreq::Response;

/// The map from pre-specified chemistry types that salmon knows
/// to the corresponding command line flag that salmon should be passed
/// to use this chemistry.
static KNOWN_CHEM_MAP_SALMON: phf::Map<&'static str, &'static str> = phf_map! {
        "10xv2" => "--chromium",
        "10xv3" => "--chromiumV3",
        // NOTE:: This is not a typo, the geometry for
        // the v3 and v4 chemistry are identical. Nonetheless,
        // we may want to still add an explicit flag to
        // salmon and change this when we bump the minimum
        // required version.
        "10xv4-3p" => "--chromiumV3",
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
    "10xv2-5p" => "chromium_v2_5p",
    "10xv3" => "chromium_v3",
    "10xv3-5p" => "chromium_v3_5p",
    "10xv4-3p" => "chromium_v4_3p"
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

/// The different alevin-fry supported methods for
/// permit-list generation.
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

impl CellFilterMethod {
    /// How a [CellFilterMethod] should add itself to an
    /// `alevin-fry` command.
    pub fn add_to_args(&self, cmd: &mut std::process::Command) {
        match self {
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
}

/// The builtin geometry types that have special handling to
/// reduce necessary options in the common case, as well as the
/// `Other` varant that covers custom geometries.
#[derive(Debug, PartialEq)]
pub enum Chemistry {
    Rna(RnaChemistry),
    Atac(AtacChemistry),
}

/// The builtin geometry types that have special handling to
/// reduce necessary options in the common case, as well as the
/// `Other` varant that covers custom geometries.
#[derive(EnumIter, Clone, PartialEq)]
pub enum RnaChemistry {
    TenxV2,
    TenxV25P,
    TenxV3,
    TenxV35P,
    TenxV43P,
    Other(String),
}

/// `&str` representations of the different geometries.
impl Chemistry {
    pub fn as_str(&self) -> &str {
        match self {
            Chemistry::Rna(rna_chem) => rna_chem.as_str(),
            Chemistry::Atac(atac_chem) => atac_chem.as_str(),
        }
    }
}

impl RnaChemistry {
    pub fn as_str(&self) -> &str {
        match self {
            RnaChemistry::TenxV2 => "10xv2",
            RnaChemistry::TenxV25P => "10xv2-5p",
            RnaChemistry::TenxV3 => "10xv3",
            RnaChemistry::TenxV35P => "10xv3-5p",
            RnaChemistry::TenxV43P => "10xv4-3p",
            RnaChemistry::Other(s) => s.as_str(),
        }
    }
}

/// [Debug] representations of the different geometries.
impl fmt::Debug for RnaChemistry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RnaChemistry::TenxV2 => write!(f, "10xv2"),
            RnaChemistry::TenxV25P => write!(f, "10xv2-5p"),
            RnaChemistry::TenxV3 => write!(f, "10xv3"),
            RnaChemistry::TenxV35P => write!(f, "10xv3-5p"),
            RnaChemistry::TenxV43P => write!(f, "10xv4-3p"),
            RnaChemistry::Other(s) => write!(f, "custom({})", s.as_str()),
        }
    }
}

/// The result of requesting a permit list
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
    // check if the file already exists
    let odir = af_home.join("plist");
    match chem {
        Chemistry::Rna(rna_chem) => match rna_chem {
            RnaChemistry::TenxV2 => {
                let chem_file = "10x_v2_permit.txt";
                if odir.join(chem_file).exists() {
                    return Ok(PermitListResult::AlreadyPresent(odir.join(chem_file)));
                }
            }
            RnaChemistry::TenxV25P => {
                // v2 and v2-5' use the same permit list
                let chem_file = "10x_v2_permit.txt";
                if odir.join(chem_file).exists() {
                    return Ok(PermitListResult::AlreadyPresent(odir.join(chem_file)));
                }
            }
            RnaChemistry::TenxV3 => {
                let chem_file = "10x_v3_permit.txt";
                if odir.join(chem_file).exists() {
                    return Ok(PermitListResult::AlreadyPresent(odir.join(chem_file)));
                }
            }
            RnaChemistry::TenxV35P => {
                let chem_file = "10x_v3_5p_permit.txt";
                if odir.join(chem_file).exists() {
                    return Ok(PermitListResult::AlreadyPresent(odir.join(chem_file)));
                }
            }
            RnaChemistry::TenxV43P => {
                let chem_file = "10x_v4_3p_permit.txt";
                if odir.join(chem_file).exists() {
                    return Ok(PermitListResult::AlreadyPresent(odir.join(chem_file)));
                }
            }
            _ => {
                return Ok(PermitListResult::UnregisteredChemistry);
            }
        },
        Chemistry::Atac(atac_chem) => match atac_chem {
            AtacChemistry::TenxV11 | AtacChemistry::TenxV2 => {
                let chem_file = "10x_atac_v1_v11_v2.txt";
                if odir.join(chem_file).exists() {
                    return Ok(PermitListResult::AlreadyPresent(odir.join(chem_file)));
                }
            }
            AtacChemistry::TenxMulti => {
                let chem_file = "10x_arc_atac_v1.txt";
                if odir.join(chem_file).exists() {
                    return Ok(PermitListResult::AlreadyPresent(odir.join(chem_file)));
                }
            }
        },
    }

    // the file doesn't exist, so get the json file that gives us
    // the chemistry name to permit list URL mapping.
    let permit_dict_url = "https://raw.githubusercontent.com/COMBINE-lab/simpleaf/dev/resources/permit_list_info.json";
    let request_result = minreq::get(permit_dict_url).send().inspect_err( |err| {
        error!("Could not obtain the permit list metadata from {}; encountered {:?}.", &permit_dict_url, &err);
        error!("This may be a transient failure, or could be because the client is lacking a network connection. \
        In the latter case, please consider manually providing the appropriate permit list file directly \
        via the command line to avoid an attempt by simpleaf to automatically obtain it.");
    })?;
    let permit_dict: serde_json::Value = request_result.json::<serde_json::Value>()?;
    let opt_chem_file: Option<String>;
    let opt_dl_url: Option<String>;
    // parse the JSON appropriately based on the chemistry we have
    match chem {
        Chemistry::Rna(rna_chem) => match rna_chem {
            RnaChemistry::TenxV2
            | RnaChemistry::TenxV25P
            | RnaChemistry::TenxV3
            | RnaChemistry::TenxV35P
            | RnaChemistry::TenxV43P => {
                let chem_key = chem.as_str();
                if let Some(d) = permit_dict.get(chem_key) {
                    opt_chem_file = d
                        .get("filename")
                        .expect("value for filename field should be a string")
                        .as_str()
                        .map(|cf| cf.to_string());
                    opt_dl_url = d
                        .get("url")
                        .expect("value for url field should be a string")
                        .as_str()
                        .map(|url| url.to_string());
                } else {
                    bail!(
                        "could not obtain \"{}\" key from the fetched permit_dict at {} = {:?}",
                        chem_key,
                        permit_dict_url,
                        permit_dict
                    )
                }
            }
            _ => {
                return Ok(PermitListResult::UnregisteredChemistry);
            }
        },
        Chemistry::Atac(atac_chem) => match atac_chem {
            AtacChemistry::TenxV11 | AtacChemistry::TenxV2 | AtacChemistry::TenxMulti => {
                let chem_key = atac_chem.resource_key();
                if let Some(d) = permit_dict.get(&chem_key) {
                    opt_chem_file = d
                        .get("filename")
                        .expect("value for filename field should be a string")
                        .as_str()
                        .map(|cf| cf.to_string());
                    opt_dl_url = d
                        .get("url")
                        .expect("value for url field should be a string")
                        .as_str()
                        .map(|url| url.to_string());
                } else {
                    bail!(
                        "could not obtain \"{}\" key from the fetched permit_dict at {} = {:?}",
                        chem_key,
                        permit_dict_url,
                        permit_dict
                    )
                }
            }
        },
    }

    // actually download the permit list if we need it and don't have it.
    if let (Some(chem_file), Some(dl_url)) = (opt_chem_file, opt_dl_url) {
        if odir.join(&chem_file).exists() {
            Ok(PermitListResult::AlreadyPresent(odir.join(&chem_file)))
        } else {
            run_fun!(mkdir -p $odir)?;

            let output_file = odir.join(&chem_file).to_string_lossy().to_string();
            prog_utils::download_to_file(dl_url, &output_file)?;

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
///   an anchor or one or more bounded range parts), then the provided reads are passed through
///   the transformation function, and the fragment library is "normalized" to one with fixed
///   length geometry.  The new reads are written to a pair of fifos, and the mapper is provided
///   with the corresponding simplified geometry description.  In this case, the function returns
///   Ok(FragmentTransformationType::TransformedIntoFifo(FifoXFormData)), where the FifoXFormData
///   contains the names of the fifos being populated and a `JoinHandle` for the thread performing
///   the transformation.
///
/// * If the `fragment_geometry_str` represents a "simple" geometry, then the provided reads are
///   given directly to the underlying mapper and `fragment_geometry_str` is transformed into the
///   appropriate argument format for `mapper_type`.  In this case, the function returns
///   Ok(FragmentTransformationType::Identity).
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
