// This crate is a modified version of jrsonnet cli.
// https://github.com/CertainLach/jrsonnet/blob/master/cmds/jrsonnet/src/main.rs

use crate::utils::workflow_utils::JsonPatch;
use anyhow::{anyhow, bail, Context};
use clap::Parser;
use jrsonnet_cli::{GcOpts, ManifestOpts, MiscOpts, OutputOpts, StdOpts, TlaOpts, TraceOpts};
use jrsonnet_evaluator::{
    apply_tla,
    error::{Error as JrError, ErrorKind},
    State,
};
use std::path::{Path, PathBuf};

#[derive(Parser)]
struct InputOpts {
    /// Treat input as code, evaluate them instead of reading file
    #[arg(long, short = 'e')]
    pub exec: bool,

    /// Path to the file to be compiled if `--evaluate` is unset, otherwise code itself
    pub input: Option<String>,
}

/// Jsonnet commandline interpreter (Rust implementation)
#[derive(Parser)]
struct Opts {
    #[clap(flatten)]
    input: InputOpts,
    #[clap(flatten)]
    misc: MiscOpts,
    #[clap(flatten)]
    tla: TlaOpts,
    #[clap(flatten)]
    std: StdOpts,
    #[clap(flatten)]
    gc: GcOpts,

    #[clap(flatten)]
    trace: TraceOpts,
    #[clap(flatten)]
    manifest: ManifestOpts,
    #[clap(flatten)]
    output: OutputOpts,
}

#[derive(Debug, PartialEq, Eq)]
pub enum TemplateState {
    Uninstantiated,
    Instantiated,
}

impl TemplateState {
    pub fn is_instantiated(&self) -> bool {
        match &self {
            TemplateState::Uninstantiated => false,
            TemplateState::Instantiated => true,
        }
    }
}

pub fn parse_jsonnet(
    config_file_path: &Path,
    output_opt: Option<PathBuf>,
    utils_dir: &Path,
    jpaths: &Option<Vec<PathBuf>>,
    ext_codes: &Option<Vec<String>>,
    patch: &Option<&JsonPatch>,
    template_state: TemplateState,
) -> anyhow::Result<String> {
    // define jrsonnet arguments
    // config file
    let instantiated = template_state.is_instantiated();
    let tla_config_file_path = format!(
        "workflow={}",
        config_file_path.to_str().with_context(|| {
            format!(
                "Could not convert workflow config file path to str: {:?}",
                config_file_path
            )
        })?
    );

    let ext_utils_file_path = r#"__utils=import 'simpleaf_workflow_utils.libsonnet'"#;
    let ext_instantiated = format!(r#"__instantiated='{}'"#, instantiated);

    // af_home_dir
    let jpath_pe_utils = utils_dir.to_str().with_context(|| {
        format!(
            "Could not convert Protocol Estuarys path to str: {:?}",
            utils_dir
        )
    })?;

    // get main.jsonnet file path
    let main_jsonnet_file_path = utils_dir.join("main.jsonnet");
    if !main_jsonnet_file_path.exists() {
        bail!("Could not find main.jsonnet file protocol-asturay; Please update it by invoking `simpleaf workflow refresh`")
    }
    let main_jsonnet_file_str = main_jsonnet_file_path.to_str().with_context(|| {
        format!(
            "Could not convert main.jsonnet file path to str: {:?}",
            main_jsonnet_file_path
        )
    })?;

    // if we patch, output_opt will always be None
    let ext_output = if let Some(output) = output_opt {
        format!(r#"__output='{}'"#, output.display())
    } else {
        format!(r#"__output=null"#)
    };

    // create command vector for clap parser
    let mut jrsonnet_cmd_vec = vec![
        "jrsonnet",
        main_jsonnet_file_str,
        "--ext-code",
        ext_utils_file_path,
        "--ext-code",
        &ext_output,
        "--ext-code",
        &ext_instantiated,
        "--jpath",
        jpath_pe_utils,
        "--tla-code-file",
        tla_config_file_path.as_str(),
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

    // if the user provides patch, then assign it.
    let patch_string = if let Some(patch) = patch {
        jrsonnet_cmd_vec.push("--tla-code");
        jrsonnet_cmd_vec.push(r#"patch=true"#);
        jrsonnet_cmd_vec.push("--tla-code");
        Some(format!("json={}", patch.patch))
    } else {
        None
    };
    if let Some(s) = &patch_string {
        jrsonnet_cmd_vec.push(s.as_str());
    }

    let opts: Opts = Opts::parse_from(jrsonnet_cmd_vec);
    main_catch(opts)
}

#[derive(thiserror::Error, Debug)]
enum Error {
    // Handled differently
    #[error("evaluation error")]
    Evaluation(JrError),
    #[error("IO error")]
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
    let trace = opts.trace.trace_format();
    match main_real(&s, opts) {
        Ok(js) => Ok(js),
        Err(e) => {
            if let Error::Evaluation(e) = e {
                let mut out = String::new();
                trace.write_trace(&mut out, &e).expect("format error");
                Err(anyhow!(
                    "Jsonnet {out}"
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
    let _gc_leak_guard = opts.gc.leak_on_exit();
    let _gc_print_stats = opts.gc.stats_printer();
    let _stack_depth_override = opts.misc.stack_size_override();

    let import_resolver = opts.misc.import_resolver();
    s.set_import_resolver(import_resolver);

    let std = opts.std.context_initializer(s)?;
    if let Some(std) = std {
        s.set_context_initializer(std);
    }

    let input = opts.input.input.ok_or(Error::MissingInputArgument)?;
    let val = s.import(input)?;

    let tla = opts.tla.tla_opts()?;
    let val = apply_tla(s.clone(), &tla, val)?;

    let manifest_format = opts.manifest.manifest_format();
    let output = val.manifest(manifest_format)?;

    if !output.is_empty() {
        Ok(output)
    } else {
        Err(Error::EmptyJSON)
    }
}
