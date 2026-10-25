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

/// Downloads `url` to `file_path`. The file appears under `file_path` only once
/// the download is complete (see [`write_to_file_atomically`]).
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
                write_to_file_atomically(&mut req, file_path).with_context(|| {
                    format!("could not save {} to {}", url, file_path.display())
                })?;
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

/// Writes everything `reader` yields to `file_path`, such that `file_path`
/// never holds partial content.
///
/// The bytes go to a uniquely named temporary file in the directory of
/// `file_path` (`.<file name>.<random>.part`), which is renamed over
/// `file_path` only once the copy and the final flush have succeeded. On an
/// error the temporary file is removed and `file_path` is left as it was.
/// Writing in place instead leaves a truncated file under the final name when
/// a download is cut short, and the callers of [`download_to_file`] treat an
/// existing file as a complete, cached download. The unique name also keeps
/// concurrent runs that share an `ALEVIN_FRY_HOME` from writing into one file.
fn write_to_file_atomically<R: std::io::Read + ?Sized>(
    reader: &mut R,
    file_path: &Path,
) -> Result<()> {
    let dir = match file_path.parent() {
        Some(d) if !d.as_os_str().is_empty() => d,
        _ => Path::new("."),
    };
    let file_name = file_path
        .file_name()
        .map_or_else(|| "download".into(), |n| n.to_string_lossy());
    let prefix = format!(".{}.", file_name);

    let mut builder = tempfile::Builder::new();
    builder.prefix(&prefix).suffix(".part");
    // Temporary files are created owner-only (0o600); ask for the mode that
    // `File::create` used (0o666, less the umask) so the renamed file is as
    // readable as before, e.g. in an `ALEVIN_FRY_HOME` shared by a group.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        builder.permissions(std::fs::Permissions::from_mode(0o666));
    }
    let tmp = builder
        .tempfile_in(dir)
        .with_context(|| format!("could not create a temporary file in {}", dir.display()))?;

    let mut ofile = std::io::BufWriter::new(tmp);
    std::io::copy(reader, &mut ofile)?;
    // Flush explicitly: dropping a `BufWriter` discards the error of its final
    // flush (e.g. a full disk), which would let a truncated file through.
    let tmp = ofile.into_inner().map_err(|e| e.into_error())?;
    tmp.persist(file_path)
        .map_err(|e| e.error)
        .with_context(|| {
            format!(
                "could not move the temporary file into place at {}",
                file_path.display()
            )
        })?;
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

/// [`VersionReq::matches`], except that a pre-release is also accepted when
/// its release version satisfies `req`.
///
/// Plain semver only lets a pre-release match a comparator that carries a
/// pre-release of the same major.minor.patch, so `0.24.0-rc.1` fails
/// `>=0.24.0` (and `0.25.0-dev` fails even `>=0.24.0-0`), and rc/dev builds of
/// the tools cannot pass the floors in [`min_versions`]. Here each
/// comparator is checked against the release version (`0.24.0`) instead,
/// except one that names a pre-release of that same release (`>=0.24.0-rc.2`),
/// which still compares the full version. So versions below the floor stay
/// rejected (`0.23.1-rc.1` vs `>=0.24.0`), and so does a pre-release of an
/// excluded upper bound (`1.0.0-rc.1` vs `<1.0.0`). Note that `X.Y.Z-dev` is
/// accepted by a floor of exactly `X.Y.Z`, although semver orders it below.
fn matches_allowing_prerelease(req: &VersionReq, version: &Version) -> bool {
    if req.matches(version) {
        return true;
    }
    if version.pre.is_empty() {
        return false;
    }
    let release = Version::new(version.major, version.minor, version.patch);
    req.comparators.iter().all(|c| {
        let names_same_release = !c.pre.is_empty()
            && c.major == version.major
            && c.minor == Some(version.minor)
            && c.patch == Some(version.patch);
        c.matches(if names_same_release {
            version
        } else {
            &release
        })
    })
}

/// Checks that `prog_ver_string`, the version of `prog_name`, satisfies
/// `req_string` by semantic versioning, also accepting a pre-release whose
/// release version satisfies it (see [`matches_allowing_prerelease`]).
/// Returns the parsed version, or an error saying why it is not compatible.
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
    if matches_allowing_prerelease(&req, &parsed_version) {
        if !parsed_version.pre.is_empty() {
            info!(
                "using pre-release {} {} (required: {}); pre-releases are meant for testing and may lack features of the {}.{}.{} release.",
                prog_name,
                parsed_version,
                req,
                parsed_version.major,
                parsed_version.minor,
                parsed_version.patch
            );
        }
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
/// flag is compatible with the provided `req_string`.  The version is taken
/// to be the last whitespace-separated word of the output, and the check
/// itself is [`check_version_constraints`].
/// This returns either `Ok(Version)` of the parsed, compatible, version or
/// an `anyhow::Error` describing the incompatibility of the version is not
/// compatible.
pub fn check_version_constraints_from_output<S1: AsRef<str>>(
    prog_name: &str,
    req_string: S1,
    prog_output: std::result::Result<String, std::io::Error>,
) -> Result<Version> {
    let vs = prog_output.with_context(|| format!("could not run `{} --version`", prog_name))?;
    let version = vs
        .split_whitespace()
        .last()
        .with_context(|| format!("`{} --version` did not report a version", prog_name))?;
    // Only fall back to major.minor.patch for words that are not semver
    // (macs "2.2.9.1", "3.0.3.dev0"): a valid pre-release or build tag can
    // contain dots too ("0.24.0-rc.1"), and cutting it would change the version.
    let ver = if Version::parse(version).is_err() && version.split(".").count() > 3 {
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
    check_version_constraints(prog_name, req_string, &ver)
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
        ProgInfo, ReqProgs, check_files_exist, check_piscem_index_files, ensure_alevin_fry_version,
    };
    use cmd_lib::run_fun;
    use semver::Version;
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

    /// Yields `data` and then fails, like a connection dropped mid-download.
    struct CutShortReader {
        data: &'static [u8],
    }

    impl std::io::Read for CutShortReader {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            if self.data.is_empty() {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::ConnectionReset,
                    "peer disconnected",
                ));
            }
            let n = buf.len().min(self.data.len());
            buf[..n].copy_from_slice(&self.data[..n]);
            self.data = &self.data[n..];
            Ok(n)
        }
    }

    fn dir_entries(dir: &std::path::Path) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(dir)
            .expect("failed to read dir")
            .map(|e| {
                e.expect("bad dir entry")
                    .file_name()
                    .to_string_lossy()
                    .into_owned()
            })
            .collect();
        names.sort();
        names
    }

    #[test]
    fn write_to_file_atomically_writes_content_and_leaves_no_temp_file() {
        let td = tempdir().expect("failed to create tempdir");
        let dest = td.path().join("list.txt");
        let content = b"AAAC\nAAAG\nAAAT\n";
        super::write_to_file_atomically(&mut &content[..], &dest).expect("write should succeed");
        assert_eq!(fs::read(&dest).expect("failed to read dest"), content);
        assert_eq!(dir_entries(td.path()), vec!["list.txt".to_string()]);

        // an existing file is replaced, as `File::create` truncated it before
        super::write_to_file_atomically(&mut &b"AAAC\n"[..], &dest)
            .expect("overwrite should succeed");
        assert_eq!(fs::read(&dest).expect("failed to read dest"), b"AAAC\n");
        assert_eq!(dir_entries(td.path()), vec!["list.txt".to_string()]);
    }

    #[test]
    fn write_to_file_atomically_leaves_nothing_when_the_reader_fails() {
        let td = tempdir().expect("failed to create tempdir");
        let dest = td.path().join("list.txt");
        let mut reader = CutShortReader {
            data: b"AAAC\nAAAG\nAA",
        };
        let err = super::write_to_file_atomically(&mut reader, &dest)
            .expect_err("a reader error must fail the write");
        assert!(
            format!("{:#}", err).contains("peer disconnected"),
            "unexpected error: {:#}",
            err
        );
        // neither the partial content nor the temporary file is left behind
        assert!(!dest.exists());
        assert!(dir_entries(td.path()).is_empty());
    }

    #[test]
    fn write_to_file_atomically_keeps_the_previous_file_when_the_reader_fails() {
        let td = tempdir().expect("failed to create tempdir");
        let dest = td.path().join("list.txt");
        fs::write(&dest, b"previous\n").expect("failed to write previous file");
        let mut reader = CutShortReader { data: b"new con" };
        super::write_to_file_atomically(&mut reader, &dest).expect_err("a reader error must fail");
        assert_eq!(fs::read(&dest).expect("failed to read dest"), b"previous\n");
        assert_eq!(dir_entries(td.path()), vec!["list.txt".to_string()]);
    }

    #[cfg(unix)]
    #[test]
    fn write_to_file_atomically_uses_the_file_create_mode() {
        use std::os::unix::fs::PermissionsExt;

        let td = tempdir().expect("failed to create tempdir");
        let dest = td.path().join("list.txt");
        super::write_to_file_atomically(&mut &b"AAAC\n"[..], &dest).expect("write should succeed");
        let reference = td.path().join("reference.txt");
        fs::File::create(&reference).expect("failed to create reference file");
        let mode = |p: &std::path::Path| {
            fs::metadata(p)
                .expect("failed to stat")
                .permissions()
                .mode()
                & 0o777
        };
        assert_eq!(mode(&dest), mode(&reference));
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

    fn version_from_output(prog: &str, req: &str, output: &str) -> anyhow::Result<Version> {
        super::check_version_constraints_from_output(prog, req, Ok(output.to_string()))
    }

    #[test]
    fn version_from_output_checks_the_last_word() {
        let v = version_from_output("alevin-fry", ">=0.18.3, <1.0.0", "alevin-fry 0.19.0\n")
            .expect("0.19.0 satisfies >=0.18.3");
        assert_eq!(v, Version::new(0, 19, 0));
        let err = version_from_output("piscem", ">=0.24.0, <1.0.0", "piscem 0.23.0")
            .expect_err("0.23.0 is below the floor");
        assert_eq!(
            err.to_string(),
            "Parsed version of piscem (\"0.23.0\") does not satisfy constraints >=0.24.0, <1.0.0. \
             Please install a compatible version."
        );
        // more than 3 dotted parts (macs) -> only major.minor.patch is checked
        let v = version_from_output("macs3", ">=3.0.2, <4.0.0", "macs3 3.0.3.dev0")
            .expect("3.0.3.dev0 is checked as 3.0.3");
        assert_eq!(v, Version::new(3, 0, 3));
        let v = version_from_output("macs2", ">=2.2.0", "macs2 2.2.9.1")
            .expect("2.2.9.1 is checked as 2.2.9");
        assert_eq!(v, Version::new(2, 2, 9));
    }

    #[test]
    fn version_from_output_keeps_valid_semver_intact() {
        // "0.24.0-rc.1" has 4 dotted parts but is valid semver; it used to be
        // cut to "0.24.0-rc", which `>=0.24.0-rc.1` then rejected.
        let v = version_from_output("piscem", ">=0.24.0-rc.1, <1.0.0", "piscem 0.24.0-rc.1")
            .expect("0.24.0-rc.1 satisfies >=0.24.0-rc.1");
        assert_eq!(v.to_string(), "0.24.0-rc.1");
        let v = version_from_output(
            "alevin-fry",
            ">=0.18.3, <1.0.0",
            "alevin-fry 0.19.0+g1a2b.3",
        )
        .expect("build metadata does not affect matching");
        assert_eq!(v.to_string(), "0.19.0+g1a2b.3");
        let err = version_from_output("piscem", ">=0.24.0, <1.0.0", "piscem 0.23.1-rc.1")
            .expect_err("0.23.1-rc.1 is below the floor");
        assert!(
            err.to_string().contains("(\"0.23.1-rc.1\")"),
            "unexpected error: {}",
            err
        );
    }

    #[test]
    fn check_version_constraints_accepts_prereleases_of_satisfying_releases() {
        let ok = |v: &str, req: &str| super::check_version_constraints("piscem", req, v).is_ok();
        // a pre-release is judged by its release version ...
        assert!(ok("0.24.0-rc.1", ">=0.24.0, <1.0.0"));
        assert!(ok("0.24.0-dev", ">=0.24.0, <1.0.0"));
        assert!(ok("0.25.0-rc.1", ">=0.24.0, <1.0.0"));
        assert!(ok("0.25.0-dev", ">=0.24.0-0, <1.0.0"));
        assert!(ok("0.24.0-rc.3", ">=0.24.0-rc.2, <1.0.0"));
        // ... so one below the floor, or of an excluded upper bound, is rejected
        assert!(!ok("0.23.1-rc.1", ">=0.24.0, <1.0.0"));
        assert!(!ok("0.22.9-rc.1", ">=0.23.0, <1.0.0"));
        assert!(!ok("1.0.0-rc.1", ">=0.24.0, <1.0.0"));
        assert!(!ok("0.24.0-rc.1", "<0.24.0"));
        // a floor naming a pre-release still orders pre-releases of that release
        assert!(!ok("0.24.0-rc.1", ">=0.24.0-rc.2, <1.0.0"));
        // release versions are matched exactly as before
        assert!(ok("0.24.0", ">=0.24.0, <1.0.0"));
        assert!(ok("0.24.0+g1a2b", ">=0.24.0, <1.0.0"));
        assert!(!ok("0.23.9", ">=0.24.0, <1.0.0"));
        assert!(!ok("1.0.0", ">=0.24.0, <1.0.0"));
        // the returned version keeps its pre-release tag
        let v = super::check_version_constraints("piscem", ">=0.24.0, <1.0.0", "0.24.0-rc.1")
            .expect("0.24.0-rc.1 is accepted");
        assert_eq!(v.to_string(), "0.24.0-rc.1");
    }

    #[test]
    fn version_from_output_accepts_release_candidates_of_the_floor() {
        for (prog, req, output) in [
            ("piscem", ">=0.24.0, <1.0.0", "piscem 0.24.0-rc.1"),
            ("alevin-fry", ">=0.19.0, <1.0.0", "alevin-fry 0.19.0-rc.1"),
        ] {
            let v = version_from_output(prog, req, output).expect("an rc of the floor passes");
            // set-paths records `v`; the cached re-checks must accept it too
            let recorded = format!("{}", v);
            assert_eq!(Some(recorded.as_str()), output.split_whitespace().last());
            super::check_version_constraints(prog, req, &recorded)
                .expect("recorded version re-checks");
        }
        assert!(version_from_output("piscem", ">=0.24.0, <1.0.0", "piscem 0.23.1-rc.1").is_err());
    }

    #[test]
    fn version_from_output_reports_run_failures_and_empty_output() {
        let err = super::check_version_constraints_from_output(
            "piscem",
            ">=0.24.0, <1.0.0",
            Err(std::io::Error::other("exited with status 127")),
        )
        .expect_err("a failed `--version` run is an error");
        let msg = format!("{:#}", err);
        assert!(
            msg.contains("could not run `piscem --version`") && msg.contains("status 127"),
            "unexpected error: {}",
            msg
        );
        let err = version_from_output("piscem", ">=0.24.0, <1.0.0", " \n")
            .expect_err("empty output has no version");
        assert!(
            format!("{:#}", err).contains("`piscem --version`"),
            "unexpected error: {:#}",
            err
        );
    }
}
