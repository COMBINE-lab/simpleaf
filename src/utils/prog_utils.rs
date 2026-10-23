use anyhow::{Context, Result, anyhow, bail};
use cmd_lib::run_fun;
use semver::{Version, VersionReq};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::env;
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::LazyLock;
use tracing::{debug, error, info, warn};
use ureq::ResponseExt;
use which::which;

use file_requirements::{FileRequirementBuildError, FileRequirementBuilder};

/// The versions of the external programs this release of simpleaf requires.
///
/// These are the single source of truth. They used to be written out at each
/// call site, and drifted: `get_required_progs_from_paths` demanded piscem
/// `>=0.19.0` while every per-command check still said `>=0.18.0` and
/// `simpleaf_conda_env.yml` said `>=0.17.1` — three different answers to one
/// question, so raising the floor meant finding all of them.
///
/// Keep `simpleaf_conda_env.yml` in step when these change; it pins the same
/// programs for CI and cannot read these constants.
pub mod min_versions {
    /// piscem 0.23.0 is the first release built against piscem-rs 0.10
    /// (sshash-lib 0.7, the unified canonical minimizer scheme): its index
    /// format is incompatible with earlier releases in BOTH directions —
    /// 0.23 rejects old indexes with a rebuild message, and older piscem
    /// cannot read 0.23-built indexes. Requiring >=0.23 keeps the piscem
    /// simpleaf indexes with and the piscem it maps with format-consistent.
    /// (0.22.0 introduced `--decoder`/`--thread-policy` and the shared `-t`
    /// budget, which simpleaf forwards unconditionally.)
    pub const PISCEM: &str = ">=0.23.0, <1.0.0";

    /// alevin-fry 0.18.0 provides deterministic compiled barcode-correction
    /// plans and the correction/resource controls forwarded by simpleaf 0.28.
    /// 0.18.3 is required because collate is now invoked with `--compress=<codec>`
    /// (per-chunk codec, on by default): earlier releases spell `--compress` as a
    /// bare boolean flag and reject the `=<codec>` value, which would fail at
    /// collate on every run.
    pub const ALEVIN_FRY: &str = ">=0.18.3, <1.0.0";

    /// Only consulted when peak calling is actually requested.
    pub const MACS3: &str = ">=3.0.2, <4.0.0";
}

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

/// NOTE: the body of the JSON object we fetch cannot exceed 10MB
/// this is a limitation put in place by `ureq` (see
/// <https://docs.rs/ureq/3.0.0-rc4/ureq/struct.Body.html#method.read_json>).
pub fn read_json_from_remote_url<T: AsRef<str>>(url: T) -> Result<serde_json::Value> {
    let url = url.as_ref();

    let config = ureq::Agent::config_builder()
        .timeout_recv_response(Some(std::time::Duration::from_secs(120)))
        .max_redirects(10)
        .build();
    let agent = ureq::Agent::new_with_config(config);
    let response = agent.get(url).call();

    match response {
        Ok(mut response) => {
            if response.status().is_success() {
                return Ok(response.body_mut().read_json()?);
            } else {
                let c = response
                    .status()
                    .canonical_reason()
                    .unwrap_or("UNKNOWN FAILURE");
                warn!(
                    "Obtained status code {} from final url {}",
                    c,
                    response.get_uri()
                );
            }
        }
        Err(e) => {
            bail!("could not obtain content from {} because {:#?}", url, e);
        }
    }
    bail!("control should not reach here");
}

pub fn download_to_file_compute_hash<T: AsRef<str>>(
    url: T,
    file_path: &Path,
) -> Result<blake3::Hash> {
    let url = url.as_ref();
    download_to_file(url, file_path)
        .with_context(|| format!("failed to download the file from {}", url))?;
    let new_file = std::fs::File::open(file_path)?;
    let reader = std::io::BufReader::new(new_file);
    let mut hasher = blake3::Hasher::new();
    hasher.update_reader(reader)?;
    Ok(hasher.finalize())
}

pub fn download_to_file<T: AsRef<str>>(url: T, file_path: &Path) -> Result<()> {
    let url = url.as_ref();

    debug!(
        "Downloading file from {} and writing to file {}",
        url,
        file_path.display()
    );

    let config = ureq::Agent::config_builder()
        .timeout_recv_response(Some(std::time::Duration::from_secs(120)))
        .max_redirects(10)
        .build();
    let agent = ureq::Agent::new_with_config(config);
    let response = agent.get(url).call();

    match response {
        Ok(mut response) => {
            if response.status().is_success() {
                let mut req = response.body_mut().as_reader();
                let f = std::fs::File::create(file_path)
                    .with_context(|| format!("could not create file {}", file_path.display()))?;
                let mut ofile = std::io::BufWriter::new(f);
                std::io::copy(&mut req, &mut ofile)?;
            } else {
                // A non-success HTTP status (404, 403, rate-limit, 5xx, ...)
                // must be a hard error: otherwise this function returns Ok(())
                // without ever creating `file_path`, and callers that open it
                // next fail with a cryptic "No such file or directory".
                let status = response.status();
                let reason = status.canonical_reason().unwrap_or("UNKNOWN FAILURE");
                bail!(
                    "could not download {} — server returned status {} ({}) from final url {}",
                    url,
                    status.as_u16(),
                    reason,
                    response.get_uri()
                );
            }
        }
        Err(e) => {
            bail!("could not obtain content from {} because {:#?}", url, e);
        }
    }

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
    pub piscem: Option<ProgInfo>,
    pub alevin_fry: Option<ProgInfo>,
    pub macs: Option<ProgInfo>,
}

#[allow(dead_code)]
pub fn check_version_constraints<S1: AsRef<str>>(
    prog_name: &str,
    req_string: S1,
    prog_ver_string: &str,
) -> Result<Version> {
    // A tool that prints something that is not a semantic version is a
    // misconfigured install, not a bug in simpleaf; report it rather than
    // panicking with an unwrap.
    let parsed_version = Version::parse(prog_ver_string).with_context(|| {
        format!(
            "could not parse the version of {} (`{}`) as a semantic version",
            prog_name, prog_ver_string
        )
    })?;
    let req = VersionReq::parse(req_string.as_ref())
        .with_context(|| format!("invalid version requirement `{}`", req_string.as_ref()))?;
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

/// Preflight: confirm the alevin-fry recorded in the cached program info
/// (`simpleaf_info.json`) satisfies [`min_versions::ALEVIN_FRY`], erroring with
/// an actionable message *before* any long-running work.
///
/// `set-paths`/`refresh-prog-info` already validate at registration time, but a
/// user who registered an older alevin-fry and then upgraded simpleaf would
/// otherwise only discover the mismatch deep in `collate` (e.g. `--compress` was
/// added in alevin-fry 0.18.3). This checks the *cached* version string, so it
/// spawns no subprocess.
pub fn ensure_alevin_fry_version(rp: &ReqProgs) -> Result<()> {
    let af = rp
        .alevin_fry
        .as_ref()
        .context("alevin-fry program info is missing; please run `simpleaf set-paths`.")?;
    check_version_constraints("alevin-fry", min_versions::ALEVIN_FRY, &af.version).with_context(
        || {
            "the registered alevin-fry does not meet this simpleaf's minimum; install a newer \
             alevin-fry and re-run `simpleaf set-paths`"
        },
    )?;
    Ok(())
}

/// Optional alevin-fry capabilities that simpleaf uses only on a build that
/// has them.
///
/// The version string cannot tell builds apart here: development and
/// integration builds that carry these still report the last release (0.18.3).
/// So they are read off `--help` instead, which lists exactly what that build
/// accepts.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct AlevinFryCapabilities {
    /// The `quant-probe` subcommand: resolves UMIs over the probes of a
    /// probe-set reference and sums each gene's probes into its count. Without
    /// it every run is quantified per gene by `quant`, as before.
    pub quant_probe: bool,
    /// `quant --dump-molecules`: writes the molecule table.
    pub quant_dump_molecules: bool,
    /// `quant-probe --dump-molecules`. Held apart from the `quant` one because
    /// the check has to be about the subcommand the run will actually invoke.
    pub quant_probe_dump_molecules: bool,
    /// `quant-probe --probe-mtx`: also writes the probe x cell matrix.
    pub probe_mtx: bool,
}

impl AlevinFryCapabilities {
    /// Read the capabilities off the `--help` text of `quant` and of
    /// `quant-probe`, the latter `None` on a build that has no such
    /// subcommand.
    pub fn from_help(quant_help: &str, quant_probe_help: Option<&str>) -> Self {
        Self {
            quant_probe: quant_probe_help.is_some(),
            quant_dump_molecules: help_defines_flag(quant_help, "--dump-molecules"),
            quant_probe_dump_molecules: quant_probe_help
                .is_some_and(|help| help_defines_flag(help, "--dump-molecules")),
            probe_mtx: quant_probe_help.is_some_and(|help| help_defines_flag(help, "--probe-mtx")),
        }
    }

    /// Run `quant --help` and `quant-probe --help` once each on the given
    /// alevin-fry.
    ///
    /// `quant-probe` failing is how a build without that subcommand answers,
    /// so it is not reported. A `quant` whose help cannot be read is, and
    /// leaves the optional options off -- what simpleaf assumed before it
    /// checked.
    pub fn detect(alevin_fry: &Path) -> Self {
        let help = |subcommand: &str| -> Option<String> {
            match Command::new(alevin_fry)
                .arg(subcommand)
                .arg("--help")
                .output()
            {
                Ok(out) if out.status.success() => {
                    let mut text = String::from_utf8_lossy(&out.stdout).into_owned();
                    text.push_str(&String::from_utf8_lossy(&out.stderr));
                    Some(text)
                }
                Ok(_) => None,
                Err(e) => {
                    warn!(
                        "could not run `{} {} --help` ({}); assuming this alevin-fry does not have it",
                        alevin_fry.display(),
                        subcommand,
                        e
                    );
                    None
                }
            }
        };
        let quant_help = help("quant");
        if quant_help.is_none() {
            warn!(
                "`{} quant --help` could not be read; assuming it has none of the optional options",
                alevin_fry.display()
            );
        }
        let quant_probe_help = help("quant-probe");
        let caps = Self::from_help(
            quant_help.as_deref().unwrap_or(""),
            quant_probe_help.as_deref(),
        );
        debug!("alevin-fry capabilities: {:?}", caps);
        caps
    }
}

/// The error for a simpleaf option that is forwarded as alevin-fry's
/// `subcommand` option `flag` when the configured alevin-fry does not have it.
pub fn missing_alevin_fry_option(alevin_fry: &Path, subcommand: &str, flag: &str) -> anyhow::Error {
    anyhow!(
        "`{flag}` needs an alevin-fry whose `{subcommand}` has `{flag}`, but `{} {subcommand} \
         --help` does not list it. Install an alevin-fry that has it and re-run `simpleaf \
         set-paths`, or rerun without `{flag}`.",
        alevin_fry.display()
    )
}

/// Whether clap-style `help` text defines the long option `flag` (e.g.
/// `--probe-mtx`). Only option lines count -- `  -x, --flag ...` or
/// `      --flag ...` -- so a flag that is merely mentioned in another option's
/// description (whose wrapped lines clap indents further), or one that only
/// shares a prefix (`--flag-extra`), does not register.
fn help_defines_flag(help: &str, flag: &str) -> bool {
    help.lines().any(|line| {
        let trimmed = line.trim_start();
        if line.len() - trimmed.len() > 6 {
            return false;
        }
        let line = trimmed;
        // Drop a short alias such as `-d, `.
        let line = match line.split_once(", ") {
            Some((short, rest)) if short.len() == 2 && short.starts_with('-') && short != "--" => {
                rest
            }
            _ => line,
        };
        line.strip_prefix(flag).is_some_and(|rest| {
            rest.chars()
                .next()
                .is_none_or(|c| c.is_whitespace() || matches!(c, '<' | '=' | '['))
        })
    })
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
                let ver = if version.split(".").count() > 3 {
                    warn!(
                        "version info {} is not a valid semver (more than 3 dotted version parts; looking only at the major, minor & patch versions).",
                        version
                    );
                    version
                        .split(".")
                        .take(3)
                        .collect::<Vec<&str>>()
                        .join(".")
                        .to_string()
                } else {
                    version.to_string()
                };
                // See the note in `check_version_constraints`: a tool printing
                // something unparseable is a bad install, not a panic.
                let parsed_version = Version::parse(&ver).with_context(|| {
                    format!(
                        "could not parse the version of {} (`{}`) as a semantic version; \
                         the program reported: {:?}",
                        prog_name, ver, version
                    )
                })?;
                let req = VersionReq::parse(req_string.as_ref()).with_context(|| {
                    format!("invalid version requirement `{}`", req_string.as_ref())
                })?;
                if req.matches(&parsed_version) {
                    return Ok(parsed_version);
                } else {
                    return Err(anyhow!(
                        "Parsed version of {} ({:?}) does not satisfy constraints {}. Please install a compatible version.",
                        prog_name,
                        ver,
                        req
                    ));
                }
            }
        }
        Err(e) => {
            eprintln!("Error running {} {}", prog_name, e);
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
    piscem_exe: Option<PathBuf>,
    alevin_fry_exe: Option<PathBuf>,
    macs_exe: Option<PathBuf>,
) -> Result<ReqProgs> {
    let mut rp = ReqProgs {
        piscem: None,
        alevin_fry: None,
        macs: None,
    };

    // use the given path if we have it
    // otherwise, check `which`

    let opt_piscem = match piscem_exe {
        Some(p) => Some(p),
        None => get_which_executable("piscem").ok(),
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

    let opt_macs = match macs_exe {
        Some(p) => Some(p),
        None => match get_which_executable("macs3") {
            Ok(p) => Some(p),
            Err(_e) => {
                info!(
                    "Could not find macs3 executable, peak calling cannot be peformed by simpleaf"
                );
                None
            }
        },
    };

    if let Some(piscem) = opt_piscem {
        let st = piscem.display().to_string();
        let sr = run_fun!(${st} --version);
        let v = check_version_constraints_from_output("piscem", min_versions::PISCEM, sr)?;
        rp.piscem = Some(ProgInfo {
            exe_path: piscem,
            version: format!("{}", v),
        });
    }

    if let Some(macs) = opt_macs {
        let st = macs.display().to_string();
        let sr = run_fun!(${st} --version);
        let v = check_version_constraints_from_output("macs3", min_versions::MACS3, sr)?;
        rp.macs = Some(ProgInfo {
            exe_path: macs,
            version: format!("{}", v),
        });
    }

    let st = alevin_fry.display().to_string();
    let sr = run_fun!(${st} --version);
    let v = check_version_constraints_from_output("alevin-fry", min_versions::ALEVIN_FRY, sr)?;
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
    let piscem_exe = Some(search_for_executable("PISCEM", "piscem")?);
    let alevin_fry_exe = Some(search_for_executable("ALEVIN_FRY", "alevin-fry")?);
    let macs_exe = Some(search_for_executable("MACS3", "macs3")?);

    get_required_progs_from_paths(piscem_exe, alevin_fry_exe, macs_exe)
}

/// Check that all files in `file_vec` exist.
///
/// This maintains backwards compatibility for existing call sites by expressing
/// the check as a conjunction of file terms in the generic file-requirement
/// engine.
pub fn check_files_exist(file_vec: &[PathBuf]) -> Result<()> {
    let mut builder = FileRequirementBuilder::new();
    let mut seen = HashSet::new();
    for fb in file_vec {
        if !seen.insert(fb.clone()) {
            continue;
        }
        builder
            .require_file(fb)
            .map_err(map_file_requirement_build_err)?;
    }
    builder
        .build()
        .check()
        .map_err(|e| anyhow!("Required input files were missing; cannot proceed! {}", e))
}

/// Check that a piscem index base contains all required files for either supported
/// on-disk layout:
///
/// 1. C++ layout: includes `<base>.sshash`
/// 2. Rust layout: includes both `<base>.ssi` and `<base>.ssi.mphf`
///
/// In both cases the common files must exist (`.ctab` and `.refinfo`).
pub fn check_piscem_index_files(index_base: &Path) -> Result<()> {
    let mut builder = FileRequirementBuilder::new();
    builder
        .require_all(|all| {
            all.require_file(index_base.with_extension("ctab"))?;
            all.require_file(index_base.with_extension("refinfo"))?;
            Ok(())
        })
        .map_err(map_file_requirement_build_err)?;
    builder
        .require_any(|any| {
            any.require_file(index_base.with_extension("sshash"))?;
            any.require_all(|rust_all| {
                rust_all.require_file(index_base.with_extension("ssi"))?;
                rust_all.require_file(index_base.with_extension("ssi.mphf"))?;
                Ok(())
            })?;
            Ok(())
        })
        .map_err(map_file_requirement_build_err)?;

    builder.build().check().map_err(|e| {
        anyhow!(
            "Required piscem index files were missing or incomplete for base {}. {}",
            index_base.display(),
            e
        )
    })
}

fn map_file_requirement_build_err(err: FileRequirementBuildError) -> anyhow::Error {
    anyhow!("Invalid file requirement expression: {}", err)
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

#[cfg(test)]
mod tests {
    use super::{
        AlevinFryCapabilities, ProgInfo, ReqProgs, check_files_exist, check_piscem_index_files,
        ensure_alevin_fry_version, help_defines_flag,
    };
    use cmd_lib::run_fun;
    use std::fs;
    use tempfile::tempdir;

    /// Excerpts of real `--help` output. Every build here reports 0.18.3, so
    /// the version string cannot tell them apart.
    const QUANT_HELP_0_18_3: &str = "\
Options:
  -m, --tg-map <TGMAP>                  transcript to gene map
  -d, --dump-eqclasses                  flag for dumping equivalence classes
  -b, --num-bootstraps <NUMBOOTSTRAPS>  number of bootstraps to use [default: 0]
      --small-thresh <SMALLTHRESH>      cells with fewer than this many reads are resolved by a fast
  -h, --help                            Print help
";
    const QUANT_HELP_INTEGRATION: &str = "\
Options:
  -m, --tg-map <TGMAP>                  transcript to gene map
  -d, --dump-eqclasses                  flag for dumping equivalence classes
      --dump-molecules                  write alevin/molecules.parquet: one row per resolved
                                        molecule (cell barcode, representative UMI, feature,
  -b, --num-bootstraps <NUMBOOTSTRAPS>  number of bootstraps to use [default: 0]
  -h, --help                            Print help
";
    const QUANT_PROBE_HELP: &str = "\
Quantify a probe-based (e.g. 10x Flex) collated RAD file per probe, summing each gene's probes into its count

Options:
  -m, --tg-map <TGMAP>           transcript to gene map
  -r, --resolution <RESOLUTION>  the resolution strategy by which molecules will be counted
      --dump-molecules           write alevin/molecules.parquet: one row per resolved molecule
      --probe-mtx                also write the probe x cell count matrix the gene matrix was
                                 summed from; the gene matrix is written either way
  -h, --help                     Print help
";

    #[test]
    fn capabilities_are_read_off_the_help_text() {
        // 0.18.3 has no `quant-probe`, so `--help` for it does not run.
        assert_eq!(
            AlevinFryCapabilities::from_help(QUANT_HELP_0_18_3, None),
            AlevinFryCapabilities::default()
        );
        // The RAD v2 integration build writes the molecule table but still has
        // no `quant-probe`, so a probe-set run stays on the gene axis.
        assert_eq!(
            AlevinFryCapabilities::from_help(QUANT_HELP_INTEGRATION, None),
            AlevinFryCapabilities {
                quant_probe: false,
                quant_dump_molecules: true,
                ..AlevinFryCapabilities::default()
            }
        );
        // A build with `quant-probe`. Its options are read off *its* help: the
        // molecule table is listed by both subcommands here, `--probe-mtx` by
        // `quant-probe` alone.
        assert_eq!(
            AlevinFryCapabilities::from_help(QUANT_HELP_INTEGRATION, Some(QUANT_PROBE_HELP)),
            AlevinFryCapabilities {
                quant_probe: true,
                quant_dump_molecules: true,
                quant_probe_dump_molecules: true,
                probe_mtx: true,
            }
        );
        // `--probe-mtx` on `quant` does not count: it is passed to `quant-probe`.
        assert!(!AlevinFryCapabilities::from_help(QUANT_PROBE_HELP, None).probe_mtx);
        // No help text at all (the subcommand could not be run) means no
        // optional options, i.e. what simpleaf assumed before it checked.
        assert_eq!(
            AlevinFryCapabilities::from_help("", None),
            AlevinFryCapabilities::default()
        );
    }

    #[test]
    fn help_flag_detection_needs_an_option_line_for_that_exact_flag() {
        // With and without a short alias, with and without a value.
        assert!(help_defines_flag(
            QUANT_HELP_INTEGRATION,
            "--dump-eqclasses"
        ));
        assert!(help_defines_flag(QUANT_HELP_INTEGRATION, "--tg-map"));
        assert!(help_defines_flag(QUANT_PROBE_HELP, "--probe-mtx"));
        assert!(help_defines_flag("  --tg-map=<TGMAP>  x", "--tg-map"));
        // A prefix of a longer flag is not the flag.
        assert!(!help_defines_flag(QUANT_HELP_INTEGRATION, "--dump"));
        assert!(!help_defines_flag(
            "      --probe-mtx-dir <DIR>  x",
            "--probe-mtx"
        ));
        // A mention inside another option's description is not a definition.
        assert!(!help_defines_flag(
            "      --probe-mtx   only defined on the quant-probe subcommand\n\
             \x20                 --dump-molecules stays on quant",
            "--dump-molecules"
        ));
    }

    #[cfg(unix)]
    #[test]
    fn capabilities_detect_runs_each_subcommand_help_once() {
        use std::os::unix::fs::PermissionsExt;

        let write_fake = |path: &std::path::Path, body: &str| {
            fs::write(path, body).expect("failed to write fake alevin-fry");
            let mut perms = fs::metadata(path).expect("stat").permissions();
            perms.set_mode(0o755);
            fs::set_permissions(path, perms).expect("chmod");
        };

        let td = tempdir().expect("failed to create tempdir");
        let log = td.path().join("calls.log");
        let exe = td.path().join("alevin-fry");
        write_fake(
            &exe,
            &format!(
                "#!/bin/sh\necho \"$@\" >> '{}'\n\
                 case \"$1\" in\n\
                 quant) echo '      --dump-molecules  x' ;;\n\
                 quant-probe) echo '      --probe-mtx  x' ;;\n\
                 *) exit 2 ;;\n\
                 esac\n",
                log.display()
            ),
        );

        assert_eq!(
            AlevinFryCapabilities::detect(&exe),
            AlevinFryCapabilities {
                quant_probe: true,
                quant_dump_molecules: true,
                quant_probe_dump_molecules: false,
                probe_mtx: true,
            }
        );
        assert_eq!(
            fs::read_to_string(&log).expect("read log"),
            "quant --help\nquant-probe --help\n"
        );

        // A build without the subcommand: `quant-probe --help` exits non-zero,
        // which is its answer rather than a failure to report.
        let older = td.path().join("alevin-fry-0.18.3");
        write_fake(
            &older,
            "#!/bin/sh\ncase \"$1\" in\nquant) echo '  -m, --tg-map <TGMAP>  x' ;;\n*) exit 2 ;;\nesac\n",
        );
        assert_eq!(
            AlevinFryCapabilities::detect(&older),
            AlevinFryCapabilities::default()
        );

        // An executable that cannot be run reports no optional options.
        assert_eq!(
            AlevinFryCapabilities::detect(&td.path().join("missing")),
            AlevinFryCapabilities::default()
        );
    }

    fn req_progs_with_af(version: &str) -> ReqProgs {
        ReqProgs {
            piscem: None,
            alevin_fry: Some(ProgInfo {
                exe_path: "/bin/echo".into(),
                version: version.to_string(),
            }),
            macs: None,
        }
    }

    #[test]
    fn ensure_alevin_fry_version_accepts_current_and_rejects_old_or_missing() {
        // meets the >=0.18.3 floor
        assert!(ensure_alevin_fry_version(&req_progs_with_af("0.18.3")).is_ok());
        assert!(ensure_alevin_fry_version(&req_progs_with_af("0.20.0")).is_ok());
        // below the floor -> clear error before any work
        assert!(ensure_alevin_fry_version(&req_progs_with_af("0.18.2")).is_err());
        assert!(ensure_alevin_fry_version(&req_progs_with_af("0.18.0")).is_err());
        // not registered at all -> error pointing at set-paths
        let none = ReqProgs {
            piscem: None,
            alevin_fry: None,
            macs: None,
        };
        assert!(ensure_alevin_fry_version(&none).is_err());
    }

    #[test]
    fn check_files_exist_ignores_duplicate_entries() {
        let td = tempdir().expect("failed to create tempdir");
        let file = td.path().join("a.txt");
        fs::write(&file, "").expect("failed to write test file");
        let files = vec![file.clone(), file];
        check_files_exist(&files).expect("duplicate terms should not fail legacy file checks");
    }

    #[test]
    fn check_piscem_index_files_accepts_cpp_layout() {
        let td = tempdir().expect("failed to create tempdir");
        let base = td.path().join("piscem_idx");
        fs::write(base.with_extension("ctab"), "").expect("failed to write ctab");
        fs::write(base.with_extension("refinfo"), "").expect("failed to write refinfo");
        fs::write(base.with_extension("sshash"), "").expect("failed to write sshash");
        check_piscem_index_files(base.as_path()).expect("cpp index layout should be accepted");
    }

    #[test]
    fn check_piscem_index_files_accepts_rust_layout() {
        let td = tempdir().expect("failed to create tempdir");
        let base = td.path().join("piscem_idx");
        fs::write(base.with_extension("ctab"), "").expect("failed to write ctab");
        fs::write(base.with_extension("refinfo"), "").expect("failed to write refinfo");
        fs::write(base.with_extension("ssi"), "").expect("failed to write ssi");
        fs::write(base.with_extension("ssi.mphf"), "").expect("failed to write ssi.mphf");
        check_piscem_index_files(base.as_path()).expect("rust index layout should be accepted");
    }

    #[test]
    fn check_piscem_index_files_rejects_partial_layout() {
        let td = tempdir().expect("failed to create tempdir");
        let base = td.path().join("piscem_idx");
        fs::write(base.with_extension("ctab"), "").expect("failed to write ctab");
        fs::write(base.with_extension("refinfo"), "").expect("failed to write refinfo");
        fs::write(base.with_extension("ssi"), "").expect("failed to write ssi");
        let err =
            check_piscem_index_files(base.as_path()).expect_err("partial rust layout must fail");
        assert!(
            format!("{:#}", err).contains("Required piscem index files were missing or incomplete"),
            "unexpected error: {:#}",
            err
        );
    }

    #[test]
    fn run_fun_supports_interpolated_executable_paths_with_spaces() {
        let td = tempdir().expect("failed to create tempdir");
        let bin_dir = td.path().join("bin with spaces");
        fs::create_dir_all(&bin_dir).expect("failed to create bin dir");
        let script = bin_dir.join("fake-tool");
        fs::write(&script, "#!/bin/sh\necho fake-tool 0.1.0\n").expect("failed to write script");

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;

            let mut perms = fs::metadata(&script)
                .expect("failed to stat script")
                .permissions();
            perms.set_mode(0o755);
            fs::set_permissions(&script, perms).expect("failed to chmod script");
        }

        let st = script.display().to_string();
        let version = run_fun!(${st} --version).expect("failed to run fake tool");
        assert_eq!(version, "fake-tool 0.1.0");
    }
}
