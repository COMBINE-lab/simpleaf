use crate::utils::jrsonnet_main::{parse_jsonnet, ParseAction};
use crate::utils::prog_utils;
use crate::utils::prog_utils::ReqProgs;
use crate::utils::workflow_utils;

use anyhow::{bail, Context};
use cmd_lib::run_fun;
use serde_json::json;
use serde_json::Value;
use std::fs;
use std::path::Path;
use tabled::{settings::Style, Table, Tabled};
use tracing::{info, warn};

use super::WorkflowCommands;

#[derive(Tabled)]
struct WorkflowTemplate {
    registry: String,
    name: String,
    version: String,
}

pub fn refresh_protocol_estuary<T: AsRef<Path>>(af_home_path: T) -> anyhow::Result<()> {
    workflow_utils::get_protocol_estuary(
        af_home_path.as_ref(),
        workflow_utils::RegistrySourceStrategy::ForceRefresh,
    )?;
    Ok(())
}

/// This function takes a patch command, represented as the Patch
/// variant of the WorkflowCommands enum, which specifies *either* a
/// template or a manifest, as well as the path to a patch file and
/// optionally an output directory.
///
/// The function first generates JSON patches from the input patch file
/// and then applies these patches one at a time to the provided template
/// or manifest.
///
/// If the optional output directory is provided, the resulting patched
/// manfiests are written to that directory. Otherwise, they are written
/// in the same directory as the input template or manifest.
///
/// If all patches are applied and written succesfully, this function returns
/// OK(()). This function returns an Error if any error occurs in the processing
/// and application of the patches, which could include errors related to
/// instantiation of the underlying template, failure to parse the provided
/// patch file into JSON records, or even permission errors on the specified
/// output destination.
pub fn patch_manifest_or_template<T: AsRef<Path>>(
    af_home_path: T,
    workflow_cmd: WorkflowCommands,
) -> anyhow::Result<()> {
    // get protocol_estuary path
    let protocol_estuary = workflow_utils::get_protocol_estuary(
        af_home_path.as_ref(),
        workflow_utils::RegistrySourceStrategy::PreferLocal,
    )?;

    match workflow_cmd {
        WorkflowCommands::Patch {
            manifest,
            template,
            patch,
            output,
        } => {
            // generate a set of JSON patch files from the input
            // semicolon separated CSV file.
            let target = if template.is_some() {
                workflow_utils::PatchTargetType::Template
            } else {
                workflow_utils::PatchTargetType::Manifest
            };

            if let Some(o) = &output {
                fs::create_dir_all(o)?;
            }

            let patches: workflow_utils::PatchCollection =
                workflow_utils::patches_from_csv(patch, target)?;

            let template_value = template.unwrap_or_else(|| manifest.unwrap());

            let template_value = template_value.canonicalize()?;
            for p in patches.iter() {
                // call parse_jsonnet to patch the template
                match parse_jsonnet(
                    // af_home_path,
                    template_value.as_ref(),
                    None,
                    &protocol_estuary.utils_dir,
                    &None,
                    &None,
                    &Some(p),
                    ParseAction::Instantiate,
                ) {
                    Ok(js) => {
                        let v: Value = serde_json::from_str(js.as_str())?;

                        // get template location
                        let patch_name = if let Some(stem) = template_value.file_stem() {
                            format!("{}_{}.json", Path::new(stem).display(), p.name)
                        } else {
                            format!("{}.json", p.name)
                        };
                        let path = output.clone().map_or_else(
                            || template_value.with_file_name(&patch_name),
                            |mut v| {
                                v.push(&patch_name);
                                v
                            },
                        );
                        let fw = std::fs::File::create(path)?;
                        serde_json::to_writer_pretty(fw, &v)?
                    }
                    Err(e) => bail!(
                        "Failed patching file {} using patch {}. {}",
                        template_value.display(),
                        p.name,
                        e
                    ),
                };
            }
        }
        _ => {
            bail!("The patch function received a non-patch command. This should not happen. Please report this issue.");
        }
    }
    Ok(())
}

pub fn list_workflows<T: AsRef<Path>>(af_home_path: T) -> anyhow::Result<()> {
    // get af_home
    let v: Value = prog_utils::inspect_af_home(af_home_path.as_ref())?;
    // Read the JSON contents of the file as an instance of `User`.
    // TODO: use it somehwere?
    let _rp: ReqProgs = serde_json::from_value(v["prog_info"].clone())?;

    // get protocol library path
    let protocol_estuary = workflow_utils::get_protocol_estuary(
        af_home_path.as_ref(),
        workflow_utils::RegistrySourceStrategy::PreferLocal,
    )?;
    // get the corresponding workflow directory path
    let workflow_path = protocol_estuary.protocols_dir.as_path();
    let workflows = fs::read_dir(workflow_path)?;
    let mut print_na_cap = false;
    let na_string = String::from("N/A*");
    let mut workflow_entries = vec![];
    for prot in workflows {
        if let Ok(prot) = prot {
            let version =
                workflow_utils::get_template_version(prot.path(), &protocol_estuary.utils_dir)?;
            if version == na_string {
                print_na_cap = true;
            }
            let n = format!("{:?}", prot.file_name());
            workflow_entries.push(WorkflowTemplate {
                registry: String::from("COMBINE-lab/protocol-estuary"),
                name: n,
                version,
            })
        } else {
            warn!("Cannot traverse directory {:?}", workflow_path)
        }
    }
    println!("{}", Table::new(workflow_entries).with(Style::rounded()));
    if print_na_cap {
        println!("* : could not parse uninstantiated template to attempt extracting the version, please see [shorturl.at/gouB1] for further details");
    }
    Ok(())
}

/// ### Program Name
/// simpleaf get-workflow-config
///
/// ### Program Input
/// A string representing the name of an existing workflow in the protocol-estuary
/// A output path
///
/// ### Program Output
/// A folder that in the protocol estuary that is named by the querying workflow.
///
/// ### Program Description
/// This program is used for getting the source files of a pubished workflow
/// from the protocol estuary GitHub repo https://github.com/COMBINE-lab/protocol-estuary
///
/// This program takes a string representing the name of a published workflow, and copy the
/// folder of that workflow in the protocol estuary to the provided output directory
/// as a sub-directory.
// TODO: implement essential only
pub fn get_wokflow<T: AsRef<Path>>(
    af_home_path: T,
    gw_cmd: WorkflowCommands,
) -> anyhow::Result<()> {
    match gw_cmd {
        WorkflowCommands::Get {
            output,
            name,
            // essential_only: _,
        } => {
            // get af_home
            let v: Value = prog_utils::inspect_af_home(af_home_path.as_ref())?;
            // Read the JSON contents of the file as an instance of `User`.
            // TODO: use it somehwere?
            let _rp: ReqProgs = serde_json::from_value(v["prog_info"].clone())?;

            // get protocol library path
            let protocol_estuary = workflow_utils::get_protocol_estuary(
                af_home_path.as_ref(),
                workflow_utils::RegistrySourceStrategy::PreferLocal,
            )?;
            // get the corresponding workflow directory path
            let workflow_path = protocol_estuary.protocols_dir.join(name.as_str());

            let version_str = workflow_utils::get_template_version(
                workflow_path.clone(),
                &protocol_estuary.utils_dir,
            )?;

            /* NOTE: we can't check against this version for very old templates, because we can't
             * even instantaite them any longer because of how the evaluation has changed. Only
             * check the below if what we get back is not "N/A*" or "missing"
             */
            match version_str.as_ref() {
                "N/A*" => {
                    warn!("couldn't evaluate the requested template to fetch the version number, it may be a deprecated version; consider refreshing.");
                }
                "missing" => {
                    warn!("the template that was requested to be fetched appeared to be missing a version number, but this field should be present; consider refreshing or further investigating the issue.");
                }
                ver => {
                    const REQ_VER: &str = "0.1.0";
                    match prog_utils::check_version_constraints(&name, REQ_VER, ver) {
                        Ok(ver) => {
                            info!("getting workflow {} version {}", name, ver);
                        }
                        Err(_) => {
                            warn!("the version parsed from the workflow you are attempting to get is {}, but it should be at least {}.", version_str, REQ_VER);
                        }
                    }
                }
            };

            // make output dir
            let mut output_dir_name = name.clone();
            output_dir_name.push_str("_template");
            let output_path = output.join(output_dir_name);

            // check if workflow path exists
            match workflow_path.try_exists() {
                // if it exists, then copy this folder to the output dir
                Ok(true) => {
                    info!("Exporting workflow files to the output folder");

                    match workflow_utils::copy_dir_all(
                        workflow_path.as_path(),
                        output_path.as_path(),
                    ) {
                        Ok(_) => {}
                        Err(e) => {
                            bail!("Could not copy workflow files to the output folder. The error was: {}", e);
                        }
                    };
                }
                Ok(false) => {
                    // if doesn't exist, check if there are similar workflow names
                    // return with error and report similar workflow name if any.
                    let protocol_library_dir = fs::read_dir(
                        protocol_estuary.protocols_dir.as_path(),
                    )
                    .with_context(|| {
                        format!(
                            "Could not get protocol library in directory: {} ",
                            protocol_estuary.protocols_dir.display()
                        )
                    })?;
                    let mut similar_names: Vec<String> = Vec::new();
                    // iterate over protocol library folder
                    for p in protocol_library_dir {
                        let pp = p
                            .expect("Could not read directory protocol library directory")
                            .path();
                        let curr_workflow_name = pp
                            .file_name()
                            .expect("Could not get the directory name")
                            .to_str()
                            .expect("Could not convert dir name to str.");
                        // if finds similar file names, push to the vec
                        if curr_workflow_name.contains(name.as_str()) {
                            similar_names.push(curr_workflow_name.to_string());
                        }
                    }

                    // decide the final log info
                    let similar_name_hints = if similar_names.is_empty() {
                        String::from("")
                    } else {
                        similar_names.insert(
                            0,
                            String::from("Workflows with a similar name exist, which are"),
                        );
                        similar_names.join(", ")
                    };

                    // return with an error
                    bail!(
                        "Could not find a workflow with name: {}. {}",
                        name,
                        similar_name_hints
                    );
                }
                Err(e) => {
                    bail!(e)
                }
            }

            // write log
            let gwc_info_path = output_path.join("get_wokflow.json");
            let gwc_info = json!({
                "command" : "get-workflow-config",
                "workflow dir": output_path,

                "args" : {
                    "output" : output,
                    "name" : name,
                    // "essential_only" : essential_only,
                }
            });

            std::fs::write(
                &gwc_info_path,
                serde_json::to_string_pretty(&gwc_info).unwrap(),
            )
            .with_context(|| format!("could not write {}", gwc_info_path.display()))?;

            info!(
                "Successfully export {} workflow configuration files to {}",
                name,
                output_path.display()
            );
        }
        _ => {
            bail!("Unknown Command.")
        }
    }
    Ok(())
}

/// ## simpleaf run-workflow
/// #### Input
/// one or more simpleaf workflow JSON file (s) with all required fields
///
/// #### Output
/// the output of the simpleaf commands recorded in the input JSON file
///
/// #### Description
/// This program is used for running the commands recorded in the
/// user-provided simpleaf workflow JSON file(s).
/// Simpleaf Workflow JSON format required fields:
/// 1. json_type: This field has to exist and have the value "Simpleaf Workflow"
/// 2. simpleaf_version: This field has to exist and contains the version of simpleaf
///     used for making the file. If the files are made manually, this value has to be
///      higher than 0.11.0
/// 3. index: (Optional): this field records all simpleaf index commands that need to be run.
/// 4. quant: (Optional): this field records all simpleaf quant commands that need to be run.
// TODO: add a `skip` argument for skipping steps
pub fn run_workflow<T: AsRef<Path>>(
    af_home_path: T,
    workflow_cmd: WorkflowCommands,
) -> anyhow::Result<()> {
    match workflow_cmd {
        WorkflowCommands::Run {
            template,
            output,
            // TODO: write JSON only if no execution
            // actually, we want to write it regardless, right?
            no_execution,
            manifest,
            start_at,
            resume,
            jpaths,
            skip_step,
            ext_codes,
        } => {
            // designate that output is optional
            let output_opt = output;

            let instantiated_manifest: serde_json::Value;
            let source_path: std::path::PathBuf;
            let output_path: std::path::PathBuf;
            if let Some(manifest) = manifest {
                // If the user passed a fully-instantiated
                // manifest to execute
                info!("Loading and executing manifest.");
                // iterate json files and parse records to commands
                // convert files into json string vector
                instantiated_manifest = workflow_utils::parse_manifest(&manifest)?;
                output_path = workflow_utils::get_output_path(&instantiated_manifest)?;
                source_path = manifest.clone();
            } else if let Some(template) = template {
                //  check the validity of the file
                if !template.exists() || !template.is_file() {
                    bail!("the path of the given workflow template file doesn't exist; Cannot proceed.")
                }

                info!("Processing simpleaf template to produce and execute manifest.");

                // iterate json files and parse records to commands
                // convert files into json string vector
                let workflow_json_string = workflow_utils::instantiate_workflow_template(
                    af_home_path.as_ref(),
                    template.as_path(),
                    output_opt.clone(),
                    &jpaths,
                    &ext_codes,
                )?;

                // write complete workflow (i.e. the manifest) JSON to output folder
                instantiated_manifest = serde_json::from_str(workflow_json_string.as_str())?;
                output_path = workflow_utils::get_output_path(&instantiated_manifest)?;

                // check if the output path we read from the instantiated template matches
                // the output path requested by the user (if the user passed one in). If
                // they do not match, issue an obnoxious warning.
                // @DongzeHe : We should also probably log this warning to the output
                // log for subsequent inspection.
                if let Some(requested_output_path) = output_opt {
                    if requested_output_path != output_path {
                        warn!(
                            r#"The output path {} was requested via the command line, but 
                            the output path {} was resolved from the workflow template.
                            In this case, since the output variable is not used when instantiating 
                            the template, the value ({}) present in the template must be used.
                            Please be aware that {} will not be used for output!"#,
                            requested_output_path.display(),
                            output_path.display(),
                            output_path.display(),
                            requested_output_path.display()
                        );
                    }
                }

                source_path = template.clone();
            } else {
                bail!(concat!(
                    "You must have one of a manifest or template, ",
                    "but provided neither; this shouldn't happen"
                ));
            }

            // recursively make the output directory, which at this point
            // has been resolved as the one used in the template or manifest
            // (possibly as provided by the user in the former case).
            run_fun!(mkdir -p $output_path)?;

            // we need to convert the optional to a vector
            let final_skip_step = skip_step.unwrap_or(Vec::new());

            // initialize simpleaf workflow and log struct
            // TODO: print some log using meta_info fields
            let (simpleaf_workflow, mut workflow_log) = workflow_utils::initialize_workflow(
                af_home_path.as_ref(),
                source_path.as_path(),
                output_path.as_path(),
                instantiated_manifest,
                start_at,
                final_skip_step,
                resume,
            )?;

            if !no_execution {
                workflow_utils::execute_commands_in_workflow(
                    simpleaf_workflow,
                    af_home_path,
                    &mut workflow_log,
                )?;
                // write log
                workflow_log.write(true)?;
                info!("all commands ran successfully.");
            } else {
                workflow_log.write(false)?;
                info!("no execution mode ran successfully.");
            }
        } //
        _ => {
            warn!("encountered unknown command type!");
            bail!("unknown command type!");
        }
    } // match Commands::Workflow
    Ok(())
}
