// This crate is a modified version of jrsonnet cli.
// https://github.com/CertainLach/jrsonnet/blob/master/cmds/jrsonnet/src/main.rs

use anyhow::anyhow;
use clap::Parser;
use jrsonnet_cli::{ConfigureState, GeneralOpts, ManifestOpts, OutputOpts, TraceOpts};
use jrsonnet_evaluator::{
    apply_tla,
    error::{Error as JrError, ErrorKind},
    State,
};
use std::path::Path;

#[derive(Parser)]
#[command(next_help_heading = "DEBUG")]
struct DebugOpts {
    /// Required OS stack size.
    /// This shouldn't be changed unless jrsonnet is failing with stack overflow error.
    #[arg(long, id = "size")]
    pub os_stack: Option<usize>,
}

#[derive(Parser)]
#[command(next_help_heading = "INPUT")]
struct InputOpts {
    /// Treat input as code, evaluate them instead of reading file
    #[arg(long, short = 'e')]
    pub exec: bool,

    /// Path to the file to be compiled if `--evaluate` is unset, otherwise code itself
    pub input: Option<String>,
}

/// Jsonnet commandline interpreter (Rust implementation)
#[derive(Parser)]
#[command(
    args_conflicts_with_subcommands = true,
    disable_version_flag = true,
    version,
    author
)]
struct Opts {
    #[clap(flatten)]
    input: InputOpts,
    #[clap(flatten)]
    general: GeneralOpts,

    #[clap(flatten)]
    trace: TraceOpts,
    #[clap(flatten)]
    manifest: ManifestOpts,
    #[clap(flatten)]
    output: OutputOpts,
    #[clap(flatten)]
    debug: DebugOpts,
}

pub fn parse_jsonnet(
    // af_home_path: &Path,
    config_file_path: &Path,
    utils_libsonnet_path: &Path,
    output: &Path,
) -> anyhow::Result<String> {
    // define top level argumetns
    // let tla_af_home_path = format!("af_home_path='{}'", af_home_path.display());
    let tla_output = format!("\"output_dir='{}'\"", output.to_string_lossy().into_owned());
    let ext_utils_file_path = format!("\"utils = import '{}'\"", utils_libsonnet_path.display());
    let input_config_file_path = config_file_path.to_string_lossy().into_owned();

    // create command vector for clap parser
    let jrsonnet_cmd_vec = vec![
        "jrsonnet",
        &input_config_file_path,
        "--tla-code",
        &tla_output,
        "--ext-code",
        &ext_utils_file_path,
    ];
    let opts: Opts = Opts::parse_from(jrsonnet_cmd_vec);
    main_catch(opts)
}

#[derive(thiserror::Error, Debug)]
enum Error {
    // Handled differently
    #[error("evaluation error")]
    Evaluation(JrError),
    #[error("io error")]
    Io(#[from] std::io::Error),
    #[error("input is not utf8 encoded")]
    Utf8(#[from] std::str::Utf8Error),
    #[error("missing input argument")]
    MissingInputArgument,
    #[error("Evaluated empty JSON record")]
    EmptyJSON,
}
impl From<JrError> for Error {
    fn from(e: JrError) -> Self {
        Self::Evaluation(e)
    }
}
impl From<ErrorKind> for Error {
    fn from(e: ErrorKind) -> Self {
        Self::from(JrError::from(e))
    }
}

fn main_catch(opts: Opts) -> anyhow::Result<String> {
    let s = State::default();
    let trace = opts
        .trace
        .configure(&s)
        .expect("this configurator doesn't fail");
    match main_real(&s, opts) {
        Ok(js) => Ok(js),
        Err(e) => {
            if let Error::Evaluation(e) = e {
                let mut out = String::new();
                trace.write_trace(&mut out, &e).expect("format error");
                Err(anyhow!(
                    "Error Occurred when evaluating a configuration file. Cannot proceed. {out}"
                ))
            } else {
                Err(anyhow!(
                    "Found invalid configuration file. The error message was: {e}"
                ))
            }
        }
    }
}

fn main_real(s: &State, opts: Opts) -> Result<String, Error> {
    let (tla, _gc_guard) = opts.general.configure(s)?;
    let manifest_format = opts.manifest.configure(s)?;

    let input = opts.input.input.ok_or(Error::MissingInputArgument)?;
    let val = s
        .import(input)
        .expect("Cannot import workflow config file.");

    let val = apply_tla(s.clone(), &tla, val)?;

    let output = val.manifest(manifest_format)?;
    if !output.is_empty() {
        Ok(output)
    } else {
        Err(Error::EmptyJSON)
    }
}
