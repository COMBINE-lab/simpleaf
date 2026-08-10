// This crate is a modified version of jrsonnet cli.
// https://github.com/CertainLach/jrsonnet/blob/master/cmds/jrsonnet/src/main.rs

use crate::utils::workflow_utils::JsonPatch;
use anyhow::{Context, anyhow, bail};
use clap::Parser;
use jrsonnet_cli::{GcOpts, ManifestOpts, MiscOpts, OutputOpts, StdOpts, TlaOpts, TraceOpts};
use jrsonnet_evaluator::{
    State, apply_tla,
    error::{Error as JrError, ErrorKind},
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

#[allow(dead_code)]
#[derive(Debug, PartialEq, Eq)]
pub enum ParseAction {
    Inspect,
    Instantiate,
    InstantiateWithoutValidation,
}

impl ParseAction {
    pub fn requires_validation(&self) -> bool {
        match &self {
            Self::Inspect => false,
            Self::Instantiate => true,
            Self::InstantiateWithoutValidation => false,
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
    template_state: ParseAction,
) -> anyhow::Result<String> {
    // define jrsonnet arguments
    // config file
    let do_validate = template_state.requires_validation();
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
    let ext_instantiated = format!(r#"__validate={}"#, do_validate);

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
        bail!(
            "Could not find main.jsonnet file protocol-asturay; Please update it by invoking `simpleaf workflow refresh`"
        )
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
        r#"__output=null"#.to_string()
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
    let trace = opts.trace.trace_format();
    let eval_result = main_real(opts);
    match eval_result {
        Ok(js) => Ok(js),
        Err(e) => {
            if let Error::Evaluation(e) = e {
                let mut out = String::new();
                trace.write_trace(&mut out, &e).expect("format error");
                Err(anyhow!("Jsonnet {out}"))
            } else {
                Err(anyhow!(
                    "Found invalid configuration file. The error message was: {e}"
                ))
            }
        }
    }
}

fn main_real(opts: Opts) -> Result<String, Error> {
    let _gc_leak_guard = opts.gc.leak_on_exit();
    let _gc_print_stats = opts.gc.stats_printer();
    let _stack_depth_override = opts.misc.stack_size_override();

    let import_resolver = opts.misc.import_resolver();
    let mut state_builder = State::builder();
    state_builder.import_resolver(import_resolver);

    let std = opts.std.context_initializer()?;
    if let Some(std) = std {
        state_builder.context_initializer(std);
    }
    let s = state_builder.build();

    // jrsonnet 0.5.0-pre98 resolves the evaluation state from a thread-local
    // rather than taking it as an argument. `apply_tla` no longer receives the
    // `State`, so it looks the current one up itself — and if none has been
    // entered it silently falls back to `DEFAULT_STATE`, which carries no
    // import resolver. Our TLA argument is `--tla-code-file workflow=<path>`,
    // an import, so without this guard every template fails with "imports are
    // not supported" and the caller only sees a missing version.
    //
    // The guard must outlive the `apply_tla` call below; it un-installs the
    // state on drop.
    let _state_guard = s.enter();

    let input = opts.input.input.ok_or(Error::MissingInputArgument)?;
    // jrsonnet 0.5.0-pre98: `import` takes `impl AsPathLike`, which `str`
    // implements but `String` does not.
    let val = s.import(input.as_str())?;

    let tla = opts.tla.tla_opts()?;
    let val = apply_tla(&tla, val)?;

    let manifest_format = opts.manifest.manifest_format();
    let output = val.manifest(manifest_format)?;

    if !output.is_empty() {
        Ok(output)
    } else {
        Err(Error::EmptyJSON)
    }
}

#[cfg(test)]
mod tests {
    use super::{ParseAction, parse_jsonnet};
    use std::io::Write;

    /// `parse_jsonnet` must be able to resolve imports while applying the
    /// top-level argument.
    ///
    /// The workflow template is handed to jsonnet as
    /// `--tla-code-file workflow=<path>`, which is an *import* performed during
    /// the TLA call. jrsonnet 0.5.0-pre98 stopped passing the evaluation state
    /// into `apply_tla` and now looks it up from a thread-local instead; if no
    /// state has been entered it quietly falls back to a default that has no
    /// import resolver. Every template then fails with "imports are not
    /// supported", which `get_template_version` swallows into a bare "N/A*" in
    /// a table — a regression with no error message anywhere.
    ///
    /// This builds a miniature protocol estuary so the check needs no network,
    /// and asserts on a value that can only have come through the TLA import.
    #[test]
    fn parse_jsonnet_resolves_imports_during_tla() {
        let dir = tempfile::tempdir().expect("could not create temp dir");
        let utils_dir = dir.path().join("utils");
        std::fs::create_dir_all(&utils_dir).unwrap();

        // Stand-ins for the estuary's own files. `main.jsonnet` takes the
        // workflow as a top-level argument and returns it, which is the
        // smallest thing that still forces the import to be resolved.
        let mut main = std::fs::File::create(utils_dir.join("main.jsonnet")).unwrap();
        writeln!(main, "function(workflow,patch=false,json={{}}) workflow").unwrap();

        // Imported via `--ext-code __utils=import '...'`, so it has to exist on
        // the jpath even though this stub is empty.
        let mut lib =
            std::fs::File::create(utils_dir.join("simpleaf_workflow_utils.libsonnet")).unwrap();
        writeln!(lib, "{{}}").unwrap();

        let template_path = dir.path().join("template.jsonnet");
        let mut template = std::fs::File::create(&template_path).unwrap();
        writeln!(
            template,
            r#"{{ meta_info: {{ template_version: "9.9.9" }}, value: 42 }}"#
        )
        .unwrap();

        let out = parse_jsonnet(
            &template_path,
            Some(std::path::PathBuf::from(".")),
            &utils_dir,
            &None,
            &None,
            &None,
            ParseAction::Inspect,
        )
        .expect("parse_jsonnet failed; imports during the TLA call are most likely unresolved");

        let v: serde_json::Value =
            serde_json::from_str(&out).expect("parse_jsonnet did not return JSON");
        assert_eq!(
            v.pointer("/meta_info/template_version")
                .and_then(|x| x.as_str()),
            Some("9.9.9"),
            "template contents did not survive the TLA import: {out}"
        );
        assert_eq!(v.pointer("/value").and_then(|x| x.as_i64()), Some(42));
    }
}
