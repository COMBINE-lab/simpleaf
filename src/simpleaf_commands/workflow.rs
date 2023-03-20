use crate::utils::prog_utils;
use crate::utils::prog_utils::ReqProgs;
use crate::utils::workflow_utils;
use crate::utils::workflow_utils::{SimpleafWorkflow, WorkflowLog};

use anyhow::{bail, Context};
use cmd_lib::{run_cmd, run_fun};
use serde_json::json;
use serde_json::Value;
use std::fs;
use std::path::Path;
use tracing::info;

use super::Commands;

/// ### Program Name
/// simpleaf generate-workflow
///
/// ### Program Input
/// A json file that records all top level variables needed by the template
///                  and optionally, some extra variables
/// ### Program Output
/// A json file that contains the actual simpelaf workflow information, which can be
///         consumed directly by the simpleaf run-workflow command. Additionally, if --execute is specified,
///          the generated simpleaf workflow will be executed.
/// ### Program Description
/// This program is used for generating a simpleaf workflow JSON file
/// that can be consumed directly by the `simpleaf workflow` program.\
/// This program takes a template from the template library as the input
/// and does the following:
/// 1. It loads the required arguments of that template and
///      find them in the user-provided JSON file.
/// 2. It validates the files in the user-provided JSON file.
///      This can be checking the existance and validate the first few records
/// 3. It feeds the template the required inputs, and
///      generates a simpleaf workflow JSON file.
///      This JSON file contains the simpleaf programs need to be run and
///      the required arguments.

// TODO:
// 1. figure out the layout of protocol estuary
// 2. find workflow using name, if doesn't exist, find similar names and return error
// 3. copy the config file from af_home protocol estuary dir to the output dir.
// 4. allow name change?

pub fn get_workflow_config(af_home_path: &Path, gw_cmd: Commands) -> anyhow::Result<()> {
    match gw_cmd {
        Commands::GetWorkflowConfig {
            output,
            workflow,
            // essential_only: _,
        } => {
            // get af_home
            let v: Value = prog_utils::inspect_af_home(af_home_path)?;
            // Read the JSON contents of the file as an instance of `User`.
            let rp: ReqProgs = serde_json::from_value(v["prog_info"].clone())?;

            // get protocol library path
            let protocol_estuary = workflow_utils::get_protocol_estuary(af_home_path)?;
            // get the corresponding workflow directory path
            let workflow_path = protocol_estuary.protocols_dir.join(workflow.as_str());
            // make output dir
            let mut output_dir_name = workflow.clone();
            output_dir_name.push_str("_config");
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
                        if curr_workflow_name.contains(workflow.as_str()) {
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
                        workflow,
                        similar_name_hints
                    );
                }
                Err(e) => {
                    bail!(e)
                }
            }

            // write log
            let gwc_info_path = output_path.join("get_workflow_config.json");
            let gwc_info = json!({
                "command" : "get-workflow-config",
                "version_info" : rp,
                "workflow dir": output_path,

                "args" : {
                    "output" : output,
                    "workflow" : workflow,
                    // "essential_only" : essential_only,
                }
            });

            std::fs::write(
                &gwc_info_path,
                serde_json::to_string_pretty(&gwc_info).unwrap(),
            )
            .with_context(|| format!("could not write {}", gwc_info_path.display()))?;

            info!(
                "Successfully export {} workflow to {}",
                workflow,
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
pub fn workflow(af_home_path: &Path, workflow_cmd: Commands) -> anyhow::Result<()> {
    match workflow_cmd {
        Commands::Workflow {
            config_path,
            workflow_path,
            output,
            // TODO: write JSON only if no execution
            no_execution,
            start_at,
            resume,
            lib_paths,
            skip_step,
        } => {
            run_fun!(mkdir -p $output)?;

            let simpleaf_workflow: SimpleafWorkflow;
            let mut workflow_log: WorkflowLog;

            let final_start_at = if resume {
                workflow_utils::update_start_at(output.as_path())?
            } else {
                start_at
            };

            let final_skip_step = if let Some(ss) = skip_step {
                ss
            } else {
                Vec::new()
            };

            // we will have either a config_path or a workflow_path
            // if we see config files. process it
            if let Some(cp) = config_path {
                //  check the validity of the file
                if !cp.exists() {
                    bail!("the path of the given workflow configuratioin file doesn't exist; Cannot proceed.")
                }

                info!("Processing simpleaf workflow configuration file.");

                // iterate json files and parse records to commands
                // convert files into json string vector
                let workflow_json_string = workflow_utils::parse_workflow_config(
                    af_home_path,
                    cp.as_path(),
                    output.as_path(),
                    &lib_paths,
                )?;

                // write complete workflow json to output folder
                // the `Step` of each command in this json file will be changed to "-1"
                // once the command is run successfully.
                // The final workflow file name will be the same as the input config but
                // with json as the extention.
                let workflow_json_value: Value =
                    serde_json::from_str(workflow_json_string.as_str())?;

                // initialize simpleaf workflow and log struct
                // TODO: print some log using meta_info fields
                (simpleaf_workflow, workflow_log) = workflow_utils::initialize_workflow(
                    af_home_path,
                    cp.as_path(),
                    output.as_path(),
                    workflow_json_value,
                    final_start_at,
                    final_skip_step,
                )?;
            } else {
                // This file has to exist
                let wp = workflow_path.expect(
                    "Neither configuration file nor workflow file is provided; Cannot proceed.",
                );

                // check the existence of the file
                if !wp.exists() {
                    bail!("the path of the given workflow configuratioin file doesn't exist; Cannot proceed.")
                }
                // load each file as a wrapper struct of a vector of simpleaf commands
                let json_file = fs::File::open(wp.as_path())
                    .with_context(|| format!("Could not open JSON file {}.", wp.display()))?;

                // TODO: print some log using meta_info fields
                let workflow_json_value: Value = serde_json::from_reader(json_file)?;

                (simpleaf_workflow, workflow_log) = workflow_utils::initialize_workflow(
                    af_home_path,
                    wp.as_path(),
                    output.as_path(),
                    workflow_json_value,
                    final_start_at,
                    final_skip_step,
                )?;
            }
            if !no_execution {
                for cr in simpleaf_workflow.cmd_queue {
                    let pn = cr.program_name;
                    let step = cr.step;
                    // this if statement is no longer needed as commands with a negative exec order
                    // are ignore when constructing the the cmd queue
                    // say something
                    info!("Running {} command with step {}.", pn, step,);

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
                            info!("Execution terminated at {} command with step {}", pn, step);
                            return Err(e);
                        } else {
                            info!("Successfully ran {} command with step {}", pn, step);

                            workflow_log.update(&cr.field_trajectory_vec[..]);
                        }
                    }

                    // If this is an external command, then initialize it and run
                    if let Some(mut cmd) = cr.external_cmd {
                        // log
                        let cmd_string = prog_utils::get_cmd_line_string(&cmd);
                        info!("Invoking command : {}", cmd_string);

                        // initiate a stopwatch
                        workflow_log.timeit(cr.step);

                        match cmd.output() {
                            Ok(cres) => {
                                // check the return status of external command
                                if cres.status.success() {
                                    // succeed. update log
                                    workflow_log.update(&cr.field_trajectory_vec[..]);
                                } else {
                                    let cmd_string = prog_utils::get_cmd_line_string(&cmd);
                                    match run_cmd!(sh -c $cmd_string) {
                                        Ok(_) => {
                                            // succeed. update log
                                            workflow_log.update(&cr.field_trajectory_vec[..]);
                                        }
                                        Err(e2) => {
                                            workflow_log.write(false)?;
                                            bail!(
                                                "{} with step {} failed in two different attempts.\n\
                                                The exit status of the first attempt was: {:?}. \n\
                                                The stderr of the first attempt was: {:?}. \n\
                                                The error message of the second attempt was: {:?}.",
                                                pn, step,
                                                cres.status,
                                                std::str::from_utf8(&cres.stderr[..]).unwrap(),
                                                e2
                                            );
                                        }
                                    };
                                }
                            }
                            Err(e) => {
                                let cmd_string = prog_utils::get_cmd_line_string(&cmd);
                                match run_cmd!(sh -c $cmd_string) {
                                    Ok(_) => {
                                        workflow_log.update(&cr.field_trajectory_vec[..]);
                                    }
                                    Err(e2) => {
                                        workflow_log.write(false)?;
                                        bail!(
                                            "{} command with step {} failed in two different attempts.\n\
                                            The stderr of the first attempt was: {:?}. \n\
                                            The error message of the second attempt was: {:?}.",
                                            pn, step,
                                            e,
                                            e2
                                        );
                                    }
                                };
                            } // TODO: use this in the log somewhere.
                        } // invoke external cmd

                        info!("Successfully ran {} command with step {}.", pn, step);
                    } // for cmd_queue
                }
                // write log
                workflow_log.write(true)?;

                info!("All commands ran successfully.");
            } else {
                workflow_log.write(false)?;
            }
        } //
        _ => {
            bail!("unknown command")
        }
    } // match Commands::Workflow
    Ok(())
}
