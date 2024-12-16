use anyhow::{anyhow, bail, Context, Result};
// use cmd_lib::run_fun;
use phf::phf_map;
use seq_geom_parser::{AppendToCmdArgs, FragmentGeomDesc, PiscemGeomDesc, SalmonSeparateGeomDesc};
use seq_geom_xform::{FifoXFormData, FragmentGeomDescExt};
use serde_json;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::fmt;
use std::io::BufReader;
use std::path::{Path, PathBuf};

use strum_macros::EnumIter;
use tracing::{error, info, warn};

use crate::atac::commands::AtacChemistry;
use crate::utils::prog_utils;
//use ureq;
//use minreq::Response;

// TODO: Update the path while merging
static PERMIT_LIST_INFO_VERSION: &str = "0.1.0";
static PERMIT_LIST_INFO_URL: &str = "https://raw.githubusercontent.com/an-altosian/simpleaf/spatial/resources/permit_list_info.json";
// "https://raw.githubusercontent.com/COMBINE-lab/simpleaf/dev/resources/permit_list_info.json";

static CUSTOM_CHEMISTRIES_VERSION: &str = "0.1.0";
static CUSTOM_CHEMISTRIES_URL: &str = "https://raw.githubusercontent.com/an-altosian/simpleaf/spatial/resources/custom_chemistries.json";
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
    //check if the permit_list_info.json file exists
    // if we have permit list file in af_home, there should be a permit_list_info.json file
    // if it's not there, we should download it
    let permit_info_p = af_home.join("permit_list_info.json");
    if !permit_info_p.exists() {
        // download the permit_list_info.json file if needed
        prog_utils::download_to_file(PERMIT_LIST_INFO_URL, &permit_info_p)?;
    }
    // read the permit_list_info.json file
    let permit_info_file = std::fs::File::open(&permit_info_p)?;
    let permit_info_reader = BufReader::new(permit_info_file);
    let v: Value = serde_json::from_reader(permit_info_reader)?;

    let fake_version = json!("0.0.1");
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
        Err(_) => warn!("found outdated permit list info file (with version {}) at {:#?}. Please consider delete it.", version, &permit_info_p)
    }

    // get chemistry name
    let chem_name = chem.as_str();

    // check if the file already exists
    let odir = af_home.join("plist");

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
            })?
            .to_string();

        //if it exists, return the path
        if odir.join(&chem_filename).is_file() {
            return Ok(PermitListResult::AlreadyPresent(odir.join(&chem_filename)));
        }

        // now, we download it
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

        // download the file
        let output_file = odir.join(&chem_filename);
        prog_utils::download_to_file(dl_url, &output_file)?;
        Ok(PermitListResult::DownloadSuccessful(output_file))
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
}
#[derive(Debug, Clone, PartialEq)]
#[allow(dead_code)]
pub struct CustomChemistry {
    pub name: String,
    pub geometry: String,
    pub expected_ori: ExpectedOri,
}

#[allow(dead_code)]
impl CustomChemistry {
    pub fn geometry(&self) -> &str {
        self.geometry.as_str()
    }

    pub fn name(&self) -> &str {
        self.name.as_str()
    }

    pub fn expected_ori(&self) -> ExpectedOri {
        self.expected_ori.clone()
    }
}

/// This function gets the custom chemistry from the `af_home_path` directory.
/// If the file doesn't exist, it downloads the file from the `url` and saves it
pub fn get_custom_chem_hm(custom_chem_p: &Path) -> Result<HashMap<String, CustomChemistry>> {
    // check if the custom_chemistries.json file exists
    let custom_chem_exists = custom_chem_p.is_file();

    // get the file
    if custom_chem_exists {
        // test if the file is good
        let custom_chem_file = std::fs::File::open(custom_chem_p).with_context(|| {
            format!(
                "Couldn't open the existing custom chemistry file. Please consider delete it from {}",
                custom_chem_p.display()
            )
        })?;
        let custom_chem_reader = BufReader::new(custom_chem_file);
        let v: Value = serde_json::from_reader(custom_chem_reader).with_context(|| {
            format!(
                "Couldn't parse the existing custom chemistry file. Please consider delete it from {}",
                custom_chem_p.display()
            )
        })?;

        // we check if the file is up to date
        let fake_version = json!("0.0.1");
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

        // check if the permit_list_info.json file is up to date
        match prog_utils::check_version_constraints(
            "custom_chemistries.json",
            ">=".to_string() + CUSTOM_CHEMISTRIES_VERSION,
            version,
        ) {
            Ok(af_ver) => info!("found permit_list_info.json version {:#}; Proceeding", af_ver),
            Err(_) => warn!("found outdated permit list info file (with version {}) at {:#?}. Please consider delete it.", version, custom_chem_p)
        }
    } else {
        // download the custom_chemistries.json file if needed
        let custom_chem_url = CUSTOM_CHEMISTRIES_URL;
        prog_utils::download_to_file(custom_chem_url, custom_chem_p)?;
    }

    // load the file
    let custom_chem_file = std::fs::File::open(custom_chem_p)?;
    let custom_chem_reader = BufReader::new(custom_chem_file);
    let v: Value = serde_json::from_reader(custom_chem_reader)?;
    get_custom_chem_hm_from_value(v, custom_chem_p)
}

pub fn get_custom_chem_hm_from_value(
    v: Value,
    custom_chem_p: &Path,
) -> Result<HashMap<String, CustomChemistry>> {
    let v_obj = v.as_object().with_context(|| {
        format!(
            "Couldn't parse the existing custom chemistry file. Please consider delete it from {}",
            custom_chem_p.display()
        )
    })?;

    let expected_ori_key = String::from("expected_ori");
    // check if expected_ori exists
    let expected_oris = v_obj.get(&expected_ori_key);

    // warn if the expected_ori doesn't exist
    if expected_oris.is_none() {
        warn!("The expected_ori key is not found in the custom chemistry file, indicating it is an outdated version. All custom chemistries'  expected_ori will be treated as `both`. Please consider deleting the existing file from {}", custom_chem_p.display());
    }

    // Then we go over the keys and values and create a hashmap
    let mut custom_chem_map = HashMap::new();

    // Except the expected_ori key, others are custom chemistries
    for (key, value) in v_obj.iter() {
        // skip the expected_ori key
        if key == &expected_ori_key {
            continue;
        }

        // Now, we would expect we are working on a custom chemistry
        let chem_spec = value.as_str().with_context(|| {
            format!(
                "Couldn't parse chemistry {} : {} in the custom chemistry file. Please consider delete the file from {}",
                key,
                value,
                custom_chem_p.display()
            )
        })?;
        let _cg = extract_geometry(chem_spec).with_context(|| {
            format!(
                "Couldn't parse the geometry for {}: {}. Please consider delete the file from {}",
                key,
                chem_spec,
                custom_chem_p.display()
            )
        })?;

        // insert it into the custom_chem_map
        custom_chem_map.insert(key.clone(), CustomChemistry {
            name: key.clone(),
            geometry: chem_spec.to_string(),
            expected_ori: {
                // if expected_ori exists, we use it
                if let Some(expected_ori_value) = expected_oris {
                    let default_v = json!("both");
                    // get the expected_ori str
                    let expected_ori = expected_ori_value.get(key).unwrap_or(&default_v).as_str().with_context(|| {
                        format!(
                            "Couldn't parse the expected_ori for {}: {}. Please consider delete the file from {}",
                            key,
                            expected_ori_value.get(key).unwrap_or(&json!("both")),
                            custom_chem_p.display()
                        )
                    })?;
                    // convert it to expected_ori enum
                    ExpectedOri::from_str(expected_ori).with_context(|| {
                        format!(
                            "Couldn't parse the expected_ori for {}: {}. Please consider delete the file from {}",
                            key,
                            expected_ori,
                            custom_chem_p.display()
                        )
                    })?
                } else {
                    ExpectedOri::Both
                }
            }
        });
    }

    Ok(custom_chem_map)
}

pub fn custom_chem_hm_to_json(custom_chem_hm: &HashMap<String, CustomChemistry>) -> Result<Value> {
    // first create the name to genometry mapping
    let mut v: Value = custom_chem_hm
        .iter()
        .map(|(k, v)| {
            json!({
                k.clone() : v.geometry().to_string()
            })
        })
        .collect();

    // add in expected ori mapping
    let expected_ori_v: Value = custom_chem_hm
        .iter()
        .map(|(k, v)| {
            json!({
                k.clone() : v.expected_ori().as_str().to_string()
            })
        })
        .collect();

    // add the expected_ori to the geometry json
    v["expected_ori"] = expected_ori_v;

    Ok(v)
}
