use anyhow::{anyhow, bail, Context, Result};
// use cmd_lib::run_fun;
use phf::phf_map;
use seq_geom_parser::{AppendToCmdArgs, FragmentGeomDesc, PiscemGeomDesc, SalmonSeparateGeomDesc};
use seq_geom_xform::{FifoXFormData, FragmentGeomDescExt};
use serde_json;
use serde_json::{json, Value};
use std::fmt;
use std::io::BufReader;
use std::path::{Path, PathBuf};

use strum::IntoEnumIterator;
use strum_macros::EnumIter;
use tracing::{error, info, warn};

use crate::atac::commands::AtacChemistry;
use crate::utils::chem_utils::{CustomChemistry, LOCAL_PL_PATH_KEY};
use crate::utils::prog_utils;

//use ureq;
//use minreq::Response;

// TODO: Update the path while merging
static PERMIT_LIST_INFO_VERSION: &str = "0.1.0";
static PERMIT_LIST_INFO_URL: &str = "https://raw.githubusercontent.com/an-altosian/simpleaf/spatial/resources/permit_list_info.json";
// "https://raw.githubusercontent.com/COMBINE-lab/simpleaf/dev/resources/permit_list_info.json";

// static CUSTOM_CHEMISTRIES_VERSION: &str = "0.1.0";
// "https://raw.githubusercontent.com/COMBINE-lab/simpleaf/dev/resources/custom_chem.json";

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

/// This function try to get permit list from five different sources with the following order:
/// 1. If it has a local_pl_path in the custom_chemistries.json, it will use it.
/// 2. If it has a remote_pl_url in the custom_chemistries.json and the default download path is a file, it will use the default download path.
/// 3. If it has a remote_pl_url in the custom_chemistries.json and the default download path doesn't exist, it will download the file from url to the default path.
/// 4. If it has a permit list file defined in the permit_list_info.json, it will use it.
/// 4. If it has a remote url in the custom chemistry in the permit_list_info.json, it will download the file from the remote url to the defined path and use it
// TODO: if we can combine the permit_list_info.json and custom_chemistries.json, we can simplify the logic and only check a single file
pub fn get_permit_if_absent(af_home: &Path, chem: &Chemistry) -> Result<PermitListResult> {
    let odir = af_home.join("plist");
    if !odir.exists() {
        std::fs::create_dir(&odir).with_context(|| {
            format!(
                "Couldn't create the permit list directory at {}",
                odir.display()
            )
        })?;
    }

    // define pl_path and url
    let mut local_pl_path: PathBuf = odir.join(chem.as_str());
    local_pl_path.set_extension("txt");
    let mut remote_pl_url: Option<String> = None;

    // FIRST TRY
    // the first try will be to get the pl file from the custom chemistry
    if let Chemistry::Custom(custom_chem) = chem {
        info!("Try to get the permit list file from the custom chemistry");
        // if we have local pl path, we should use it
        if let Some(lpp) = custom_chem.local_pl_path() {
            local_pl_path = PathBuf::from(lpp);
            // we assume it is an absolute path
            if local_pl_path.is_file() {
                info!(
                    "Use local permit list file recorded in {} at {:#?}",
                    LOCAL_PL_PATH_KEY, local_pl_path
                );
                return Ok(PermitListResult::AlreadyPresent(local_pl_path));
            } else if odir.join(lpp).is_file() {
                // we assume it is a file name in the plist directory
                info!("Use local permit list file at {:#?}", odir.join(lpp));
                return Ok(PermitListResult::AlreadyPresent(odir.join(lpp)));
            } else {
                warn!(
                    "Couldn't find the local permit list file recorded in {} at {:#?}",
                    LOCAL_PL_PATH_KEY, local_pl_path
                );
            }
        } else if let Some(rpu) = custom_chem.remote_pl_url() {
            // SECOND TRY
            // we check if the default download path exists
            if local_pl_path.is_file() {
                info!("Use downloaded permit list file at {:#?}", local_pl_path);
                return Ok(PermitListResult::AlreadyPresent(local_pl_path));
            } else {
                remote_pl_url = Some(rpu.to_string());
            }
        }
    }

    // the second try is to get the local file path from the permit_list_info.json file
    // check if the permit_list_info.json file exists
    // if we have permit list file in af_home, there should be a permit_list_info.json file
    // if it's not there, we should download it
    info!("Try to get the permit list file from predefined permit list info file");

    let permit_info_p = af_home.join("permit_list_info.json");
    let v: Value = parse_resource_json_file(&permit_info_p, Some(PERMIT_LIST_INFO_URL))?;

    let fake_version = json!("0.0.0");
    // get the version. If it is an old version, suggest the user to delete it
    let version = v
        .get("version")
        .unwrap_or(&fake_version)
        .as_str()
        .with_context(|| {
            format!(
                "value for version field should be a string from the permit_list_info.json file. Please report this issue onto the simpleaf github repository. The value obtained was {:?}",
                v
            )
        })?;

    match prog_utils::check_version_constraints(
        "permit_list_info.json",
        ">=".to_string() + PERMIT_LIST_INFO_VERSION,
        version,
    ) {
        Ok(af_ver) => info!("found permit_list_info.json version {:#}; Proceeding", af_ver),
        Err(_) => warn!("found outdated permit list info file with version {}. Please consider delete it from {:#?}.", version, &permit_info_p)
    }

    // get chemistry name
    let chem_name = chem.as_str();

    // THIRD TRY
    // get the permit list file name and url if its in the permit info file
    if let Some(chem_info) = v.get(chem_name) {
        info!(
            "Chemistry {} is registered in the permit_list_info.json file",
            chem_name
        );
        // get chemistry file name
        let chem_filename = chem_info
            .get("filename")
            .with_context(|| {
                format!(
                    "could not obtain the filename field for chemistry {} from the permit_list_info.json file. Please report this issue onto the simpleaf github repository. The value obtained was {:?}",
                    chem_name,
                    chem_info
                )
            })?
            .as_str()
            .with_context(|| {
                format!(
                    "value for filename field should be a string for chemistry {} from the permit_list_info.json file. Please report this issue onto the simpleaf github repository. The value obtained was {:?}",
                    chem_name,
                    chem_info
                )
            })?;

        //if it exists, return the path
        if odir.join(chem_filename).is_file() {
            info!("Use permit list file at {:#?}", odir.join(chem_filename));
            return Ok(PermitListResult::AlreadyPresent(odir.join(chem_filename)));
        }

        // we update the url if we did not get it from the custom chemistry
        if remote_pl_url.is_none() {
            let dl_url = chem_info
                .get("url")
                .with_context(|| {
                    format!(
                        "could not obtain the url field for chemistry {} from the permit_list_info.json file. Please report this issue onto the simpleaf github repository. The value obtained was {:?}",
                        chem_name,
                        chem_info
                    )
                })?
                .as_str()
                .with_context(|| {
                    format!(
                        "value for url field should be a string for chemistry {} from the permit_list_info.json file. Please report this issue onto the simpleaf github repository. The value obtained was {:?}",
                        chem_name,
                        chem_info
                    )
                })?
                .to_string();

            // update the url and corresponding local path
            remote_pl_url = Some(dl_url);
            local_pl_path = odir.join(chem_filename);
        }
    }

    // LAST TRY
    // we download the file from the remote url
    if let Some(url) = remote_pl_url {
        // download the file
        prog_utils::download_to_file(url, &local_pl_path)?;
        Ok(PermitListResult::DownloadSuccessful(local_pl_path))
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
