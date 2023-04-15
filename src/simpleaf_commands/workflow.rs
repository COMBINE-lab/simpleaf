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

use super::Commands;
use super::WorkflowCommands;

#[derive(Tabled)]
struct WorkflowTemplate {
    registry: String,
    name: String,
    version: String,
}

pub fn refresh_protocol_estuary(af_home_path: &Path) -> anyhow::Result<()> {
    workflow_utils::get_protocol_estuary(af_home_path, true)?;
    Ok(())
}

pub fn list_workflows(af_home_path: &Path) -> anyhow::Result<()> {
    // get af_home
    let v: Value = prog_utils::inspect_af_home(af_home_path)?;
    // Read the JSON contents of the file as an instance of `User`.
    // TODO: use it somehwere?
    let _rp: ReqProgs = serde_json::from_value(v["prog_info"].clone())?;

    // get protocol library path
    let protocol_estuary = workflow_utils::get_protocol_estuary(af_home_path, false)?;
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
        println!("* : could not parse uninstantiated template to attempt extracting the version, please see [url about parsing uninstantiated templates in our tutorial] for further details");
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

pub fn get_wokflow(af_home_path: &Path, gw_cmd: WorkflowCommands) -> anyhow::Result<()> {
    match gw_cmd {
        WorkflowCommands::Get {
            output,
            name,
            // essential_only: _,
        } => {
            // get af_home
            let v: Value = prog_utils::inspect_af_home(af_home_path)?;
            // Read the JSON contents of the file as an instance of `User`.
            // TODO: use it somehwere?
            let _rp: ReqProgs = serde_json::from_value(v["prog_info"].clone())?;

            // get protocol library path
            let protocol_estuary = workflow_utils::get_protocol_estuary(af_home_path, false)?;
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
pub fn run_workflow(af_home_path: &Path, workflow_cmd: WorkflowCommands) -> anyhow::Result<()> {
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
            run_fun!(mkdir -p $output)?;

            // we need to convert the optional to a vector
            let final_skip_step = if let Some(ss) = skip_step {
                ss
            } else {
                Vec::new()
            };

            //  check the validity of the file
            if !template.exists() {
                bail!("the path of the given workflow configuratioin file doesn't exist; Cannot proceed.")
            }

            info!("Processing simpleaf workflow configuration file.");

            // iterate json files and parse records to commands
            // convert files into json string vector
            let workflow_json_string = workflow_utils::parse_workflow_config(
                af_home_path,
                template.as_path(),
                output.as_path(),
                &jpaths,
                &ext_codes,
            )?;

            // write complete workflow json to output folder
            // the `Step` of each command in this json file will be changed to "-1"
            // once the command is run successfully.
            // The final workflow file name will be the same as the input config but
            // with json as the extention.
            let workflow_json_value: Value = serde_json::from_str(workflow_json_string.as_str())?;

            // initialize simpleaf workflow and log struct
            // TODO: print some log using meta_info fields
            let (simpleaf_workflow, mut workflow_log) = workflow_utils::initialize_workflow(
                af_home_path,
                template.as_path(),
                output.as_path(),
                workflow_json_value,
                start_at,
                final_skip_step,
                resume,
            )?;

            if !no_execution {
                for cr in simpleaf_workflow.cmd_queue {
                    let pn = cr.program_name;
                    let step = cr.step;
                    // this if statement is no longer needed as commands with a negative exec order
                    // are ignore when constructing the the cmd queue
                    // say something
                    info!("Running {} command for step {}.", pn, step,);

                    // initiliaze a stopwatch
                    workflow_log.timeit(step);

                    if let Some(cmd) = cr.simpleaf_cmd {
                        let exec_result = match cmd {
                            Commands::Index {
                                ref_type,
                                fasta,
                                gtf,
                                rlen,
                                spliced,
                                unspliced,
                                dedup,
                                keep_duplicates,
                                ref_seq,
                                output,
                                use_piscem,
                                kmer_length,
                                minimizer_length,
                                overwrite,
                                sparse,
                                threads,
                            } => super::indexing::build_ref_and_index(
                                af_home_path,
                                Commands::Index {
                                    ref_type,
                                    fasta,
                                    gtf,
                                    rlen,
                                    spliced,
                                    unspliced,
                                    dedup,
                                    keep_duplicates,
                                    ref_seq,
                                    output,
                                    use_piscem,
                                    kmer_length,
                                    minimizer_length,
                                    overwrite,
                                    sparse,
                                    threads,
                                },
                            ),

                            // if we are running mapping and quantification
                            Commands::Quant {
                                index,
                                use_piscem,
                                map_dir,
                                reads1,
                                reads2,
                                threads,
                                use_selective_alignment,
                                expected_ori,
                                knee,
                                unfiltered_pl,
                                explicit_pl,
                                forced_cells,
                                expect_cells,
                                min_reads,
                                resolution,
                                t2g_map,
                                chemistry,
                                output,
                            } => super::quant::map_and_quant(
                                af_home_path,
                                Commands::Quant {
                                    index,
                                    use_piscem,
                                    map_dir,
                                    reads1,
                                    reads2,
                                    threads,
                                    use_selective_alignment,
                                    expected_ori,
                                    knee,
                                    unfiltered_pl,
                                    explicit_pl,
                                    forced_cells,
                                    expect_cells,
                                    min_reads,
                                    resolution,
                                    t2g_map,
                                    chemistry,
                                    output,
                                },
                            ),
                            _ => todo!(),
                        };
                        if let Err(e) = exec_result {
                            workflow_log.write(false)?;
                            info!("Execution terminated at {} command for step {}", pn, step);
                            return Err(e);
                        } else {
                            info!("Successfully ran {} command for step {}", pn, step);

                            workflow_log.update(&cr.field_trajectory_vec[..])?;
                        }
                    }

                    // If this is an external command, then initialize it and run
                    if let Some(mut ext_cmd) = cr.external_cmd {
                        // log
                        let cmd_string = prog_utils::get_cmd_line_string(&ext_cmd);
                        info!("Invoking command : {}", cmd_string);

                        // initiate a stopwatch
                        workflow_log.timeit(cr.step);

                        match ext_cmd.output() {
                            Ok(cres) => {
                                // check the return status of external command
                                if cres.status.success() {
                                    // succeed. update log
                                    workflow_log.update(&cr.field_trajectory_vec[..])?;
                                } else {
                                    let cmd_stderr = std::str::from_utf8(&cres.stderr[..])?;
                                    let msg = format!("{} command at step {} failed to exit with code 0 under the shell.\n\
                                                      The exit status was: {}.\n\
                                                      The stderr of the invocation was: {}.", pn, step, cres.status, cmd_stderr);
                                    warn!(msg);
                                    bail!(msg);
                                }
                            }
                            Err(e) => {
                                let msg = format!(
                                    "{} command at step {} failed to execute under the shell.\n\
                                     The returned error was: {:?}.\n",
                                    pn, step, e
                                );
                                warn!(msg);
                                bail!(msg);
                            } // TODO: use this in the log somewhere.
                        } // invoke external cmd

                        info!("successfully ran {} command for step {}.", pn, step);
                    } // for cmd_queue
                }
                // write log
                workflow_log.write(true)?;

                info!("all commands ran successfully.");
            } else {
                workflow_log.write(false)?;
            }
        } //
        _ => {
            warn!("encountered unknown command type!");
            bail!("unknown command type!");
        }
    } // match Commands::Workflow
    Ok(())
}
