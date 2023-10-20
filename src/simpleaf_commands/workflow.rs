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

pub fn patch_manifest_or_template<T: AsRef<Path>>(
    af_home_path: T,
    workflow_cmd: WorkflowCommands) -> anyhow::Result<()> {

    match workflow_cmd {
        WorkflowCommands::Patch{
            manifest, template, patch
        } => {
            if let Some(manifest_value) = manifest {
                todo!();
            } else if let Some(template_value) = template {
                let patches = workflow_utils::template_patches_from_csv(patch);
            }
        },
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
            no_execution,
            start_at,
            resume,
            jpaths,
            skip_step,
            ext_codes,
        } => {
            // recursively make the output directory
            run_fun!(mkdir -p $output)?;

            // we need to convert the optional to a vector
            let final_skip_step = skip_step.unwrap_or(Vec::new());

            //  check the validity of the file
            if !template.exists() || !template.is_file() {
                bail!("the path of the given workflow configuratioin file doesn't exist; Cannot proceed.")
            }

            info!("Processing simpleaf workflow configuration file.");

            // iterate json files and parse records to commands
            // convert files into json string vector
            let workflow_json_string = workflow_utils::parse_workflow_config(
                af_home_path.as_ref(),
                template.as_path(),
                output.as_path(),
                &jpaths,
                &ext_codes,
            )?;

            // write complete workflow (i.e. the manifest) JSON to output folder
            let workflow_json_value: Value = serde_json::from_str(workflow_json_string.as_str())?;

            // initialize simpleaf workflow and log struct
            // TODO: print some log using meta_info fields
            let (simpleaf_workflow, mut workflow_log) = workflow_utils::initialize_workflow(
                af_home_path.as_ref(),
                template.as_path(),
                output.as_path(),
                workflow_json_value,
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
