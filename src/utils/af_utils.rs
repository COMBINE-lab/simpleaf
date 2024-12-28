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

use strum_macros::EnumIter;
use tracing::{debug, error, info, warn};

use crate::atac::commands::AtacChemistry;
use crate::utils::chem_utils::{get_single_custom_chem_from_file, CustomChemistry, ExpectedOri};
use crate::utils::{self, prog_utils};

use super::chem_utils::{QueryInRegistry, LOCAL_PL_PATH_KEY, REMOTE_PL_URL_KEY};

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

        // commented out because they are UMI free
        // "gemcode" => "--gemcode",
        // "celseq" => "--celseq",

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

#[derive(Debug, Clone, PartialEq)]
pub enum IndexType {
    Salmon(PathBuf),
    Piscem(PathBuf),
    NoIndex,
}

// Note that It is totally valid, even though
// "MapperType" instead of "IndexType" would be
// a better place to implement this method.
impl IndexType {
    pub fn is_known_chem(&self, chem: &str) -> bool {
        match self {
            IndexType::Salmon(_) => KNOWN_CHEM_MAP_SALMON.contains_key(chem),
            IndexType::Piscem(_) => KNOWN_CHEM_MAP_PISCEM.contains_key(chem),
            IndexType::NoIndex => {
                info!("Since we are dealing with an already-mapped RAD file, the user is responsible for ensuring that a valid chemistry definition was provided during mapping");
                KNOWN_CHEM_MAP_SALMON.contains_key(chem) || KNOWN_CHEM_MAP_PISCEM.contains_key(chem)
            }
        }
    }
    pub fn is_unsupported_known_chem(&self, chem: &str) -> bool {
        match self {
            IndexType::Salmon(_) => KNOWN_CHEM_MAP_PISCEM.contains_key(chem),
            IndexType::Piscem(_) => KNOWN_CHEM_MAP_SALMON.contains_key(chem),
            IndexType::NoIndex => {
                info!("Since we are dealing with an already-mapped RAD file, the user is responsible for ensuring that a valid chemistry definition was provided during mapping");
                !self.is_known_chem(chem)
            }
        }
    }
    pub fn as_str(&self) -> &str {
        match self {
            IndexType::Salmon(_) => "salmon",
            IndexType::Piscem(_) => "piscem",
            IndexType::NoIndex => "no_index",
        }
    }

    pub fn counterpart(&self) -> IndexType {
        match self {
            IndexType::Salmon(p) => IndexType::Piscem(p.clone()),
            IndexType::Piscem(p) => IndexType::Salmon(p.clone()),
            IndexType::NoIndex => IndexType::NoIndex,
        }
    }
}

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

impl QueryInRegistry for RnaChemistry {
    fn registry_key(&self) -> &str {
        self.as_str()
    }
}

impl RnaChemistry {
    pub fn expected_ori(&self) -> ExpectedOri {
        match self {
            RnaChemistry::TenxV2 | RnaChemistry::TenxV3 | RnaChemistry::TenxV43P => {
                ExpectedOri::Forward
            }
            RnaChemistry::TenxV25P | RnaChemistry::TenxV35P => {
                // NOTE: This is because we assume the piscem encoding
                // that is, these are treated as potentially paired-end protocols and
                // we infer the orientation of the fragment = orientation of read 1.
                // So, while the direction we want is the same as the 3' protocols
                // above, we separate out the case statement here for clarity.
                // Further, we may consider changing this or making it more robust if
                // and when we propagate more information about paired-end mappings.
                ExpectedOri::Forward
            }
            RnaChemistry::Other(x) => match x.as_str() {
                "sciseq3" | "splitseqv1" | "splitseqv2" | "dropseq" | "indropv2" | "citeseq" => {
                    ExpectedOri::Forward
                }
                "celseq2" => ExpectedOri::Reverse,
                _ => ExpectedOri::default(),
            },
        }
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

    pub fn expected_ori(&self) -> ExpectedOri {
        match self {
            Chemistry::Rna(x) => x.expected_ori(),
            Chemistry::Custom(custom_chem) => custom_chem.expected_ori().clone(),
            _ => ExpectedOri::default(),
        }
    }

    pub fn from_str(
        index_type: &IndexType,
        custom_chem_p: &Path,
        chem_str: &str,
    ) -> Result<Chemistry> {
        // First, we check if the chemistry is a 10x chem
        let chem = match chem_str {
            // 10xv2, 10xv3 and 10xv4-3p are valid in both mappers
            "10xv2" => Chemistry::Rna(RnaChemistry::TenxV2),
            "10xv3" => Chemistry::Rna(RnaChemistry::TenxV3),
            "10xv4-3p" => Chemistry::Rna(RnaChemistry::TenxV43P),
            // TODO: we want to keep the 10xv2-5p and 10xv3-5p
            // only because we want to directly assign their orientation as fw.
            // If we think we can retrieve its ori from chemistries.json, delete it
            // otherwise, delete the comment.
            "10xv3-5p" => match index_type {
                IndexType::Piscem(_) => Chemistry::Rna(RnaChemistry::TenxV35P),
                IndexType::NoIndex => {
                    info!("The 10xv3-5p chemistry flag is designed only for the piscem index. Please make sure the RAD file you are provided was mapped using piscem; otherwise the fragment orientations may not be treated correctly");
                    Chemistry::Rna(RnaChemistry::TenxV35P)
                }
                IndexType::Salmon(_) => {
                    bail!("The 10xv3-5p chemistry flag is not suppored under the salmon mapper. Instead please use the 10xv3 chemistry (which will treat samples as single-end).");
                }
            },
            "10xv2-5p" => match index_type {
                IndexType::Piscem(_) => Chemistry::Rna(RnaChemistry::TenxV25P),
                IndexType::NoIndex => {
                    info!("The 10xv2-5p chemistry flag is designed only for the piscem index. Please make sure the RAD file you are provided was mapped using piscem; otherwise the fragment orientations may not be treated correctly");
                    Chemistry::Rna(RnaChemistry::TenxV25P)
                }
                IndexType::Salmon(_) => {
                    bail!("The 10xv2-5p chemistry flag is not suppored under the salmon mapper. Instead please use the 10xv2 chemistry (which will treat samples as single-end).");
                }
            },
            s => {
                // first, we check if the chemistry is a known chemistry for the given mapper
                // Second, we check if its a registered custom chemistry
                // Third, we check if its a custom geometry string
                if index_type.is_known_chem(s) {
                    Chemistry::Rna(RnaChemistry::Other(s.to_string()))
                } else if let Some(chem) =
                    get_single_custom_chem_from_file(custom_chem_p, chem_str)?
                {
                    info!(
                        "custom chemistry {} maps to geometry {}",
                        s,
                        chem.geometry()
                    );
                    Chemistry::Custom(chem)
                } else {
                    // we want to bail with an error if the chemistry is *known* but
                    // not supported using this mapper (i.e. if it is a piscem-specific chem
                    // and the mapper is salmon or vice versa).
                    if index_type.is_unsupported_known_chem(s) {
                        bail!("The chemistry {} is not supported by the given mapper {}. Please switch to {}, provide the explicit geometry, or add this chemistry to the registry with the \"chemistry add\" command.", s, index_type.as_str(), index_type.counterpart().as_str());
                    }
                    Chemistry::Custom(CustomChemistry::simple_custom(s).with_context(|| {
                        format!(
                            "Could not parse the provided chemistry {}. Please ensure it is a valid chemistry string wrapped by quotes or that it is defined in the custom_chemistries.json file.",
                            s
                        )
                    })?)
                }
            }
        };

        Ok(chem)
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

/// Check if the provided directory `odir` exists, and create it if it doesn't.
/// Returns Ok(()) on success or an `anyhow::Error` otherwise.
pub fn create_dir_if_absent<T: AsRef<Path>>(odir: T) -> Result<()> {
    let pdir = odir.as_ref();
    if !pdir.exists() {
        info!(
            "The directory {} doesn't yet exist; attempting to create it.",
            pdir.display()
        );
        std::fs::create_dir(pdir)
            .with_context(|| format!("Couldn't create the directory at {}", pdir.display()))?;
    }
    Ok(())
}

/// Checks if the provided str `s` is a builtin (designated by starting with a double-underscore
/// "__").  If the provided string is a builtin, then return the part after the leading "__",
/// otherwise return `None`.
fn is_builtin(s: &str) -> Option<&str> {
    // anything starting with `__` is a built-in or reserved keyword, so
    // don't attempt to parse it as a geometry.
    if s.starts_with("__") {
        // calling `unwrap` here is OK because we
        // called `starts_with` above to determine we have a leading `__`
        let keyword = s.strip_prefix("__").unwrap();
        Some(keyword)
    } else {
        None
    }
}

pub fn validate_geometry(geo: &str) -> Result<()> {
    if let Some(builtin_kwd) = is_builtin(geo) {
        debug!(
            "geometry string started with \"__\" and so is reserved. Keyword :: [{}]",
            builtin_kwd
        );
        Ok(()) //bail!("The provided geometry is a builtin keyword [{}] (preceeded by \"__\") and so no attempt was made to parse it", builtin_kwd);
    } else {
        let fg = FragmentGeomDesc::try_from(geo);
        match fg {
            Ok(_fg) => Ok(()),
            Err(e) => {
                bail!("Could not parse geometry {}. Please ensure that it is a valid geometry definition wrapped by quotes. The error message was: {:?}", geo, e);
            }
        }
    }
}

pub fn extract_geometry(geo: &str) -> Result<FragmentGeomDesc> {
    if let Some(builtin_kwd) = is_builtin(geo) {
        bail!("The provided geometry is a builtin keyword [{}] (preceeded by \"__\") and so no attempt was made to parse it", builtin_kwd);
    } else {
        let fg = FragmentGeomDesc::try_from(geo);
        match fg {
            Ok(fg) => Ok(fg),
            Err(e) => {
                error!("Could not parse geometry {}. Please ensure that it is a valid geometry definition wrapped by quotes. The error message was: {:?}", geo, e);
                Err(e)
            }
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

/// This function tries to get permit list for this chemistry. The general algorithm is as follows:
/// * If this chemistry is unregistered, then we can't obtain a permit list
/// * If this chemistry is registered, but with no permit list, we can't obtain a permit list
/// * If it is registered and has a plist_name in the chemistries.json, we will look for that file.
///     - if that file is found, we use it (success)
///     - if that file is not found, we use the `remote_url` to download it (success)
///         - *NOTE* if there is a plist_name, but that files doesn't exist and there is no
///           remote_url, then something is wrong and registry is in a state it should not
///           be in (i.e. this should not be possible, and perhaps the user did something
///           like manually delete a file). In that case, they are responsible for fixing
///           it --- we should suggest they remove and re-add the chemistry
pub fn get_permit_if_absent(af_home: &Path, chem: &Chemistry) -> Result<PermitListResult> {
    // consult the chemistry file to see what the permit list for this should be
    let chem_registry_path = af_home.join(utils::constants::CHEMISTRIES_PATH);
    let chem_registry =
        parse_resource_json_file(&chem_registry_path, Some(utils::constants::CHEMISTRIES_URL))
            .with_context(|| {
                format!(
                    "couldn't obtain and/or parse {}",
                    chem_registry_path.display()
                )
            })?;

    // make sure the output directory exists
    let pdir = af_home.join("plist");
    create_dir_if_absent(&pdir)?;

    let expected_file_path;
    let expected_file_name;
    // we try to get the registry entry for this chemistry
    if let Some(reg_val) = chem_registry.get(chem.registry_key()) {
        if let Some(chem_obj) = reg_val.as_object() {
            // check if the resource has a local url
            match chem_obj.get(LOCAL_PL_PATH_KEY) {
                // if we didn't have this key or the value was explicitly
                // null, then we don't even have a place to put this file
                // when we download it, so it's an error.
                None | Some(Value::Null) => {
                    error!("No permit list is registered for {}, so one cannot be used automatically. You should either provide a permit list directly on the command line, or re-add this chemistry to the registry with a permit list.", chem.registry_key());
                    bail!("No registered permit list available");
                }
                Some(Value::String(lpath)) => {
                    expected_file_name = PathBuf::from(lpath);
                    expected_file_path = pdir.join(&expected_file_name);
                    if expected_file_path.is_file() {
                        return Ok(PermitListResult::AlreadyPresent(expected_file_path));
                    } else {
                        info!("Expected {} but didn't find it, will try to download it using a remote url.", expected_file_path.display());
                    }
                }
                _ => {
                    error!("Expected a JSON string associated with the {} key, but didn't find one; cannot proceed.", LOCAL_PL_PATH_KEY);
                    bail!("Wrong JSON type");
                }
            }

            // There was a plist_name, but the file was not present; try to get from the remote url
            match chem_obj.get(REMOTE_PL_URL_KEY) {
                // if we didn't have this key or the value was explicitly
                // null then we are out of luck.
                None => {
                    error!(
                        "The chemistry {} is registered in {} with the local permit list file {}.
However, no such file was present, and no remote url was provided from which 
to obtain it. This should not happen! It could occur if, for example, a 
permit list was added using a local-url and later (manually) removed. However, 
the chemistry registry should only be modified using the `chemistry` command.
Please consider removing and re-adding this chemistry with a valid permit list.",
                        chem.as_str(),
                        chem_registry_path.display(),
                        expected_file_path.display()
                    );
                    bail!("Expected permit list was absent, and no remote source was provided.");
                }
                Some(Value::String(rpath)) => {
                    // download the file
                    let hash =
                        prog_utils::download_to_file_compute_hash(rpath, &expected_file_path)?;
                    let expected_hash = expected_file_name
                        .to_str()
                        .ok_or(anyhow!("cannot convert expected filename to proper string"))?;
                    if hash.to_string() != expected_hash {
                        warn!("The permit list file for {}, obtained from the provided remote url {}, does not match the expected hash {} (the observed hash was {})", chem.registry_key(), rpath, expected_hash, hash.to_string());
                    }
                    Ok(PermitListResult::DownloadSuccessful(expected_file_path))
                }
                _ => {
                    error!("Expected a JSON string associated with the {} key, but didn't find one; cannot proceed.", REMOTE_PL_URL_KEY);
                    bail!("Wrong JSON type");
                }
            }
        } else {
            error!("The chemistry entry should be an object.");
            bail!("Found a non-object associated with this chemistry key");
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

#[cfg(test)]
mod tests;
