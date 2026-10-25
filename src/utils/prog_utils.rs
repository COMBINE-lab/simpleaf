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
    /// piscem 0.24.0 is the first release built against piscem-rs 0.11
    /// (sshash-lib 0.8, whose MPHF changes the serialized format): its index
    /// format is incompatible with earlier releases in BOTH directions —
    /// 0.24 rejects 0.23-built indexes with a rebuild message (contig/EC
    /// tables are now `PCTAB04`/`PECTB04`), and older piscem cannot read
    /// 0.24-built indexes or the `PPOIS02` poison tables 0.24 writes.
    /// Requiring >=0.24 keeps the piscem simpleaf indexes with and the piscem
    /// it maps with format-consistent. It is also what makes the
    /// `--excluded-probes decoy` default affordable: 0.24 filters poison
    /// k-mers at almost no cost, where 0.23 maps 10x Flex reads 40-55% slower
    /// once the decoys are indexed.
    /// 0.24.0 also writes RAD v2, unconditionally. That is the reason for the
    /// [`ALEVIN_FRY`] floor, not for this one: the alevin-fry that reads RAD v2
    /// still reads the RAD v1 that piscem 0.23 writes.
    /// (0.22.0 introduced `--decoder`/`--thread-policy` and the shared `-t`
    /// budget, which simpleaf forwards unconditionally.)
    pub const PISCEM: &str = ">=0.24.0, <1.0.0";

    /// alevin-fry 0.18.0 provides deterministic compiled barcode-correction
    /// plans and the correction/resource controls forwarded by simpleaf 0.28.
    /// 0.18.3 is required because collate is now invoked with `--compress=<codec>`
    /// (per-chunk codec, on by default): earlier releases spell `--compress` as a
    /// bare boolean flag and reject the `=<codec>` value, which would fail at
    /// collate on every run.
    /// 0.19.0 is required because piscem >= 0.24 writes RAD v2: it is the first
    /// release built on a libradicl (0.21) that reads RAD v2, and it still reads
    /// RAD v1. alevin-fry 0.18.x cannot parse a RAD v2 header, and
    /// `generate-permit-list` aborts on it ("invalid utf-8 sequence"), only
    /// after mapping has finished. This floor also covers `--map-dir`, whose
    /// RAD file simpleaf never inspects.
    pub const ALEVIN_FRY: &str = ">=0.19.0, <1.0.0";

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
        AlevinFryCapabilities, ProgInfo, ReqProgs, check_files_exist, check_piscem_index_files,
        ensure_alevin_fry_version, help_defines_flag,
    };
    use cmd_lib::run_fun;
    use semver::Version;
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
        // meets the >=0.19.0 floor
        assert!(ensure_alevin_fry_version(&req_progs_with_af("0.19.0")).is_ok());
        assert!(ensure_alevin_fry_version(&req_progs_with_af("0.20.0")).is_ok());
        // below the floor -> clear error before any work (0.18.3 cannot read
        // the RAD v2 that piscem >= 0.24 writes)
        assert!(ensure_alevin_fry_version(&req_progs_with_af("0.18.3")).is_err());
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
