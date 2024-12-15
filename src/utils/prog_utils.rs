use anyhow::{anyhow, bail, Context, Result};
use cmd_lib::run_fun;
use semver::{Version, VersionReq};
use serde::{Deserialize, Serialize};
use std::env;
use std::ffi::{OsStr, OsString};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::LazyLock;
use tracing::{debug, error, info, warn};
use which::which;

// The below functions are taken from the [`execute`](https://crates.io/crates/execute)
// crate.

/// Create a `Command` instance which can be executed by the current command language interpreter (shell).
#[cfg(unix)]
#[inline]
pub fn shell<S: AsRef<OsStr>>(cmd: S) -> Command {
    static SHELL: LazyLock<OsString> = LazyLock::new(|| {
        env::var_os("SHELL").unwrap_or_else(|| OsString::from(String::from("sh")))
    });
    let shell = &*SHELL;
    let mut command = Command::new(shell);

    command.arg("-c");
    command.arg(cmd);

    command
}

/// Create a `Command` instance which can be executed by the current command language interpreter (shell).
#[cfg(windows)]
#[inline]
pub fn shell<S: AsRef<OsStr>>(cmd: S) -> Command {
    let mut command = Command::new("cmd.exe");

    command.arg("/c");
    command.arg(cmd);

    command
}

pub fn download_to_file<T: AsRef<str>>(url: T, file_path: &Path) -> Result<()> {
    let url = url.as_ref();

    debug!(
        "Downloading file from {} and writing to file {}",
        url,
        file_path.display()
    );

    let request = minreq::get(url).with_timeout(120).send()?;
    match request.status_code {
        200..=299 => {
            // success
            debug!(
                "Obtained status code {} from final url {}",
                request.status_code, request.url
            );
        }
        x => {
            bail!(
                "could not obtain the permit list; HTTP status code {}, reason {}",
                x,
                request.reason_phrase
            );
        }
    }

    let mut out_file = std::fs::File::create(file_path)?;
    out_file.write_all(request.as_bytes())?;
    Ok(())
}

pub fn get_cmd_line_string(prog: &std::process::Command) -> String {
    let mut prog_vec = vec![prog.get_program().to_string_lossy().to_string()];
    prog_vec.extend(
        prog.get_args()
            .map(|x| x.to_string_lossy().to_string())
            .collect::<Vec<String>>(),
    );
    prog_vec.join(" ")
}

pub enum CommandVerbosityLevel {
    #[allow(dead_code)]
    Verbose,
    Quiet,
}

pub fn execute_command(
    cmd: &mut std::process::Command,
    verbosity_level: CommandVerbosityLevel,
) -> Result<std::process::Output, std::io::Error> {
    match cmd.output() {
        Ok(output) if output.status.success() => {
            info!("command returned successfully ({})", output.status);
            match verbosity_level {
                CommandVerbosityLevel::Verbose => {
                    if !&output.stdout.is_empty() {
                        info!(
                            "stdout :\n====\n{}====",
                            String::from_utf8_lossy(&output.stdout)
                        );
                    }
                    if !&output.stderr.is_empty() {
                        info!(
                            "stderr :\n====\n{}====",
                            String::from_utf8_lossy(&output.stderr)
                        );
                    }
                }
                CommandVerbosityLevel::Quiet => {}
            }
            Ok(output)
        }
        Ok(output) => {
            error!("command unsuccessful ({}): {:?}", output.status, cmd);
            if !&output.stdout.is_empty() {
                error!(
                    "stdout :\n====\n{}====",
                    String::from_utf8_lossy(&output.stdout)
                );
            }
            if !&output.stderr.is_empty() {
                error!(
                    "stderr :\n====\n{}====",
                    String::from_utf8_lossy(&output.stderr)
                );
            }
            Ok(output)
        }
        Err(e) => {
            error!("command unsuccessful; error : {}", e);
            Err(e)
        }
    }
}

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
}

impl ReqProgs {
    pub fn issue_recommended_version_messages(&self) {
        // Currently (11/29/2024) want to recommend piscem >= 0.11.0
        if let Some(ref piscem_info) = self.piscem {
            let desired_ver = VersionReq::parse(">=0.11.0").unwrap();
            let current_ver = Version::parse(&piscem_info.version).unwrap();
            if desired_ver.matches(&current_ver) {
                // nothing to do here
            } else {
                warn!("It is recommended to use piscem version {}, but currently version {} is being used. \
                       Please consider installing the latest version of piscem and setting simpleaf to use this \
                       new version by running the `refresh-prog-info` command.", &desired_ver, &current_ver);
            }
        }
    }
}

#[allow(dead_code)]
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

/// Checks that the version returned from a given program's `--version`
/// flag is compatible with the provided `req_string`.  The interpretation
/// of compatible is according to the standard meaning of Semantic versioning.
/// This returns either `Ok(Version)` of the parsed, compatible, version or
/// an `anyhow::Error` describing the incompatibility of the version is not
/// compatible.
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
) -> Result<ReqProgs> {
    let mut rp = ReqProgs {
        salmon: None,
        piscem: None,
        alevin_fry: None,
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
    assert!(
        opt_salmon.is_some() || opt_piscem.is_some(),
        "At least one of piscem and salmon must be available."
    );

    let alevin_fry = match alevin_fry_exe {
        Some(p) => p,
        None => match get_which_executable("alevin-fry") {
            Ok(p) => p,
            Err(e) => {
                return Err(e);
            }
        },
    };

    if let Some(piscem) = opt_piscem {
        let st = piscem.display().to_string();
        let sr = run_fun!($st --version);
        let v = check_version_constraints_from_output("piscem", ">=0.5.1, <1.0.0", sr)?;
        rp.piscem = Some(ProgInfo {
            exe_path: piscem,
            version: format!("{}", v),
        });
    }

    if let Some(salmon) = opt_salmon {
        let st = salmon.display().to_string();
        let sr = run_fun!($st --version);
        let v = check_version_constraints_from_output("salmon", ">=1.10.0, <2.0.0", sr)?;
        rp.salmon = Some(ProgInfo {
            exe_path: salmon,
            version: format!("{}", v),
        });
    }

    let st = alevin_fry.display().to_string();
    let sr = run_fun!($st --version);
    let v = check_version_constraints_from_output("alevin-fry", ">=0.8.1, <1.0.0", sr)?;
    rp.alevin_fry = Some(ProgInfo {
        exe_path: alevin_fry,
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

    get_required_progs_from_paths(salmon_exe, piscem_exe, alevin_fry_exe)
}

pub fn check_files_exist(file_vec: &[PathBuf]) -> Result<()> {
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

pub fn read_json(json_path: &Path) -> anyhow::Result<serde_json::Value> {
    let json_file = std::fs::File::open(json_path)
        .with_context(|| format!("Could not open JSON file {}.", json_path.display()))?;
    let v: serde_json::Value = serde_json::from_reader(json_file)?;
    Ok(v)
}

pub fn inspect_af_home(af_home_path: &Path) -> anyhow::Result<serde_json::Value> {
    // Open the file in read-only mode with buffer.
    let af_info_p = af_home_path.join("simpleaf_info.json");

    // try read af info
    let v = read_json(af_info_p.as_path());

    // handle the error
    match v {
        Ok(okv) => Ok(okv),
        Err(e) => Err(anyhow!(
            "{} Please run the `simpleaf set-paths` command before using `index` or `quant`.",
            e
        )),
    }
}
