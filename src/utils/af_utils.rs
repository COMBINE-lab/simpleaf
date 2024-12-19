use anyhow::{anyhow, bail, Context, Result};
// use cmd_lib::run_fun;
use phf::phf_map;
use seq_geom_parser::{AppendToCmdArgs, FragmentGeomDesc, PiscemGeomDesc, SalmonSeparateGeomDesc};
use seq_geom_xform::{FifoXFormData, FragmentGeomDescExt};
use serde_json;
use serde_json::Value;
use std::fmt;
use std::io::BufReader;
use std::path::{Path, PathBuf};

use strum::IntoEnumIterator;
use strum_macros::EnumIter;
use tracing::{error, info, warn};

use crate::atac::commands::AtacChemistry;
use crate::utils::chem_utils::{CustomChemistry, LOCAL_PL_PATH_KEY, REMOTE_PL_URL_KEY};
use crate::utils::{self, prog_utils};

use super::chem_utils::QueryInRegistry;

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
    Custom(CustomChemistry),
}

impl QueryInRegistry for Chemistry {
    fn registry_key(&self) -> &str {
        match self {
            Chemistry::Rna(rc) => rc.registry_key(),
            Chemistry::Atac(ac) => ac.registry_key(),
            Chemistry::Custom(cc) => cc.registry_key(),
        }
    }
}

/// The builtin geometry types that have special handling to
/// reduce necessary options in the common case, as well as the
/// `Other` variant that covers custom geometries.
#[derive(EnumIter, Clone, PartialEq)]
pub enum RnaChemistry {
    TenxV2,
    TenxV25P,
    TenxV3,
    TenxV35P,
    TenxV43P,
    Other(String), // this will never be used because we have Chemistry::Custom
}

impl QueryInRegistry for RnaChemistry {
    fn registry_key(&self) -> &str {
        self.as_str()
    }
}

/// `&str` representations of the different geometries.
impl Chemistry {
    pub fn as_str(&self) -> &str {
        match self {
            Chemistry::Rna(rna_chem) => rna_chem.as_str(),
            Chemistry::Atac(atac_chem) => atac_chem.as_str(),
            Chemistry::Custom(custom_chem) => custom_chem.name.as_str(),
        }
    }

    pub fn fragment_geometry_str(&self) -> &str {
        match self {
            Chemistry::Rna(rna_chem) => rna_chem.as_str(),
            Chemistry::Atac(atac_chem) => atac_chem.as_str(),
            Chemistry::Custom(custom_chem) => custom_chem.geometry(),
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
    MissingPermitKeys,
}

pub fn validate_geometry(geo: &str) -> Result<()> {
    if geo != "__builtin" {
        let fg = FragmentGeomDesc::try_from(geo);
        return match fg {
            Ok(_fg) => Ok(()),
            Err(e) => {
                bail!("Could not parse geometry {}. Please ensure that it is a valid geometry definition wrapped by quotes. The error message was: {:?}", geo, e);
            }
        };
    }
    Ok(())
}

pub fn extract_geometry(geo: &str) -> Result<FragmentGeomDesc> {
    let fg = FragmentGeomDesc::try_from(geo);
    match fg {
        Ok(fg) => Ok(fg),
        Err(e) => {
            error!("Could not parse geometry {}. Please ensure that it is a valid geometry definition wrapped by quotes. The error message was: {:?}", geo, e);
            Err(e)
        }
    }
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

/// This function try to get permit list for this chemistry. The general algorithm is as follows:
/// * If this chemsitry is unregistered, then we can't obtain a permit list
/// * If it is registered and has a plist_name in the chemistries.json, we will look for that file.
///     - If it is registered and does not have a plist_name, we'll construct a temporary one as
///       chemistry + ".txt"
/// * If the file at plist_name exists, then use it (success)
/// * If the file at plist_name doesn't exist, check for a remote_url key
/// * If a remote_url key exists, then download the file and place it in the file plist_name
///   (success)
/// * If no remote_url key exists, then inform the user that this chemsitry has no keys for
///   obtaining a permit list
pub fn get_permit_if_absent(af_home: &Path, chem: &Chemistry) -> Result<PermitListResult> {
    // consult the chemistry file to see what the permit list for this should be
    let chem_registry_path = af_home.join(utils::constants::CHEMISTRIES_PATH);
    // get the existing chemistyr registry, or try to download it
    let chem_registry =
        parse_resource_json_file(&chem_registry_path, Some(utils::constants::CHEMISTRIES_URL))?;

    let registry_key = chem.registry_key();

    if let Some(reg_entry) = chem_registry.get(registry_key) {
        let reg_map = reg_entry.as_object().with_context(|| {
            format!(
                "The entry for registry key {} should be a proper JSON object",
                registry_key
            )
        })?;

        let has_local_name;
        let local_path;
        // check if the resource has a local url
        match reg_map.get(LOCAL_PL_PATH_KEY) {
            // if we didn't have this key or the value was explicitly
            // null, then we don't even have a place to put this file
            // when we download it, so it's an error.
            None | Some(serde_json::Value::Null) => {
                has_local_name = false;
                local_path = PathBuf::from(registry_key).with_extension("txt");
            }
            Some(lpath) => {
                let lpath = lpath.as_str().with_context(|| {
                    format!(
                        "expected the local url for {}, which was {:#}, to be a string!",
                        registry_key, lpath
                    )
                })?;
                local_path = PathBuf::from(lpath);
                has_local_name = true;
            }
        }

        let pdir = af_home.join("plist");

        // if we got a name for a local file, check if it exists
        let local_permit_file = pdir.join(local_path);
        if has_local_name && local_permit_file.is_file() {
            return Ok(PermitListResult::AlreadyPresent(local_permit_file));
        }

        if !pdir.exists() {
            info!(
                "The permit list directory ({}) doesn't yet exist; attempting to create it.",
                pdir.display()
            );
            std::fs::create_dir(&pdir).with_context(|| {
                format!(
                    "Couldn't create the permit list directory at {}",
                    pdir.display()
                )
            })?;
        }

        // either we made the name up, or we had a name but the file
        // wasn't present. In either case, we now want the remote url.
        match reg_map.get(REMOTE_PL_URL_KEY) {
            // if we didn't have this key or the value was explicitly
            // null then we are out of luck.
            None | Some(serde_json::Value::Null) => {
                // if we had a local name, then this is a "registered chemistry" but
                // there is no way to obtain the permit list, so the user should place
                // the file there explicitly or provide a download url.
                if has_local_name {
                    warn!("The chemistry {} is registered in {} with the local permit list file {}.
                          However, no such file was present, and no remote url was provided from which 
                          to obtain it. Please either register a local permit list for this chemistry
                          or provide a \"remote_url\" for this chemistry in the file {} to allow 
                          downloading it.",
                        chem.as_str(), chem_registry_path.display(), local_permit_file.display(),
                        chem_registry_path.display());
                    Ok(PermitListResult::MissingPermitKeys)
                } else {
                    warn!("The chemistry {} is registered in {} but it has no associated local \"plist_name\" or \"remote url\".
                          If there is an associated permit list for this chemistry, please update the entry 
                          associated with this chemistry in {} to reflect the proper file.",
                        chem.as_str(), chem_registry_path.display(), chem_registry_path.display());
                    Ok(PermitListResult::MissingPermitKeys)
                }
            }
            Some(rpath) => {
                let rpath = rpath.as_str().with_context(|| {
                    format!(
                        "expected the remote url for {}, which was {:#}, to be a string!",
                        registry_key, rpath
                    )
                })?;
                // download the file
                prog_utils::download_to_file(rpath, &local_permit_file)?;
                Ok(PermitListResult::DownloadSuccessful(local_permit_file))
            }
        }
    } else {
        Ok(PermitListResult::UnregisteredChemistry)
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

#[derive(Debug, Clone, PartialEq, EnumIter)]
pub enum ExpectedOri {
    Forward,
    Reverse,
    Both,
}

impl std::fmt::Display for ExpectedOri {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{:?}", self.as_str())
    }
}

impl ExpectedOri {
    pub fn as_str(&self) -> &str {
        match self {
            ExpectedOri::Forward => "fw",
            ExpectedOri::Reverse => "rc",
            ExpectedOri::Both => "both",
        }
    }

    // construct the expected_ori from a str
    pub fn from_str(s: &str) -> Result<ExpectedOri> {
        match s {
            "fw" => Ok(ExpectedOri::Forward),
            "rc" => Ok(ExpectedOri::Reverse),
            "both" => Ok(ExpectedOri::Both),
            _ => Err(anyhow!("Invalid expected_ori value: {}", s)),
        }
    }

    pub fn all_to_str() -> Vec<String> {
        ExpectedOri::iter()
            .map(|v| v.to_string())
            .collect::<Vec<String>>()
    }
}

pub fn parse_resource_json_file(p: &Path, url: Option<&str>) -> Result<Value> {
    // check if the custom_chemistries.json file exists
    let resource_exists = p.is_file();

    // get the file
    if !resource_exists {
        if let Some(dl_url) = url {
            // download the custom_chemistries.json file if needed
            prog_utils::download_to_file(dl_url, p)?;
        } else {
            bail!(
                "could not find resource {}, and no remote url was provided",
                p.display()
            );
        }
    }

    // load the file
    let resource_file = std::fs::File::open(p).with_context(|| {
        format!(
            "Couldn't open the existing resource file. Please consider delete it from {}",
            p.display()
        )
    })?;
    let resource_reader = BufReader::new(resource_file);
    serde_json::from_reader(resource_reader).with_context(|| {
        format!(
            "Couldn't parse the existing resource file. Please consider delete it from {}",
            p.display()
        )
    })
}
