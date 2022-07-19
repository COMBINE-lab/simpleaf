use anyhow::{anyhow, Result};
use cmd_lib::run_fun;
use semver::{Version, VersionReq};
use serde::{Deserialize, Serialize};
use std::env;
use std::path::PathBuf;
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
    pub alevin_fry: Option<ProgInfo>,
    pub pyroe: Option<ProgInfo>,
}

pub fn check_version_constraints<S1: AsRef<str>>(
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
                        "parsed version {:?} does not satisfy constraints {:?}",
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
        Err(e) => {
            return Err(anyhow!(
                "could not find `{}` in your path: {}",
                prog_name,
                e
            ));
        }
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
    alevin_fry_exe: Option<PathBuf>,
    pyroe_exe: Option<PathBuf>,
) -> Result<ReqProgs> {
    let mut rp = ReqProgs {
        salmon: None,
        alevin_fry: None,
        pyroe: None,
    };

    // use the given path if we have it
    // otherwise, check `which`
    let salmon = match salmon_exe {
        Some(p) => p,
        None => match get_which_executable("salmon") {
            Ok(p) => p,
            Err(e) => {
                return Err(e);
            }
        },
    };
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

    let st = salmon.display().to_string();
    let sr = run_fun!($st --version);
    let v = check_version_constraints(">=1.5.1, <2.0.0", sr)?;
    rp.salmon = Some(ProgInfo {
        exe_path: salmon,
        version: format!("{}", v),
    });

    let st = alevin_fry.display().to_string();
    let sr = run_fun!($st --version);
    let v = check_version_constraints(">=0.4.1, <1.0.0", sr)?;
    rp.alevin_fry = Some(ProgInfo {
        exe_path: alevin_fry,
        version: format!("{}", v),
    });

    let st = pyroe.display().to_string();
    let sr = run_fun!($st --version);
    let v = check_version_constraints(">=0.6.2, <1.0.0", sr)?;
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
    let alevin_fry_exe = Some(search_for_executable("ALEVIN_FRY", "alevin-fry")?);
    let pyroe_exe = Some(search_for_executable("PYROE", "pyroe")?);

    get_required_progs_from_paths(salmon_exe, alevin_fry_exe, pyroe_exe)
}
