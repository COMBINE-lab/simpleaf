// TODO:
// allow multiple registry, just like conda envs
// find a way to pull files from github directly instead of using local copy of protocol estuary

use anyhow::{anyhow, bail, Context};
use chrono::{DateTime, Local};
use clap::Parser;
use cmd_lib::log::info;
use cmd_lib::run_cmd;
use serde_json::{json, Map, Value};
use std::fs;
use std::io::BufReader;
use std::path::{Path, PathBuf};
use std::process::Command;
use tracing::warn;

use crate::utils::jrsonnet_main::parse_jsonnet;
use crate::utils::prog_utils;
use crate::utils::prog_utils::CommandVerbosityLevel;
use crate::{Cli, Commands};

use super::prog_utils::shell;

// fields that are not representing any simpleaf flag
const SKIPARG: &[&str] = &["Step", "Program Name", "Active"];

// This function gets the version string from the workflwo template file in the provided folder
pub fn get_template_version<T: AsRef<Path>>(template_dir: PathBuf, utils_dir: T) -> anyhow::Result<String> {
    // we first get the workflow name
    let workflow_name = template_dir
        .file_name()
        .with_context(|| format!("Cannot get folder name from {:?}", template_dir))?;
    // Then we get the expected template file path
    let mut template_path = template_dir.join(workflow_name);
    if !template_path.set_extension("jsonnet") {
        bail!(
            "Cannot set extention for workflow template file in {:?}",
            template_dir
        )
    };

    // Then we call Jrsonnet to get JSON string
    let workflow_json_string = match parse_jsonnet(
        &template_path,
        PathBuf::from(".").as_path(),
        utils_dir.as_ref(),
        &None,
        &None,
        false,
    ) {
        Ok(v) => v,
        Err(_) => return Ok(String::from("N/A*")),
    };

    let workflow_json_value: Value = serde_json::from_str(workflow_json_string.as_str())?;

    let v = if let Some(meta_info) = workflow_json_value.get("meta_info") {
        if let Some(version_value) = meta_info.get("template_version") {
            if let Some(v) = version_value.as_str() {
                v.to_string()
            } else {
                String::from("missing")
            }
        } else {
            String::from("missing")
        }
    } else {
        String::from("missing")
    };

    Ok(v)
}

pub fn duration_to_dhms(d: chrono::Duration) -> String {
    let execution_elapsed_time = format!(
        "{}d{}h{}m{}.{:03}s",
        d.num_days(),
        (d - chrono::Duration::days(d.num_days())).num_hours(),
        (d - chrono::Duration::hours(d.num_hours())).num_minutes(),
        (d - chrono::Duration::minutes(d.num_minutes())).num_seconds(),
        (d - chrono::Duration::seconds(d.num_seconds())).num_milliseconds(),
    );
    execution_elapsed_time
}

/// This function updates the start_at variable
/// if --resume is provided.\
/// It finds the workflow_info.json exported by
/// simpleaf workflow from the previous run and
/// grab the "Succeed" and "Execution Terminated Step"
/// fields.\
/// If the previous run was succeed, then we report an error
/// saying nothing to resume
/// If Execution Terminated Step is a negative number, that
/// means there was no previous execution:
pub fn update_start_at(v: &Value) -> anyhow::Result<u64> {
    let latest_run = v.get("Latest Run").with_context(|| {
        "Could not get the `Latest Run` field from the `simpleaf_workflow_log.json`; Cannot proceed"
    })?;
    // Check if the previous run was succeed. If yes, then no need to resume
    let succeed = v
        .get("Succeed")
        .with_context(|| {
            "Could not get `Execution Terminated Step` from the log file; Cannot resume."
        })?
        .as_bool()
        .with_context(|| "cannot parse `Succeed` as bool; Cannot resume.")?;

    let start_at = latest_run
        .get("Execution Terminated Step")
        .with_context(|| {
            "Could not get `Execution Terminated Step` from the log file; Cannot resume."
        })?
        .as_u64()
        .with_context(|| "cannot parse `Execution Terminated Step` as str; Cannot resume.")?;

    if succeed {
        bail!("The previous run succeed. Cannot resume.");
    } else {
        Ok(start_at)
    }
}

pub fn get_previous_log<T: AsRef<Path>>(output: T) -> anyhow::Result<Value> {
    // the path to the expected log file
    let exec_log_path = output.as_ref().join("simpleaf_workflow_log.json");
    match exec_log_path.try_exists() {
        Ok(true) => {
            // we have the workflow_info.json file, so parse it.
            let exec_log_file = std::fs::File::open(&exec_log_path).with_context({
                || {
                    format!(
                        "Could not open file {}; Cannot resume.",
                        exec_log_path.display()
                    )
                }
            })?;

            // We read the file and return the value in a Some()
            let exec_log_reader = BufReader::new(&exec_log_file);
            let v: Value = serde_json::from_reader(exec_log_reader)?;
            Ok(v)
        }
        Ok(false) => {
            bail!(
                    "Could not find `simpleaf_workflow_log.json` in the output directory {:?}; Cannot resume.",
                    output.as_ref()
                )
        }
        Err(e) => {
            bail!(e)
        }
    }
}

/// intialize simpleaf workflow realted structs:
/// SimpleafWorkflow and WorkfowLog
pub fn initialize_workflow <T: AsRef<Path>> (
    af_home_path: T,
    template: T,
    output: T,
    workflow_json_value: Value,
    start_at: u64,
    skip_step: Vec<u64>,
    resume: bool,
) -> anyhow::Result<(SimpleafWorkflow, WorkflowLog)> {
    // Instantiate a workflow log struct
    let mut wl = WorkflowLog::new(
        output.as_ref(),
        template.as_ref(),
        &workflow_json_value,
        start_at,
        skip_step,
        resume,
    )?;

    // instantiate a simpleaf workflow struct, and complete the workflow struct
    let sw = SimpleafWorkflow::new(af_home_path.as_ref(), &workflow_json_value, &mut wl)?;

    Ok((sw, wl))
}

// Each SimpleafWorkflow represents a json
/// Simpleaf Workflow record
pub struct SimpleafWorkflow {
    pub af_home_path: PathBuf,

    // This command queue contains all commands that need to be run
    pub cmd_queue: Vec<CommandRecord>,
}

impl SimpleafWorkflow {
    /// Initialize a SimpleafWorkflow object.
    /// It needs an empty and mutable `WorkflowLog` as a complementary part.
    pub fn new<T: AsRef<Path>>(
        af_home_path: T,
        workflow_json_value: &Value,
        workflow_log: &mut WorkflowLog,
    ) -> anyhow::Result<SimpleafWorkflow> {
        // we recursively get all simpleaf command and put them into a queue
        // currently it is a recursive function.
        let mut cmd_queue: Vec<CommandRecord> = Vec::new();

        // This is a running vec as fill_cmd_queue is a recursive function
        let field_trajectory_vec: Vec<usize> = Vec::new();

        // find and parse simpleaf and external commands recorded in the workflow JSON object.
        SimpleafWorkflow::fill_cmd_queue(
            workflow_json_value,
            &mut cmd_queue,
            field_trajectory_vec,
            workflow_log,
        )?;

        // sort the cmd queue by its `Step`.
        cmd_queue.sort_by(|cmd1, cmd2| cmd1.step.cmp(&cmd2.step));

        Ok(SimpleafWorkflow {
            af_home_path: af_home_path.as_ref().to_owned(),
            cmd_queue,
        })
    }

    /// This function collect the command records from a `serde_json::Value` that records a complete simpleaf workflow,
    /// parse them as `CommandRecord` structs and push them into the `cmd_queue` vector.
    /// ### Details
    /// This function will iterate over all layers in the `Value` object to find the command records
    /// with both `Step` and `Program Name`. **These two fields must appear simutaneously**, otherwise this function
    /// will return an error.  
    /// A CommandRecord struct will be initilizaed from Each command record with a positive `Step`,
    /// including external commands and simpleaf command,
    /// and will be pushed into the `cmd_queue` vector.
    /// At the same time, a `WorkflowLog` struct will be completed for logging purpose.
    fn fill_cmd_queue(
        workflow_json_value: &Value,
        cmd_queue: &mut Vec<CommandRecord>,
        field_trajectory_vec: Vec<usize>,
        workflow_log: &mut WorkflowLog,
    ) -> anyhow::Result<()> {
        // save some allocation
        let mut pn: ProgramName;
        if let Value::Object(value_inner) = workflow_json_value {
            // As we don't know how many layers the json has, we will recursively call this function to get a vector of vector
            for (field_name, field) in value_inner {
                // clone the vec and push the current field name
                let mut curr_field_trajectory_vec = field_trajectory_vec.clone();

                curr_field_trajectory_vec.push(workflow_log.get_field_id(field_name));

                // If "Step" exists, then this field records an external or a simpleaf command
                if field.get("Step").is_some() {
                    // parse "Step"
                    let step = field
                        .get("Step")
                        .with_context(|| "Cannot get Step")?
                        .as_u64()
                        .with_context(|| {
                            format!(
                                "Cannot parse Step {:?} as an integer",
                                field.get("Step").unwrap()
                            )
                        })?;

                    // parse "Active" if there is one
                    let active =
                        if workflow_log.skip_step.contains(&step) || step < workflow_log.start_at {
                            false
                        } else if let Some(v) = field.get("Active") {
                            v.as_bool().with_context(|| {
                                format!(
                                    "Cannot parse Active {:?} as a boolean",
                                    field.get("Active").unwrap()
                                )
                            })?
                        } else {
                            true
                        };

                    // update Active in the log
                    let cmd_field = workflow_log.get_mut_cmd_field(&curr_field_trajectory_vec)?;
                    cmd_field["Active"] = json!(active);

                    // The field must contains an Program Name
                    if let Some(program_name) = field.get("Program Name") {
                        pn = ProgramName::from_str(program_name.as_str().with_context(|| {
                            "Cannot create ProgramName struct from a program name"
                        })?);
                        // if active, then push to execution queue
                        if active {
                            info!("Parsing {} command for Step {}", pn, step);
                            // The `Step` will be used for sorting the cmd_queue vector.
                            // All commands must have an valid `Step`.
                            // Note that we store this as a string in json b/c all value in config
                            // file are strings.
                            if pn.is_external() {
                                // creating an external command records using the args recorded in the field
                                let external_cmd = match pn.create_external_cmd(field) {
                                    Ok(v) => v,
                                    Err(e) => {
                                        bail!("Could not parse external command {} for Step {}. The error message was: {}", pn, step, e);
                                    }
                                };

                                cmd_queue.push(CommandRecord {
                                    step,
                                    active,
                                    program_name: pn,
                                    simpleaf_cmd: None,
                                    external_cmd: Some(external_cmd),
                                    field_trajectory_vec: curr_field_trajectory_vec,
                                });
                            } else {
                                // create a simpleaf command record using the args recorded in the field
                                let simpleaf_cmd = match pn.create_simpleaf_cmd(field) {
                                    Ok(v) => v,
                                    Err(e) => {
                                        bail!("Could not parse simpleaf command {} for Step {}. The error message was: {}", pn, step, e);
                                    }
                                };

                                cmd_queue.push(CommandRecord {
                                    step,
                                    active,
                                    program_name: pn,
                                    simpleaf_cmd: Some(simpleaf_cmd),
                                    external_cmd: None,
                                    field_trajectory_vec: curr_field_trajectory_vec,
                                });
                            }
                        } else {
                            info!("Skipping {} command for Step {}", pn, step);
                        } // if active
                    } // if have ProgramName
                } else {
                    // If this is not a command record, we move to the next level
                    SimpleafWorkflow::fill_cmd_queue(
                        field,
                        cmd_queue,
                        curr_field_trajectory_vec,
                        workflow_log,
                    )?;
                }
            }
        }
        Ok(())
    }
}

#[derive(Copy, Clone)]
struct CommandRuntime {
    start_time: DateTime<Local>,
    step: u64,
}

/// This struct is used for writing the workflow log JSON file.
/// ### What?
/// This struct contains the parsed and complete workflow JSON record.\
/// The `Step` field of the successfully invoked commands will be set as a negatvie integer
/// and will be ignored by simpleaf if feeding the output JSON file to simpleaf workflow.\
/// ### Why?
/// The purpose of having this log is that if some command in the workflow fails, the user can
/// fix that command using this log file, and feed resume the execution of the workflow from the failed step.
/// ### How?
/// It will be initialized together with the `SimpleafWorkflow` struct and
/// will be used to write a workflow JSON file that is the same as the one
/// interpreted from user-provided JSONNET file except the Step
/// field of the commands that were run sucessfully are negative values.
pub struct WorkflowLog {
    // meta info log file path
    meta_info_path: PathBuf,
    // execution log
    exec_log_path: PathBuf,
    workflow_start_time: DateTime<Local>,
    command_runtime: Option<CommandRuntime>,
    num_succ: usize,
    start_at: u64,
    skip_step: Vec<u64>,
    workflow_name: String, // doesn't matter, can convert to string
    workflow_meta_info: Option<Value>,
    // The value records the complete simpleaf workflow
    value: Value,

    cmd_runtime_records: Map<String, Value>,
    // this vector records all field names in the complete workflow.
    // This is used for locating the Step for each command
    field_id_to_name: Vec<String>,
    // TODO: the trajectory vector in each CommandRecord can be
    // move here as a long vector, and in each CommandRecord
    // we just need to record the start pos and the length of its trajectory.
    // cmds_field_id_trajectory: Vec<usize>,

    // this is used for updating the log file <simpleaf_workflow_log.json>
    // this field will be updated after the
    previous_log: Option<Value>,
}

impl WorkflowLog {
    /// This function instantiate a workflow log
    /// with a valid output path and complete workflow as a `serde_json::Value` object
    pub fn new<T: AsRef<Path>>(
        output: T,
        template: T,
        workflow_json_value: &Value,
        // start_at will be updated if setting --resume
        mut start_at: u64,
        skip_step: Vec<u64>,
        resume: bool,
    ) -> anyhow::Result<WorkflowLog> {
        // We want to update the log file instead of overwrite it if --resume,
        // So we need to know if we have previous log
        // This will be none if --resume is not set
        let previous_log = if resume {
            let v = get_previous_log(output.as_ref())?;
            Some(v)
        // if not --resume, then just give it a None
        } else {
            None
        };

        // If previous log is Some(), i.e., --resume is set and we can find the file
        // then update start at using the Terminated At field
        if let Some(v) = &previous_log {
            start_at = update_start_at(v)?;
        }

        // get output json path
        let workflow_name = template.as_ref()
            .file_stem()
            .unwrap_or_else(|| panic!("Cannot parse file name of file {}", template.as_ref().display()))
            .to_string_lossy()
            .into_owned();

        // get meta_info
        let workflow_meta_info = workflow_json_value.get("meta_info").map(|v| v.to_owned());

        // if we don't see an meta info section, report a warning
        if workflow_meta_info.is_none() {
            warn!("Found config file without meta_info field.");
        };

        Ok(WorkflowLog {
            meta_info_path: output.as_ref().join("simpleaf_workflow_log.json"),
            exec_log_path: output.as_ref().join("workflow_execution_log.json"),
            workflow_name,
            workflow_meta_info,
            workflow_start_time: Local::now(),
            command_runtime: None,
            num_succ: 0,
            start_at,
            skip_step,
            value: workflow_json_value.clone(),
            cmd_runtime_records: Map::new(),
            field_id_to_name: Vec::new(),
            // cmds_field_id_trajectory: Vec::new()
            previous_log,
        })
    }

    pub fn timeit(&mut self, step: u64) {
        self.command_runtime = Some(CommandRuntime {
            start_time: Local::now(),
            step,
        });
    }

    /// Write log to the output path.
    /// 1. an execution log file includes the whole workflow,
    ///    in which succeffully invoked commands have
    ///     a negative `Step`
    /// 2. an info log file records runtime, workflow name,
    ///     output path etc.
    pub fn write(&self, succeed: bool) -> anyhow::Result<()> {
        // initiate meta_info
        let workflow_meta_info = if let Some(workflow_meta_info) = &self.workflow_meta_info {
            workflow_meta_info.to_owned()
        } else {
            json!("{}")
        };

        // will be NA if used --no-execution
        let execution_terminated_at = if let Some(command_runtime) = &self.command_runtime {
            command_runtime.step
        } else {
            // If no record, then terminated at the beginning
            1u64
        };

        // if this is a --resume, we need to load the log from last run
        // Otherwise, we create an empty Value
        let previous_runs = if let Some(v) = &self.previous_log {
            // get the latest run from the log
            let latest_run = v.get("Latest Run").with_context(|| "Could not get the `Latest Run` field from the `simpleaf_workflow_log.json`; Cannot proceed")?;

            // get the time stamp. This will be used as the field name
            let latest_run_time_stamp = latest_run
                .get("Execution Start Local Time")
                .with_context(|| "Could not get the `Execution Start Local Time` information from the `simpleaf_workflow_log.json`; Cannot proceed")?
                .as_str()
                .with_context(|| "Could not convert the `Execution Start Local Time` from the `simpleaf_workflow_log.json` to str; Cannot proceed")?;

            // get previous runs
            let mut pr = v
                .get("Previous Runs")
                .with_context(|| "Could not get the `Previous Runs` field from the `simpleaf_workflow_log.json`; Cannot proceed")?
                .to_owned();

            // push the latest run in the log into previous run, as we will update it
            pr[latest_run_time_stamp] = latest_run.to_owned();
            pr
        } else {
            json!({})
        };

        // This might be the most straightforward elapsed time log in the history ;P
        let d = Local::now().signed_duration_since(self.workflow_start_time);
        let execution_elapsed_time = duration_to_dhms(d);

        let meta_info = json!(
            {
                "Workflow Name": self.workflow_name,
                "Workflow Meta Info":  workflow_meta_info,
                "Succeed": succeed,
                "Latest Run": {
                    "Execution Start Local Time": self.workflow_start_time.format("%Y-%m-%d %H:%M:%S").to_string(),
                    "Execution Elapsed Time": execution_elapsed_time,
                    "Execution Start Step": self.start_at,
                    "Skip Step": self.skip_step,
                    "Execution Terminated Step":  execution_terminated_at,
                    "Number of Succeed Commands": self.num_succ,
                    "Command Runtime by Step": Value::from(self.cmd_runtime_records.clone()),
                },
                "Previous Runs": previous_runs
        });

        // execution log
        std::fs::write(
            self.meta_info_path.as_path(),
            serde_json::to_string_pretty(&meta_info)
                .with_context(|| ("Cannot convert json value to string."))?,
        )
        .with_context(|| {
            format!(
                "could not write workflow meta info JSON file to {}",
                self.meta_info_path.display()
            )
        })?;

        // execution log
        std::fs::write(
            self.exec_log_path.as_path(),
            serde_json::to_string_pretty(&self.value)
                .with_context(|| "Could not convert json value to string.")?,
        )
        .with_context(|| {
            format!(
                "could not write complete simpleaf workflow JSON file to {}",
                self.exec_log_path.display()
            )
        })?;

        Ok(())
    }

    /// Get the index corresponds to the field name in the field_id_to_name vector.
    pub fn get_field_id(&mut self, field_name: &String) -> usize {
        if let Ok(pos) = self.field_id_to_name.binary_search(field_name) {
            pos
        } else {
            self.field_id_to_name.push(field_name.to_owned());
            self.field_id_to_name.len() - 1
        }
    }

    /// This function is used for testing if the exection order of
    /// successfully invoked command can be updated to a negative value
    pub fn get_mut_cmd_field(
        &mut self,
        field_trajectory_vec: &[usize],
    ) -> anyhow::Result<&mut Value> {
        // get iterator of field_trajectory vector
        let field_trajectory_vec_iter = field_trajectory_vec.iter();

        // convert id to name
        let mut curr_field_name: &String;

        // get mutable reference of current field
        let mut curr_field = &mut self.value;

        for curr_field_id in field_trajectory_vec_iter {
            curr_field_name = self
                .field_id_to_name
                .get(*curr_field_id)
                .expect("Cannot map field ID to name.");
            // let curr_pos = field_trajectory_vec.first().expect("Cannot get the first element");
            curr_field = curr_field
                .get_mut(curr_field_name)
                .with_context(|| "Cannot get field from json value")?;
        }

        Ok(curr_field)
    }

    pub fn get_elapsed_time(&self) -> anyhow::Result<String> {
        // update cmd run time
        if let Some(command_runtime) = &self.command_runtime {
            let d = Local::now().signed_duration_since(command_runtime.start_time);
            let cmd_duration = duration_to_dhms(d);
            Ok(cmd_duration)
        } else {
            bail!(
                "Execution Start Local Time is not set. Could not get elapsed time; Cannot proceed"
            );
        }
    }

    /// Update WorkflowLog:
    /// 1. the `Active` field of the executed commands in execution log
    /// 2. cmd runtime
    /// 3. number of succeed commands.

    pub fn update(&mut self, field_trajectory_vec: &[usize]) -> anyhow::Result<()> {
        // update cmd run time
        if let Some(command_runtime) = &self.command_runtime {
            let cmd_duration = self.get_elapsed_time()?;
            self.cmd_runtime_records
                .insert(command_runtime.step.to_string(), Value::from(cmd_duration));
        } else {
            warn!("Execution Start Local Time is not set.");
        }

        //update num_succ
        self.num_succ += 1;

        // update `Active`
        let curr_field = self.get_mut_cmd_field(field_trajectory_vec)?;

        curr_field["Active"] = json!(false);

        Ok(())
    }
}

/// This struct contains a command record and some supporting information.
/// It can be either a simpleaf command or an external command.
pub struct CommandRecord {
    pub step: u64,
    pub active: bool,
    pub program_name: ProgramName,
    pub simpleaf_cmd: Option<Commands>,
    pub external_cmd: Option<Command>,
    // This vector records the field name trajectory from the top level
    // this is used to update the `Step` after invoked successfully.
    pub field_trajectory_vec: Vec<usize>,
}

/// This enum represents the program name of a command.
/// It records simpleaf commands as their name and
/// all external command as `External(program name)`
#[derive(Debug, PartialEq)]
pub enum ProgramName {
    Index,
    Quant,
    External(String),
}

impl ProgramName {
    /// Instantiate a ProgramName enum according to a str
    pub fn from_str(field_name: &str) -> ProgramName {
        if field_name.starts_with("simpleaf") && field_name.ends_with("index") {
            ProgramName::Index
        } else if field_name.starts_with("simpleaf") && field_name.ends_with("quant") {
            ProgramName::Quant
        } else {
            ProgramName::External(field_name.to_string())
        }
    }

    /// check if the command is an external command.
    pub fn is_external(&self) -> bool {
        matches!(self, &ProgramName::External(_))
    }

    /// Create a valid simpleaf command object using the arguments recoreded in the field.
    /// Step and Program name will be ignored in this procedure
    pub fn create_simpleaf_cmd(&self, value: &Value) -> anyhow::Result<Commands> {
        let mut arg_vec = match self {
            ProgramName::Index => vec![String::from("simpleaf"), String::from("index")],
            ProgramName::Quant => vec![String::from("simpleaf"), String::from("quant")],
            _ => bail!("creating simpleaf command from external program."),
        };

        // Iterate over all (arg, value) pairs to
        // The assumption is that only valid argument (name,value) pairs are recorded in the root layers
        // They are all strings

        if let Value::Object(args) = value {
            for (k, v) in args {
                if !SKIPARG.contains(&k.as_str()) {
                    arg_vec.push(k.to_string());
                    if let Some(sv) = v.as_str() {
                        if !sv.is_empty() {
                            arg_vec.push(sv.to_string());
                        }
                    } else {
                        bail!("The value of argument `{}`,{} , cannot be converted as a string; Cannot proceed. Please provide valid arguments.", k, v.to_string());
                    }
                }
            }
        } else {
            warn!("Found an invalid root layer; Ignored. All root layers must represent a valid simpleaf command.");
        };

        // check if empty
        if !arg_vec.is_empty() {
            let cmd = Cli::parse_from(arg_vec).command;
            Ok(cmd)
        } else {
            bail!("Found simpleaf command with empty arg list. Cannot Proceed.")
        }
    }

    /// This function instantiates a std::process::Command
    /// for an external command record according to
    /// the  "Arguments" field.
    pub fn create_external_cmd(&self, value: &Value) -> anyhow::Result<Command> {
        // get the argument vector, which is named as "Argument"
        let arg_value_vec = value
            .get("Arguments")
            .with_context(||"Cannot find the `Arguments` field in the external command record; Cannot proceed")?
            .as_array()
            .with_context(||"Cannot convert the `Arguments` field in the external command record as an array; Cannot proceed")?;

        // initialize argument vector
        let mut arg_vec = vec![self.to_string()];
        arg_vec.reserve_exact(arg_value_vec.len() + 1);

        // fill in the argument vector
        for arg_value in arg_value_vec {
            let arg_str = arg_value.as_str().with_context(|| {
                format!("Could not convert {:?} as str; Cannot proceed", arg_value)
            })?;
            arg_vec.push(arg_str.to_string());
        }

        if !arg_vec.is_empty() {
            // make Command struct for the command
            let external_cmd = shell(arg_vec.join(" "));

            Ok(external_cmd)
        } else {
            bail!("Found an external command with empty arg list. Cannot Proceed.")
        }
    }
}

impl std::fmt::Display for ProgramName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}",
            match &self {
                ProgramName::Index => String::from("simpleaf index"),
                ProgramName::Quant => String::from("simpleaf quant"),
                ProgramName::External(pn) => pn.to_owned(),
            }
        )
    }
}

pub struct ProtocolEstuary {
    pub utils_dir: PathBuf,
    pub protocols_dir: PathBuf,
}

impl ProtocolEstuary {
    pub fn exists(&self) -> bool {
        self.protocols_dir.exists() && self.utils_dir.exists()
    }
}

/// parse the input file (either a workflow configuration file or a complete workflow JSON file) to obtain a JSON string.
pub fn parse_workflow_config<T: AsRef<Path>>(
    af_home_path: T,
    config_file_path: T,
    output: T,
    jpaths: &Option<Vec<PathBuf>>,
    ext_codes: &Option<Vec<String>>,
) -> anyhow::Result<String> {
    // get protocol_estuary path
    let protocol_estuary = get_protocol_estuary(af_home_path.as_ref(), RegistrySourceStrategy::PreferLocal)?;

    // the parse_jsonnet function calls the main function of jrsonnet.
    match parse_jsonnet(
        // af_home_path,
        config_file_path.as_ref(),
        output.as_ref(),
        &protocol_estuary.utils_dir,
        jpaths,
        ext_codes,
        true,
    ) {
        Ok(js) => Ok(js),
        Err(e) => Err(anyhow!(
            "Error occurred when processing the input config file {}. The error message was {}",
            config_file_path.as_ref().display(),
            e
        )),
    }
}

pub enum RegistrySourceStrategy {
    PreferLocal,
    ForceRefresh,
} 

impl RegistrySourceStrategy {
    pub fn is_force_refresh(&self) -> bool {
        match &self {
            RegistrySourceStrategy::PreferLocal => true,
            _ => false,
        }
    }
}

pub fn get_protocol_estuary<T: AsRef<Path>>(af_home_path: T, rss: RegistrySourceStrategy) -> anyhow::Result<ProtocolEstuary> {
    let dl_url = "https://github.com/COMBINE-lab/protocol-estuary/archive/refs/heads/main.zip";

    // define expected dirs and files
    let pe_dir = af_home_path.as_ref().join("protocol-estuary");
    let pe_main_dir = pe_dir.join("protocol-estuary-main");
    let protocols_dir = pe_main_dir.join("protocols");
    let utils_dir = pe_main_dir.join("utils");
    let pe_zip_file = pe_dir.join("protocol-estuary.zip");

    let protocol_estuary = ProtocolEstuary {
        protocols_dir,
        utils_dir,
    };

    // if output dir exists, then return
    if protocol_estuary.exists() && rss.is_force_refresh() {
        Ok(protocol_estuary)
    } else {
        // make pe
        if !pe_dir.exists() {
            run_cmd!(mkdir -p $pe_dir)?;
        }

        // download github repo as a zip file
        let mut dl_cmd = std::process::Command::new("wget");
        dl_cmd
            .arg("-v")
            .arg("-O")
            .arg(pe_zip_file.to_string_lossy().to_string())
            .arg("-L")
            .arg(dl_url);
        match prog_utils::execute_command(&mut dl_cmd, CommandVerbosityLevel::Quiet) {
            Ok(_output) => {}
            Err(e) => {
                return Err(anyhow!(
                    "failed to download protocol-estuary GitHub repository; error: {:?}",
                    e
                ));
            }
        }

        // unzip
        let mut unzip_cmd = std::process::Command::new("unzip");
        unzip_cmd
            .arg("-o")
            .arg(pe_zip_file.to_string_lossy().to_string())
            .arg("-d")
            .arg(pe_dir.to_string_lossy().to_string());

        match prog_utils::execute_command(&mut unzip_cmd, CommandVerbosityLevel::Quiet) {
            Ok(_output) => {}
            Err(e) => {
                // if failed, then remove dir and return with an error
                std::fs::remove_dir(pe_dir.as_path()).with_context({
                    || {
                        format!(
                            "failed to unzip protocol library zip file, \
                            then failed to remove the protocol library directory. \n\
                            Please remove it manually, for example, using `rm -rf {}`",
                            pe_dir.display()
                        )
                    }
                })?;
                return Err(anyhow!(
                    "failed to unzip protocol library zip file at {}. The error was: {:?}.",
                    pe_zip_file.display(),
                    e
                ));
            }
        }

        // final check
        if protocol_estuary.exists() {
            Ok(protocol_estuary)
        } else {
            bail!(
                "Could not fetch protocol library. \
                    This should not happen. \
                    Please submit an issue on the simpleaf GitHub repository."
            )
        }
    }
}

/// Copy all files from the src folder to the dst folder.\
/// Adapted from https://stackoverflow.com/a/65192210.
pub fn copy_dir_all(src: impl AsRef<Path>, dst: impl AsRef<Path>) -> anyhow::Result<()> {
    fs::create_dir_all(&dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let ty = entry.file_type()?;
        if ty.is_dir() {
            copy_dir_all(entry.path(), dst.as_ref().join(entry.file_name()))?;
        } else {
            fs::copy(entry.path(), dst.as_ref().join(entry.file_name()))?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    // use clap::Parser;

    use serde_json::{json, Map, Value};

    use super::ProgramName;
    // use crate::Cli;
    // use crate::Commands;
    // use crate::SimpleafCmdRecord;
    use crate::{
        utils::{
            prog_utils::{get_cmd_line_string, shell},
            workflow_utils::{initialize_workflow, WorkflowLog},
        },
        Commands,
    };
    use core::panic;
    use std::path::PathBuf;

    #[test]
    fn test_workflow_command() {
        let index = ProgramName::from_str("simpleaf index");
        let quant = ProgramName::from_str("simpleaf quant");
        let external = ProgramName::from_str("awk");

        assert_eq!(
            index,
            ProgramName::Index,
            "Could not get correct ProgramName from simpleaf index"
        );
        assert_eq!(
            quant,
            ProgramName::Quant,
            "Could not get correct ProgramName from simpleaf quant"
        );
        assert_eq!(
            external,
            ProgramName::External("awk".to_string()),
            "Could not get correct ProgramName from invalid command"
        );

        assert!(
            !index.is_external(),
            "ProgramName::Index is regarded as an external cmd."
        );
        assert!(
            !quant.is_external(),
            "ProgramName::Quant is regarded as an external cmd."
        );
        assert!(
            external.is_external(),
            "ProgramName::External is a simpleaf cmd."
        );
    }

    #[test]
    fn test_simpleaf_workflow_skip_start_at() {
        let af_home_path = PathBuf::from("af_home");
        let template = PathBuf::from("data_dir/fake_config.config");
        let output = PathBuf::from("output_dir");

        let workflow_json_string = String::from(
            r#"{
            "meta_info": {
                "output_dir": "output_dir"
            },
            "rna": {
                "simpleaf_index": {
                    "Step": 1,
                    "Program Name": "simpleaf index", 
                    "Active": true,
                    "--ref-type": "spliced+unspliced",
                    "--fasta": "genome.fa",
                    "--gtf": "genes.gtf",
                    "--output": "index_output",
                    "--use-piscem": "",
                    "--overwrite": ""
                },
                "simpleaf_quant": {
                    "Step": 2,
                    "Program Name": "simpleaf quant",  
                    "Active": true,
                    "--chemistry": "10xv3",
                    "--resolution": "cr-like",
                    "--expected-ori": "fw",
                    "--t2g-map": "t2g.tsv",
                    "--reads1": "reads1.fastq",
                    "--reads2": "reads2.fastq",
                    "--unfiltered-pl": "",
                    "--output": "quant_output",
                    "--index": "index_output/index",
                    "--use-piscem": "",
                    "--use-selective-alignment": ""
                }
            }, 
            "External Commands": {
                "HTO ref gunzip": {
                    "Step": 3,
                    "Program Name": "gunzip",
                    "Active": true,
                    "Arguments": ["-c","hto_ref.csv.gz",">","hto_ref.csv"]
                },
                "ADT ref gunzip": {
                    "Step": 4,
                    "Program Name": "gunzip",
                    "Active": true,
                    "Arguments": ["-c","adt_ref.csv.gz",">","adt_ref.csv"]
                }
            }
        }"#,
        );

        let workflow_json_value: Value =
            serde_json::from_str(workflow_json_string.as_str()).unwrap();

        // initialize simpleaf workflow and log struct
        let (mut sw, mut wl) = initialize_workflow(
            af_home_path.as_path(),
            template.as_path(),
            output.as_path(),
            workflow_json_value.clone(),
            2,
            vec![3],
            false,
        )
        .unwrap();

        match &wl {
            WorkflowLog {
                meta_info_path,
                exec_log_path,
                workflow_start_time: _,
                command_runtime,
                num_succ,
                start_at,
                workflow_name,
                workflow_meta_info,
                value,
                cmd_runtime_records,
                field_id_to_name,
                skip_step,
                previous_log: _,
            } => {
                // test wl
                // check JSON log output json
                assert_eq!(
                    exec_log_path,
                    &PathBuf::from("output_dir/workflow_execution_log.json")
                );
                assert_eq!(
                    meta_info_path,
                    &PathBuf::from("output_dir/simpleaf_workflow_log.json")
                );

                assert_eq!(workflow_name, &String::from("fake_config"));

                assert_eq!(
                    workflow_meta_info,
                    &Some(workflow_json_value.get("meta_info").unwrap().to_owned())
                );

                let mut new_value = value.to_owned();
                new_value["rna"]["simpleaf_index"]["Active"] = json!(true);
                new_value["External Commands"]["HTO ref gunzip"]["Active"] = json!(true);

                assert_eq!(new_value, workflow_json_value);

                assert_eq!(cmd_runtime_records, &Map::new());

                assert_eq!(start_at, &2u64);
                assert_eq!(skip_step, &vec![3]);
                assert!(
                    field_id_to_name.contains(&"rna".to_string())
                        && field_id_to_name.contains(&"meta_info".to_string())
                        && field_id_to_name.contains(&"simpleaf_index".to_string())
                        && field_id_to_name.contains(&"simpleaf_quant".to_string())
                        && field_id_to_name.contains(&"External Commands".to_string())
                        && field_id_to_name.contains(&"HTO ref gunzip".to_string())
                        && field_id_to_name.contains(&"ADT ref gunzip".to_string())
                );

                assert!(command_runtime.is_none());

                assert_eq!(num_succ, &0);
            }
        }

        // we started at 2, and skipped 3. So there are two commands
        // simpleaf_quant and ADT ref gunzip

        // first we check ADT ref gunzip
        let cmd = sw.cmd_queue.pop().unwrap();
        wl.timeit(cmd.step);

        wl.update(&cmd.field_trajectory_vec).unwrap();

        wl.get_mut_cmd_field(&cmd.field_trajectory_vec).unwrap();

        // check meta_info
        // we skipped two
        assert_eq!(
            wl.get_mut_cmd_field(&cmd.field_trajectory_vec).unwrap()["Step"].as_u64(),
            Some(4)
        );

        // check command #4
        assert_eq!(sw.cmd_queue.len(), 1);

        // let cmd = sw.cmd_queue.pop().unwrap();
        assert_eq!(cmd.step, 4);
        assert_eq!(cmd.program_name, ProgramName::from_str("gunzip"));
        assert!(cmd.external_cmd.is_some());
        assert!(cmd.simpleaf_cmd.is_none());

        let field_trajectory_vec = cmd.field_trajectory_vec.clone();
        let field_id_to_name = wl.field_id_to_name.clone();

        assert_eq!(
            field_id_to_name
                .get(field_trajectory_vec[0])
                .unwrap()
                .to_owned(),
            String::from("External Commands")
        );
        assert_eq!(
            field_id_to_name
                .get(field_trajectory_vec[1])
                .unwrap()
                .to_owned(),
            String::from("ADT ref gunzip")
        );
        let gunzip_cmd = shell("gunzip -c adt_ref.csv.gz > adt_ref.csv");

        assert_eq!(
            get_cmd_line_string(&cmd.external_cmd.unwrap()),
            get_cmd_line_string(&gunzip_cmd),
        );

        // check command #2: simpleaf quant
        let cmd = sw.cmd_queue.pop().unwrap();
        assert_eq!(cmd.step, 2);
        assert_eq!(cmd.program_name, ProgramName::from_str("simpleaf quant"));
        assert!(cmd.external_cmd.is_none());

        let field_trajectory_vec = cmd.field_trajectory_vec.clone();
        let field_id_to_name = wl.field_id_to_name.clone();

        assert_eq!(
            field_id_to_name
                .get(field_trajectory_vec[0])
                .unwrap()
                .to_owned(),
            String::from("rna")
        );
        assert_eq!(
            field_id_to_name
                .get(field_trajectory_vec[1])
                .unwrap()
                .to_owned(),
            String::from("simpleaf_quant")
        );

        match cmd.simpleaf_cmd {
            Some(Commands::Quant {
                chemistry,
                output,
                threads,
                index,
                reads1,
                reads2,
                use_selective_alignment,
                use_piscem,
                map_dir,
                knee,
                unfiltered_pl,
                forced_cells,
                explicit_pl,
                expect_cells,
                expected_ori,
                min_reads,
                t2g_map,
                resolution,
            }) => {
                assert_eq!(chemistry, String::from("10xv3"));
                assert_eq!(output, PathBuf::from("quant_output"));
                assert_eq!(threads, 16);
                assert_eq!(index, Some(PathBuf::from("index_output/index")));
                assert_eq!(reads1, Some(vec![PathBuf::from("reads1.fastq")]));
                assert_eq!(reads2, Some(vec![PathBuf::from("reads2.fastq")]));
                assert_eq!(use_selective_alignment, true);
                assert_eq!(use_piscem, true);
                assert_eq!(map_dir, None);
                assert_eq!(knee, false);
                assert_eq!(unfiltered_pl, Some(None));
                assert_eq!(forced_cells, None);
                assert_eq!(explicit_pl, None);
                assert_eq!(expect_cells, None);
                assert_eq!(expected_ori, Some(String::from("fw")));
                assert_eq!(min_reads, 10);
                assert_eq!(t2g_map, Some(PathBuf::from("t2g.tsv")));
                assert_eq!(resolution, String::from("cr-like"));
            }
            _ => panic!(),
        };
    }
}
