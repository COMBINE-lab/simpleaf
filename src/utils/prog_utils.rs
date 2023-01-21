use anyhow::{anyhow, Result};
use cmd_lib::run_fun;
use semver::{Version, VersionReq};
use serde::{Deserialize, Serialize};
use std::env;
use std::path::PathBuf;
use tracing::{error, info};
use which::which;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ProgInfo {
    pub exe_path: PathBuf,
    pub version: String,
}

impl Default for ProgInfo {
    fn default() -> Self {
        Self {
            exe_path: PathBuf::from(""),
            version: String::from("0.0.0"),
        }
    }
}

// Holds the paths to the
// programs we'll need to run
// the tool.
#[derive(Debug, Serialize, Deserialize)]
pub struct ReqProgs {
    pub salmon: Option<ProgInfo>,
    pub piscem: Option<ProgInfo>,
    pub alevin_fry: Option<ProgInfo>,
    pub pyroe: Option<ProgInfo>,
}

pub fn check_version_constraints<S1: AsRef<str>>(
    prog_name: &str,
    req_string: S1,
    prog_ver_string: &str,
) -> Result<Version> {
    let parsed_version = Version::parse(prog_ver_string).unwrap();
    let req = VersionReq::parse(req_string.as_ref()).unwrap();
    if req.matches(&parsed_version) {
        Ok(parsed_version)
    } else {
        Err(anyhow!(
            "Parsed version of {} ({:?}) does not satisfy constraints {}. Please install a compatible version.",
            prog_name,
            prog_ver_string,
            req
        ))
    }
}

pub fn check_version_constraints_from_output<S1: AsRef<str>>(
    prog_name: &str,
    req_string: S1,
    prog_output: std::result::Result<String, std::io::Error>,
) -> Result<Version> {
    match prog_output {
        Ok(vs) => {
            let x = vs.split_whitespace();
            if let Some(version) = x.last() {
                let parsed_version = Version::parse(version).unwrap();
                let req = VersionReq::parse(req_string.as_ref()).unwrap();
                if req.matches(&parsed_version) {
                    return Ok(parsed_version);
                } else {
                    return Err(anyhow!(
                        "Parsed version of {} ({:?}) does not satisfy constraints {}. Please install a compatible version.",
                        prog_name,
                        version,
                        req
                    ));
                }
            }
        }
        Err(e) => {
            eprintln!("Error running salmon {}", e);
            return Err(anyhow!("could not parse program output"));
        }
    }
    Err(anyhow!("invalid version string"))
}

pub fn get_which_executable(prog_name: &str) -> Result<PathBuf> {
    match which(prog_name) {
        Ok(p) => {
            println!("found `{}` in the PATH at {}", prog_name, p.display());
            Ok(p)
        }
        Err(e) => Err(anyhow!(
            "could not find `{}` in your path: {}",
            prog_name,
            e
        )),
    }
}

#[allow(dead_code)]
pub fn search_for_executable(env_key: &str, prog_name: &str) -> Result<PathBuf> {
    match env::var(env_key) {
        Ok(p) => Ok(PathBuf::from(p)),
        Err(e) => {
            eprintln!("${} is unset {}, trying default path.", env_key, e);
            eprintln!(
                "If a satisfactory version is not found, consider setting the ${} variable.",
                env_key
            );
            get_which_executable(prog_name)
        }
    }
}

pub fn get_required_progs_from_paths(
    salmon_exe: Option<PathBuf>,
    piscem_exe: Option<PathBuf>,
    alevin_fry_exe: Option<PathBuf>,
    pyroe_exe: Option<PathBuf>,
) -> Result<ReqProgs> {
    let mut rp = ReqProgs {
        salmon: None,
        piscem: None,
        alevin_fry: None,
        pyroe: None,
    };

    // use the given path if we have it
    // otherwise, check `which`

    // first, check for salmon and piscem.
    // we can have both, but we *need* at least
    // one of the two.
    let opt_piscem = match piscem_exe {
        Some(p) => Some(p),
        None => match get_which_executable("piscem") {
            Ok(p) => Some(p),
            Err(_e) => {
                // now we *need* salmon
                info!("could not find piscem executable, so salmon will be required.");
                None
            }
        },
    };

    let opt_salmon = match salmon_exe {
        Some(p) => Some(p),
        None => {
            match get_which_executable("salmon") {
                Ok(p) => Some(p),
                Err(e) => match &opt_piscem {
                    None => {
                        return Err(e);
                    }
                    Some(_) => {
                        info!("could not find salmon executable, only piscem will be usable as a mapper.");
                        None
                    }
                },
            }
        }
    };

    // We should only get to this point if we have at least one of piscem and salmon, sanity
    // check this.
    assert!(opt_salmon.is_some() || opt_piscem.is_some());

    let alevin_fry = match alevin_fry_exe {
        Some(p) => p,
        None => match get_which_executable("alevin-fry") {
            Ok(p) => p,
            Err(e) => {
                return Err(e);
            }
        },
    };
    let pyroe = match pyroe_exe {
        Some(p) => p,
        None => match get_which_executable("pyroe") {
            Ok(p) => p,
            Err(e) => {
                return Err(e);
            }
        },
    };

    if let Some(piscem) = opt_piscem {
        let st = piscem.display().to_string();
        let sr = run_fun!($st --version);
        let v = check_version_constraints_from_output("piscem", ">=0.3.0, <1.0.0", sr)?;
        rp.piscem = Some(ProgInfo {
            exe_path: piscem,
            version: format!("{}", v),
        });
    }

    if let Some(salmon) = opt_salmon {
        let st = salmon.display().to_string();
        let sr = run_fun!($st --version);
        let v = check_version_constraints_from_output("salmon", ">=1.5.1, <2.0.0", sr)?;
        rp.salmon = Some(ProgInfo {
            exe_path: salmon,
            version: format!("{}", v),
        });
    }

    let st = alevin_fry.display().to_string();
    let sr = run_fun!($st --version);
    let v = check_version_constraints_from_output("alevin-fry", ">=0.4.1, <1.0.0", sr)?;
    rp.alevin_fry = Some(ProgInfo {
        exe_path: alevin_fry,
        version: format!("{}", v),
    });

    let st = pyroe.display().to_string();
    let sr = run_fun!($st --version);
    let v = check_version_constraints_from_output("pyroe", ">=0.6.2, <1.0.0", sr)?;
    rp.pyroe = Some(ProgInfo {
        exe_path: pyroe,
        version: format!("{}", v),
    });

    Ok(rp)
}

#[allow(dead_code)]
pub fn get_required_progs() -> Result<ReqProgs> {
    // First look for any environment variables
    // then check the path.
    let salmon_exe = Some(search_for_executable("SALMON", "salmon")?);
    let piscem_exe = Some(search_for_executable("PISCEM", "piscem")?);
    let alevin_fry_exe = Some(search_for_executable("ALEVIN_FRY", "alevin-fry")?);
    let pyroe_exe = Some(search_for_executable("PYROE", "pyroe")?);

    get_required_progs_from_paths(salmon_exe, piscem_exe, alevin_fry_exe, pyroe_exe)
}

pub fn check_files_exist(file_vec: &Vec<PathBuf>) -> Result<()> {
    let mut all_valid = true;
    for fb in file_vec {
        let er = fb.as_path().try_exists();
        match er {
            Ok(true) => {
                // do nothing
            }
            Ok(false) => {
                error!(
                    "Required input file at path {} was not found.",
                    fb.display()
                );
                all_valid = false;
            }
            Err(e) => {
                error!("{:#?}", e);
                all_valid = false;
            }
        }
    }

    if !all_valid {
        return Err(anyhow!(
            "Required input files were missing; cannot proceed!"
        ));
    }
    Ok(())
}
