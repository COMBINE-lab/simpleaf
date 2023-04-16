// This crate is a modified version of jrsonnet cli.
// https://github.com/CertainLach/jrsonnet/blob/master/cmds/jrsonnet/src/main.rs

use anyhow::{anyhow, Context};
use clap::Parser;
use jrsonnet_cli::{ConfigureState, GeneralOpts, ManifestOpts, OutputOpts, TraceOpts};
use jrsonnet_evaluator::{
    apply_tla,
    error::{Error as JrError, ErrorKind},
    State,
};
use std::path::{Path, PathBuf};

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
    config_file_path: &Path,
    output: &Path,
    utils_dir: &Path,
    jpaths: &Option<Vec<PathBuf>>,
    ext_codes: &Option<Vec<String>>,
    instantiated: bool,
) -> anyhow::Result<String> {
    // define jrsonnet argumetns
    // config file
    let input_config_file_path = config_file_path
        .to_str()
        .expect("Could not convert workflow config file path to str");
    let ext_output = format!(r#"__output='{}'"#, output.display());
    let ext_utils_file_path = r#"__utils=import 'simpleaf_workflow_utils.libsonnet'"#;
    let ext_instantiated = format!(r#"__instantiated='{}'"#, instantiated);

    // af_home_dir
    let jpath_pe_utils = utils_dir
        .to_str()
        .expect("Could not convert Protocol Estuarys path to str");

    // create command vector for clap parser
    let mut jrsonnet_cmd_vec = vec![
        "jrsonnet",
        input_config_file_path,
        "--ext-code",
        &ext_output,
        "--ext-code",
        ext_utils_file_path,
        "--ext-code",
        &ext_instantiated,
        "--jpath",
        jpath_pe_utils,
    ];

    // if the user provides more lib search path, then assign it.
    if let Some(jpaths) = jpaths {
        for lib_path in jpaths {
            jrsonnet_cmd_vec.push("--jpath");
            jrsonnet_cmd_vec.push(lib_path.to_str().with_context(|| {
                format!("Could not convert the following path to str {:?}", lib_path)
            })?);
        }
    }

    // if the user provides ext-code, then assign it.
    if let Some(ext_codes) = ext_codes {
        for ext_code in ext_codes {
            jrsonnet_cmd_vec.push("--ext-code");
            jrsonnet_cmd_vec.push(ext_code.as_str());
        }
    }

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
    let val = s.import(input)?;

    let val = apply_tla(s.clone(), &tla, val)?;

    let output = val.manifest(manifest_format)?;
    if !output.is_empty() {
        Ok(output)
    } else {
        Err(Error::EmptyJSON)
    }
}
