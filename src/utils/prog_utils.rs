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

use file_requirements::{FileRequirement, FileRequirementBuildError, FileRequirementBuilder};

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

/// The path of the piscem index artifact `<prefix>.<ext>`, formed by appending
/// `.<ext>` to the whole prefix, the way piscem names index files.
///
/// Unlike [`Path::with_extension`], this keeps a dotted prefix intact:
/// `idx/gencode.v49` gives `idx/gencode.v49.ssi`, not `idx/gencode.ssi`. For a
/// prefix with no dot in its last component, such as simpleaf's own
/// `piscem_idx`, the two give the same path.
///
/// This is `Path::with_added_extension` (Rust 1.91), spelled out to keep the
/// 1.89 floor: a trailing separator is dropped, and a prefix that names no
/// file (empty, `/`, `.`, `..`) is returned unchanged.
pub fn index_artifact_path(prefix: &Path, ext: &str) -> PathBuf {
    let Some(name) = prefix.file_name() else {
        return prefix.to_path_buf();
    };
    let mut name = name.to_os_string();
    name.push(".");
    name.push(ext);
    prefix.with_file_name(name)
}

/// Extensions of the core files of a piscem index: the files piscem-rs's
/// `build` writes, plus the C++ `.sshash` that [`piscem_index_requirement`]
/// still accepts. piscem built on piscem-rs 0.11.0 or earlier named these by
/// replacing the prefix's last suffix; the poison table and signatures were
/// always appended.
const PISCEM_CORE_EXTS: [&str; 8] = [
    "ssi", "ssi.mphf", "ctab", "refinfo", "ectab", "tdct", "tct", "sshash",
];

/// Extensions that, ending a prefix, name one of the index's own files
/// (`-i idx/piscem_idx.ssi` for the index at `idx/piscem_idx`): piscem-rs's
/// list, plus the C++ `.sshash`.
const PISCEM_FILE_NAME_EXTS: [&str; 8] = [
    "ssi", "ctab", "refinfo", "ectab", "tdct", "tct", "poison", "sshash",
];

/// The required piscem index files, for either supported on-disk layout:
///
/// 1. C++ layout: includes `<base>.sshash`
/// 2. Rust layout: includes both `<base>.ssi` and `<base>.ssi.mphf`
///
/// In both cases the common files must exist (`.ctab` and `.refinfo`).
/// `artifact` maps an extension to the file that carries it.
fn piscem_index_requirement(artifact: impl Fn(&str) -> PathBuf) -> Result<FileRequirement> {
    let mut builder = FileRequirementBuilder::new();
    builder
        .require_all(|all| {
            all.require_file(artifact("ctab"))?;
            all.require_file(artifact("refinfo"))?;
            Ok(())
        })
        .map_err(map_file_requirement_build_err)?;
    builder
        .require_any(|any| {
            any.require_file(artifact("sshash"))?;
            any.require_all(|rust_all| {
                rust_all.require_file(artifact("ssi"))?;
                rust_all.require_file(artifact("ssi.mphf"))?;
                Ok(())
            })?;
            Ok(())
        })
        .map_err(map_file_requirement_build_err)?;
    Ok(builder.build())
}

/// Check that a piscem index base contains all required files (see
/// [`piscem_index_requirement`]) at `<base>.<ext>` ([`index_artifact_path`]).
///
/// For a prefix with an extension, the files at the replaced-suffix paths
/// (`idx/gencode.v49` -> `idx/gencode.ssi`, what `with_extension` gives) are
/// accepted instead in two cases, following the rule piscem-rs 0.11.1 uses to
/// resolve a prefix:
///
/// - the prefix names one of the index's files (`idx/piscem_idx.ssi`), or
/// - the index was built by piscem on piscem-rs 0.11.0 or earlier, which named
///   its core files that way. This is used only when no core file exists at
///   the appended paths, so the two layouts are never mixed, and it logs a
///   warning, since prefixes that differ only after the last dot share those
///   names.
///
/// A dotted prefix with complete files under both rules is accepted at the
/// appended paths, with a warning: piscem on piscem-rs 0.11.0 or earlier reads
/// the other set, and one of the two may belong to another index.
///
/// For a prefix without an extension both rules name the same files, so the
/// files checked, the outcome and the error are those `with_extension` gave.
pub fn check_piscem_index_files(index_base: &Path) -> Result<()> {
    if let Some(warning) = piscem_index_files_warning(index_base)? {
        warn!("{warning}");
    }
    Ok(())
}

/// [`check_piscem_index_files`], returning the warning about the accepted
/// files (if any) instead of logging it.
fn piscem_index_files_warning(index_base: &Path) -> Result<Option<String>> {
    let missing = |e: &dyn std::fmt::Display, note: &str| {
        anyhow!(
            "Required piscem index files were missing or incomplete for base {}. {}{}",
            index_base.display(),
            e,
            note
        )
    };
    let appended = |ext: &str| index_artifact_path(index_base, ext);
    let appended_check = piscem_index_requirement(appended)?.check();
    if index_base.extension().is_none() {
        return appended_check.map(|()| None).map_err(|e| missing(&e, ""));
    }

    let replaced = |ext: &str| index_base.with_extension(ext);
    let replaced_check = piscem_index_requirement(replaced)?.check();
    let (new_ssi, old_ssi) = (appended("ssi"), replaced("ssi"));
    let e = match appended_check {
        Ok(()) if replaced_check.is_ok() => {
            return Ok(Some(format!(
                "Found complete piscem index files for prefix {} under both naming rules: {}, \
                 ... (appended to the whole prefix) and {}, ... (its last suffix replaced). \
                 Checked the first set, which piscem built on piscem-rs 0.11.1 or later loads; \
                 piscem built on piscem-rs 0.11.0 or earlier loads the second. One set may \
                 belong to another index in this directory: piscem built on piscem-rs 0.11.0 \
                 or earlier also writes the first set for an index at {}.<suffix>. Make sure \
                 the piscem you run loads the index you intend.",
                index_base.display(),
                new_ssi.display(),
                old_ssi.display(),
                index_base.display()
            )));
        }
        Ok(()) => return Ok(None),
        Err(e) => e,
    };

    let names_index_file = index_base
        .extension()
        .and_then(OsStr::to_str)
        .is_some_and(|ext| PISCEM_FILE_NAME_EXTS.contains(&ext));
    let any_core_file = |artifact: &dyn Fn(&str) -> PathBuf| {
        PISCEM_CORE_EXTS.iter().any(|ext| artifact(ext).exists())
    };
    match replaced_check {
        Ok(()) if names_index_file => Ok(None),
        Ok(()) if !any_core_file(&appended) => Ok(Some(format!(
            "Using the piscem index for prefix {} from files named by replacing the prefix's \
             last suffix ({} rather than {}), the layout written by piscem built on piscem-rs \
             0.11.0 or earlier. In that layout, indexes built in the same directory at prefixes \
             that differ only after the last dot overwrite each other's files, so make sure \
             these files belong to this index. piscem built on piscem-rs 0.11.1 or later \
             appends to the whole prefix instead; rebuilding the index with it gives the index \
             its own file names.",
            index_base.display(),
            old_ssi.display(),
            new_ssi.display()
        ))),
        Err(old_e) if !any_core_file(&appended) && any_core_file(&replaced) => Err(anyhow!(
            "Required piscem index files were missing or incomplete for base {}, under the \
             names formed by replacing its last suffix (the layout written by piscem built on \
             piscem-rs 0.11.0 or earlier), since none of its core files exist under the \
             appended names (e.g. {}). {}",
            index_base.display(),
            new_ssi.display(),
            old_e
        )),
        _ => Err(missing(
            &e,
            &format!(
                " piscem built on piscem-rs 0.11.0 or earlier names an index's core files by \
                 replacing the prefix's last suffix instead (e.g. {}); those names are used \
                 only when none of the core files exist under the appended names (e.g. {}).",
                old_ssi.display(),
                new_ssi.display()
            ),
        )),
    }
}

// Kept apart from `mod tests` at the end of the file so that these tests sit
// next to the code they cover, and so that edits to the two modules do not
// conflict.
#[cfg(test)]
mod index_artifact_path_tests {
    use super::{
        check_piscem_index_files, index_artifact_path, piscem_index_files_warning,
        piscem_index_requirement,
    };
    use std::fs;
    use std::path::{Path, PathBuf};
    use tempfile::tempdir;

    const RUST_LAYOUT: [&str; 4] = ["ctab", "refinfo", "ssi", "ssi.mphf"];
    const CPP_LAYOUT: [&str; 3] = ["ctab", "refinfo", "sshash"];

    fn touch_all(paths: impl IntoIterator<Item = PathBuf>) {
        for p in paths {
            fs::write(&p, "").unwrap_or_else(|e| panic!("failed to write {}: {e}", p.display()));
        }
    }

    fn shown(p: &Path) -> String {
        p.display().to_string()
    }

    #[test]
    fn appends_to_the_whole_prefix() {
        let p = Path::new("idx/gencode.v49");
        assert_eq!(
            index_artifact_path(p, "ssi"),
            Path::new("idx/gencode.v49.ssi")
        );
        assert_eq!(
            index_artifact_path(p, "ssi.mphf"),
            Path::new("idx/gencode.v49.ssi.mphf")
        );
        assert_eq!(
            index_artifact_path(p, "poison.json"),
            Path::new("idx/gencode.v49.poison.json")
        );
    }

    // Undotted prefixes (simpleaf's own `piscem_idx`, the probe cache's `index`)
    // must keep resolving to exactly the paths `with_extension` gave, and so
    // must prefixes that name no file at all.
    #[test]
    fn matches_with_extension_for_undotted_prefixes() {
        for prefix in [
            "piscem_idx",
            "out/index/piscem_idx",
            "/abs/my.dir/piscem_idx",
            "./index",
            "out/piscem_idx/",
            ".hidden",
            "",
            ".",
            "..",
            "/",
        ] {
            let prefix = Path::new(prefix);
            for ext in [
                "ctab",
                "refinfo",
                "sshash",
                "ssi",
                "ssi.mphf",
                "poison.json",
            ] {
                assert_eq!(
                    index_artifact_path(prefix, ext),
                    prefix.with_extension(ext),
                    "prefix {} ext {ext}",
                    prefix.display()
                );
            }
        }
    }

    #[test]
    fn appended_layout_is_accepted_without_warning() {
        for name in ["gencode.v49", "piscem_idx"] {
            let td = tempdir().expect("failed to create tempdir");
            let base = td.path().join(name);
            touch_all(RUST_LAYOUT.map(|ext| index_artifact_path(&base, ext)));
            let warning =
                piscem_index_files_warning(&base).unwrap_or_else(|e| panic!("{name}: {e:#}"));
            assert_eq!(warning, None, "{name}");
        }
    }

    #[test]
    fn dotted_prefix_accepts_legacy_replaced_layout_with_warning() {
        let td = tempdir().expect("failed to create tempdir");
        let base = td.path().join("gencode.v49");
        touch_all(RUST_LAYOUT.map(|ext| base.with_extension(ext)));
        // Older piscem already appended these, so a legacy index has them here.
        touch_all([index_artifact_path(&base, "sigs.json")]);
        let warning = piscem_index_files_warning(&base)
            .expect("legacy replaced layout should be accepted")
            .expect("the legacy layout should be warned about");
        for part in [
            shown(&base.with_extension("ssi")),
            "written by piscem built on piscem-rs 0.11.0 or earlier".to_string(),
            "piscem built on piscem-rs 0.11.1 or later".to_string(),
        ] {
            assert!(warning.contains(&part), "missing {part:?} in: {warning}");
        }
        check_piscem_index_files(&base).expect("legacy replaced layout should be accepted");
    }

    // `-i idx/piscem_idx.ssi` names a file of the index at `idx/piscem_idx`;
    // `with_extension` accepted that, and piscem resolves it the same way. The
    // stray file under the appended names rules out the legacy fallback, so
    // only the names-a-file rule can accept these.
    #[test]
    fn prefix_naming_an_index_file_is_accepted() {
        for (layout, named) in [(&RUST_LAYOUT[..], "ssi"), (&CPP_LAYOUT[..], "sshash")] {
            let td = tempdir().expect("failed to create tempdir");
            let base = td.path().join("piscem_idx");
            let prefix = base.with_extension(named);
            touch_all(layout.iter().map(|ext| index_artifact_path(&base, ext)));
            touch_all([index_artifact_path(&prefix, "ctab")]);
            let warning = piscem_index_files_warning(&prefix)
                .unwrap_or_else(|e| panic!("{}: {e:#}", prefix.display()));
            assert_eq!(warning, None, "{}", prefix.display());
        }
    }

    #[test]
    fn dotted_prefix_does_not_mix_layouts() {
        let td = tempdir().expect("failed to create tempdir");
        let base = td.path().join("gencode.v49");
        touch_all(RUST_LAYOUT.map(|ext| base.with_extension(ext)));
        // A partial index at the appended names (e.g. an interrupted rebuild)
        // takes precedence over the complete legacy one.
        touch_all([index_artifact_path(&base, "ctab")]);
        let err = check_piscem_index_files(&base).expect_err("mixed layouts must fail");
        let msg = format!("{err:#}");
        assert!(
            msg.contains("gencode.v49.ssi.mphf"),
            "unexpected error: {msg}"
        );
        assert!(
            msg.contains("piscem built on piscem-rs 0.11.0 or earlier"),
            "unexpected error: {msg}"
        );
    }

    #[test]
    fn dotted_prefix_missing_reports_both_layouts() {
        let td = tempdir().expect("failed to create tempdir");
        let base = td.path().join("gencode.v49");
        let err = check_piscem_index_files(&base).expect_err("missing index must fail");
        let msg = format!("{err:#}");
        assert!(
            msg.contains("Required piscem index files were missing or incomplete"),
            "unexpected error: {msg}"
        );
        assert!(
            msg.contains("gencode.v49.refinfo"),
            "unexpected error: {msg}"
        );
        assert!(msg.contains("gencode.ssi"), "unexpected error: {msg}");
    }

    // An incomplete index from piscem on piscem-rs <= 0.11.0 is reported by the
    // files missing under its own names, as the check did before.
    #[test]
    fn dotted_prefix_incomplete_legacy_reports_legacy_files() {
        let td = tempdir().expect("failed to create tempdir");
        let base = td.path().join("gencode.v49");
        touch_all(["ctab", "refinfo", "ssi"].map(|ext| base.with_extension(ext)));
        let msg = format!("{:#}", check_piscem_index_files(&base).unwrap_err());
        let legacy_err = piscem_index_requirement(|ext| base.with_extension(ext))
            .expect("requirement should build")
            .check()
            .expect_err("legacy layout is incomplete");
        assert!(
            msg.ends_with(&legacy_err.to_string()),
            "unexpected error: {msg}"
        );
        assert!(
            msg.contains(&shown(&base.with_extension("ssi.mphf"))),
            "unexpected error: {msg}"
        );
        assert!(
            !msg.contains(&shown(&index_artifact_path(&base, "refinfo"))),
            "unexpected error: {msg}"
        );
    }

    // Released piscem built `human.splici` (files `human.*`) and
    // `human.splici.k23` (files `human.splici.*`). For `human.splici` the
    // second index sits under the appended names; it is accepted there, as
    // piscem-rs 0.11.1 resolves it, but not silently.
    #[test]
    fn dotted_prefix_with_both_layouts_warns() {
        let td = tempdir().expect("failed to create tempdir");
        let base = td.path().join("human.splici");
        touch_all(RUST_LAYOUT.map(|ext| base.with_extension(ext)));
        let nested = td.path().join("human.splici.k23");
        touch_all(RUST_LAYOUT.map(|ext| nested.with_extension(ext)));
        let warning = piscem_index_files_warning(&base)
            .expect("the appended layout should be accepted")
            .expect("finding both layouts should be warned about");
        for part in [
            "under both naming rules".to_string(),
            shown(&index_artifact_path(&base, "ssi")),
            shown(&base.with_extension("ssi")),
        ] {
            assert!(warning.contains(&part), "missing {part:?} in: {warning}");
        }
    }

    #[test]
    fn undotted_prefix_error_is_unchanged() {
        let td = tempdir().expect("failed to create tempdir");
        let base = td.path().join("piscem_idx");
        touch_all(["ctab", "refinfo", "ssi"].map(|ext| base.with_extension(ext)));
        let msg = format!("{:#}", check_piscem_index_files(&base).unwrap_err());
        let expected_prefix = format!(
            "Required piscem index files were missing or incomplete for base {}. \
             Required input files were missing or incomplete (missing files: {}",
            base.display(),
            base.with_extension("sshash").display()
        );
        assert!(msg.starts_with(&expected_prefix), "unexpected error: {msg}");
        assert!(!msg.contains("piscem-rs"), "unexpected error: {msg}");
    }
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
        ProgInfo, ReqProgs, check_files_exist, check_piscem_index_files, ensure_alevin_fry_version,
    };
    use cmd_lib::run_fun;
    use std::fs;
    use tempfile::tempdir;

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
