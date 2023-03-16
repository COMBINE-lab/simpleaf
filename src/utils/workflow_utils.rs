use anyhow::{anyhow, bail, Context};
use clap::Parser;
use cmd_lib::run_fun;
use serde_json::{json, Value};
use std::isize;
use std::path::{Path, PathBuf};
use std::process::Command;
use tracing::warn;

use crate::utils::jrsonnet_main::parse_jsonnet;
use crate::{Cli, Commands};

/// intialize simpleaf workflow realted structs,
/// which includes SimpleafWorkflow and WorkfowLog

pub fn initialize_workflow(
    af_home_path: &Path,
    config_path: &Path,
    output: &Path,
    workflow_json_value: Value,
    start_at: isize,
) -> anyhow::Result<(SimpleafWorkflow, WorkflowLog)> {
    // Instantiate a workflow log struct
    let mut wl = WorkflowLog::new(output, config_path, &workflow_json_value)?;

    // instantiate a simpleaf workflow struct, and complete the workflow struct
    let sw = SimpleafWorkflow::new(af_home_path, &workflow_json_value, &mut wl, start_at)?;

    Ok((sw, wl))
}

// Each SimpleafWorkflow represents a json
/// Simpleaf Workflow record
pub struct SimpleafWorkflow {
    // TODO: Currently this is optional
    // can change to a hashmap if we need this in the future to do validation
    pub meta_info: Option<Value>,
    pub af_home_path: PathBuf,

    // This command queue contains all commands that need to be run
    pub cmd_queue: Vec<CommandRecord>,
}

impl SimpleafWorkflow {
    /// Initialize a SimpleafWorkflow object.
    /// It needs an empty and mutable `WorkflowLog` as a complementary part.
    pub fn new(
        af_home_path: &Path,
        workflow_json_value: &Value,
        workflow_log: &mut WorkflowLog,
        start_at: isize,
    ) -> anyhow::Result<SimpleafWorkflow> {
        // get meta_info
        let meta_info = workflow_json_value.get("meta_info").map(|v| v.to_owned());

        // if we don't see an meta info section, report a warning
        if meta_info.is_none() {
            warn!("Found config file without meta_info field.");
            // warning and create a empty string
        };

        // Then we recursively get all simpleaf command and put them into a queue
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
            start_at,
        )?;

        // sort the cmd queue by it execution order.
        cmd_queue.sort_by(|cmd1, cmd2| cmd1.execution_order.cmp(&cmd2.execution_order));

        Ok(SimpleafWorkflow {
            meta_info,
            af_home_path: af_home_path.to_owned(),
            cmd_queue,
        })
    }

    /// This function collect the command records from a `serde_json::Value` that records a complete simpleaf workflow,
    /// parse them as `CommandRecord` structs and push them into the `cmd_queue` vector.
    /// ### Details
    /// This function will iterate over all layers in the `Value` object to find the command records
    /// with both `Execution Order` and `Program Name`. **These two fields must appear simutaneously**, otherwise this function
    /// will return an error.  
    /// A CommandRecord struct will be initilizaed from Each command record with a positive execution order,
    /// including external commands and simpleaf command,
    /// and will be pushed into the `cmd_queue` vector.
    /// At the same time, a `WorkflowLog` struct will be completed for logging purpose.
    fn fill_cmd_queue(
        workflow_json_value: &Value,
        cmd_queue: &mut Vec<CommandRecord>,
        field_trajectory_vec: Vec<usize>,
        workflow_log: &mut WorkflowLog,
        start_at: isize,
    ) -> anyhow::Result<()> {
        // save some allocation
        let mut pn: ProgramName;
        if let Value::Object(value_inner) = workflow_json_value {
            // As we don't know how many layers the json has, we will recursively call this function to get a vector of vector
            for (field_name, field) in value_inner {
                // clone the vec and push the current field name
                let mut curr_field_trajectory_vec = field_trajectory_vec.clone();

                curr_field_trajectory_vec.push(workflow_log.get_field_id(field_name));

                // If "Execution Order" exists, then this field records an external or a simpleaf command
                if field.get("Execution Order").is_some() {
                    // parse execution order
                    let execution_order = field
                        .get("Execution Order")
                        .expect("Cannot get Execution Order")
                        .as_str()
                        .expect("cannot parse Execution Order as str")
                        .parse::<isize>()
                        .expect("Cannot parse Execution Order as an integer");

                    // only parse commands with a positive execution order
                    if !execution_order.is_negative() && execution_order >= start_at {
                        // The field must contains an Program Name
                        if let Some(program_name) = field.get("Program Name") {
                            pn = ProgramName::from_str(program_name.as_str().unwrap());

                            // The execution order will be used for sorting the cmd_queue vector.
                            // All commands must have an valid execution order.
                            // Note that we store this as a string in json b/c all value in config
                            // file are strings.
                            if pn.is_external() {
                                // creating an external command records using the args recorded in the field
                                let external_cmd = pn.create_external_cmd(field)?;

                                cmd_queue.push(CommandRecord {
                                    execution_order,
                                    program_name: pn,
                                    simpleaf_cmd: None,
                                    external_cmd: Some(external_cmd),
                                    field_trajectory_vec: curr_field_trajectory_vec,
                                });
                            } else {
                                // create a simpleaf command record using the args recorded in the field
                                let simpleaf_cmd = pn.create_simpleaf_cmd(field)?;

                                cmd_queue.push(CommandRecord {
                                    execution_order,
                                    program_name: pn,
                                    simpleaf_cmd: Some(simpleaf_cmd),
                                    external_cmd: None,
                                    field_trajectory_vec: curr_field_trajectory_vec,
                                });
                            }
                        }
                    }
                } else {
                    // If this is not a command record, we move to the next level
                    SimpleafWorkflow::fill_cmd_queue(
                        field,
                        cmd_queue,
                        curr_field_trajectory_vec,
                        workflow_log,
                        start_at,
                    )?;
                }
            }
        }
        Ok(())
    }
}

/// This struct is used for writing the workflow log JSON file.
/// ### What?
/// This struct contains the parsed and complete workflow JSON record.\
/// The `Execution Order` field of the successfully invoked commands will be set as a negatvie integer
/// and will be ignored by simpleaf if feeding the output JSON file to simpleaf workflow.\
/// ### Why?
/// The purpose of having this log is that if some command in the workflow fails, the user can
/// fix that command using this log file, and feed resume the execution of the workflow from the failed step.
/// ### How?
/// It will be initialized together with the `SimpleafWorkflow` struct and
/// will be used to write a workflow JSON file that is the same as the one
/// interpreted from user-provided JSONNET file except the Execution Order
/// field of the commands that were run sucessfully are negative values.
pub struct WorkflowLog {
    out_path: PathBuf,
    // The value records the complete simpleaf workflow
    value: Value,
    // this vector records all field names in the complete workflow.
    // This is used for locating the Execution Order for each command
    field_id_to_name: Vec<String>,
    // TODO: the trajectory vector in each CommandRecord can be
    // move here as a long vector, and in each CommandRecord
    // we just need to record the start pos and the length of its trajectory.
    // cmds_field_id_trajectory: Vec<usize>,
}
impl WorkflowLog {
    /// This function instantiate a workflow log
    /// with a valid output path and complete workflow as a `serde_json::Value` object
    pub fn new(
        output: &Path,
        config_path: &Path,
        workflow_json_value: &Value,
    ) -> anyhow::Result<WorkflowLog> {
        // get output json path
        let mut out_path =
            output.join(config_path.file_stem().unwrap_or_else(|| {
                panic!("Cannot parse file name of file {}", config_path.display())
            }));
        out_path.set_extension("json");

        Ok(WorkflowLog {
            out_path,
            value: workflow_json_value.clone(),
            field_id_to_name: Vec::new(),
            // cmds_field_id_trajectory: Vec::new()
        })
    }

    /// Write the current workflow record to the output path.
    pub fn write(&self) -> anyhow::Result<()> {
        std::fs::write(
            self.out_path.as_path(),
            serde_json::to_string_pretty(&self.value)
                .expect("Cannot convert json value to string."),
        )
        .with_context(|| {
            format!(
                "could not write complete simpleaf workflow JSON file to {}",
                self.out_path.display()
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

    #[allow(dead_code)]
    /// This function is used for testing if the exection order of
    /// successfully invoked command can be updated to a negative value
    pub fn get_execution_order(&mut self, field_trajectory_vec: &[usize]) -> String {
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
                .expect("Cannot get field from json value");
        }

        // prepend a "-" to the execution order
        curr_field = curr_field
            .get_mut("Execution Order")
            .expect("Cannot get execution order of the command.");

        curr_field
            .as_str()
            .expect("Cannot convert execution order as an integer")
            .to_string()
    }

    // Update the Execution order of a command record in the complete workflow serde_json::Value object
    // by following a trajectory path.
    pub fn update(&mut self, field_trajectory_vec: &[usize]) {
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
                .expect("Cannot get field from json value");
        }

        // prepend a "-" to the execution order
        curr_field = curr_field
            .get_mut("Execution Order")
            .expect("Cannot get execution order of the command.");
        *curr_field = json!(format!(
            "-{}",
            curr_field
                .as_str()
                .expect("Cannot convert execution order as an integer")
        ));
    }
}

/// This struct contains a command record and some supporting information.
/// It can be either a simpleaf command or an external command.
pub struct CommandRecord {
    pub execution_order: isize,
    pub program_name: ProgramName,
    pub simpleaf_cmd: Option<Commands>,
    pub external_cmd: Option<Command>,
    // This vector records the field name trajectory from the top level
    // this is used to update the execution order after invoked successfully.
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
        match field_name {
            "simpleaf index" => ProgramName::Index,
            "simpleaf quant" => ProgramName::Quant,
            exp_name => ProgramName::External(exp_name.to_string()),
        }
    }

    /// check if the command is an external command.
    pub fn is_external(&self) -> bool {
        matches!(self, &ProgramName::External(_))
    }

    /// Create a valid simpleaf command object using the arguments recoreded in the field.
    /// Execution order and Program name will be ignored in this procedure
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
            // the "Execution order" field will be ignore as it is not a valid simpleaf arg
            for (k, v) in args {
                if k.as_str() != "Execution Order" && k.as_str() != "Program Name" {
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
    /// for the external command according to
    /// the arguments recoreded in the field.
    pub fn create_external_cmd(&self, value: &Value) -> anyhow::Result<Command> {
        let mut arg_vec: Vec<(usize, String)> = Vec::new();
        // iterate the command object, record the arg into a vector
        if let Value::Object(args) = value {
            // the "Execution order" and "Program Name" fields will be ignore as it is not a valid simpleaf arg
            for (p, v) in args {
                if p.as_str() != "Execution Order" && p.as_str() != "Program Name" {
                    arg_vec.push((
                                p.parse::<usize>().expect("Cannot convert the argument position in the external command to an integer"),
                                v.as_str().expect("Cannot convert the argument value in external program call to a string.").to_string(),
                    ));
                }
            }
        }

        // sort the argument according to arg name, which are all integers.
        // This is because json doesn't reserve order
        arg_vec.sort_by(|first, second| first.0.cmp(&second.0));

        if !arg_vec.is_empty() {
            // make Command struct for the command
            let mut external_cmd = std::process::Command::new(self.to_string());
            for ea in arg_vec {
                external_cmd.arg(ea.1);
            }
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

/// parse the input file (either a workflow configuration file or a complete workflow JSON file) to obtain a JSON string.
pub fn parse_workflow_config(
    af_home_path: &Path,
    config_file_path: &Path,
    output: &Path,
) -> anyhow::Result<String> {
    // TODO do something like the `get_permit_if_absent function`
    // let protocol_estuary_path = af_home_path
    // .join("protocol_estuary");
    let utils_libsonnet_path = af_home_path
        .join("protocol_estuary")
        .join("utils.libsonnet");

    // check if the config parser file exist
    let utils_libsonnet_exist = utils_libsonnet_path
    .as_path()
    .try_exists()
    .with_context(|| format!("Could not find util fu for {}. Please make sure this is a valid simpleaf workflow configuration file.", config_file_path.display()))?;

    // TODO: download utils file if it doesn't exist.
    if !utils_libsonnet_exist {
        get_protocol_repo_if_absent(af_home_path)?;
    }

    // the parse_jsonnet function calls the main function of jrsonnet.
    match parse_jsonnet(
        // af_home_path,
        config_file_path,
        utils_libsonnet_path.as_path(),
        output,
    ) {
        Ok(js) => Ok(js),
        Err(e) => Err(anyhow!(
            "Error occurred when processing the input config file {}. The error message was {}",
            config_file_path.display(),
            e
        )),
    }
}

pub fn get_protocol_repo_if_absent(af_home: &Path) -> anyhow::Result<()> {
    let dl_url = "https://github.com/COMBINE-lab/protocol-estuary/archive/refs/heads/main.zip";

    let odir = af_home.join("protocol_estuary");
    let ofile = odir.join("protocol-estuary.zip");
    if odir.exists() {
        Ok(())
    } else {
        run_fun!(mkdir -p $odir)?;
        let mut dl_cmd = std::process::Command::new("wget");
        dl_cmd
            .arg("-v")
            .arg("-O")
            .arg(ofile.to_string_lossy().to_string())
            .arg("-L")
            .arg(dl_url);
        let r = dl_cmd.output()?;
        if !r.status.success() {
            return Err(anyhow!(
                "failed to download protocol-estuary GitHub Repository {:?}",
                r.status
            ));
        }

        let mut unzip_cmd = std::process::Command::new("unzip");
        unzip_cmd
            .arg("-d")
            .arg(ofile.to_string_lossy().to_string())
            .arg("-O")
            .arg(odir.to_string_lossy().to_string());

        let r = unzip_cmd.output()?;
        if !r.status.success() {
            return Err(anyhow!(
                "failed to unzip protocol-estuary GitHub Repository zip file: {:?}",
                r.status
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    // use clap::Parser;

    use super::ProgramName;
    // use crate::Cli;
    // use crate::Commands;
    // use crate::SimpleafCmdRecord;
    use crate::{
        utils::{prog_utils::get_cmd_line_string, workflow_utils::initialize_workflow},
        Commands, ReferenceType,
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
    fn test_simpleaf_workflow() {
        let af_home_path = PathBuf::from("af_home");
        let config_path = PathBuf::from("data_dir/fake_config.config");
        let output = PathBuf::from("output_dir");

        let workflow_json_string = String::from(
            r#"{
            "meta_info": {
                "output_dir": "output_dir"
            },
            "rna": {
                "simpleaf index": {
                    "Execution Order": "1",
                    "Program Name": "simpleaf index", 
                    "--ref-type": "spliced+unspliced",
                    "--fasta": "genome.fa",
                    "--gtf": "genes.gtf",
                    "--output": "index_output",
                    "--use-piscem": "",
                    "--overwrite": ""
                },
                "simpleaf quant": {
                    "Execution Order": "2",
                    "Program Name": "simpleaf quant",  
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
                    "Execution Order": "3",
                    "Program Name": "gunzip",
                    "1": "-c",
                    "2": "hto_ref.csv.gz",
                    "3": ">",
                    "4": "hto_ref.csv"
                },
                "ADT ref gunzip": {
                    "Execution Order": "4",
                    "Program Name": "gunzip",
                    "1": "-c",
                    "2": "adt_ref.csv.gz",
                    "3": ">",
                    "4": "adt_ref.csv"
                }
            }
        }"#,
        );

        let workflow_json_value = serde_json::from_str(workflow_json_string.as_str()).unwrap();

        // initialize simpleaf workflow and log struct
        let (mut sw, mut wl) = initialize_workflow(
            af_home_path.as_path(),
            config_path.as_path(),
            output.as_path(),
            workflow_json_value,
            1,
        )
        .unwrap();

        // test wl
        // check JSON log output json
        assert_eq!(wl.out_path, PathBuf::from("output_dir/fake_config.json"));

        let first_cmd = sw.cmd_queue.first().unwrap();

        wl.update(&first_cmd.field_trajectory_vec);

        wl.get_execution_order(&first_cmd.field_trajectory_vec);

        // check meta_info
        assert_eq!(
            wl.get_execution_order(&first_cmd.field_trajectory_vec),
            String::from("-1")
        );

        // check command #4

        let cmd = sw.cmd_queue.pop().unwrap();
        assert_eq!(cmd.execution_order, 4);
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
        assert_eq!(
            get_cmd_line_string(&cmd.external_cmd.unwrap()),
            String::from("gunzip -c adt_ref.csv.gz > adt_ref.csv")
        );

        sw.cmd_queue.pop();
        // check command #2: simpleaf quant
        let cmd = sw.cmd_queue.pop().unwrap();
        assert_eq!(cmd.execution_order, 2);
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
            String::from("simpleaf quant")
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

        // check command #1: simpleaf index
        let cmd = sw.cmd_queue.pop().unwrap();
        assert_eq!(cmd.execution_order, 1);
        assert_eq!(cmd.program_name, ProgramName::from_str("simpleaf index"));
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
            String::from("simpleaf index")
        );

        match cmd.simpleaf_cmd {
            Some(Commands::Index {
                ref_type,
                fasta,
                gtf,
                rlen,
                dedup,
                ref_seq,
                spliced,
                unspliced,
                use_piscem,
                minimizer_length,
                output,
                overwrite,
                threads,
                kmer_length,
                keep_duplicates,
                sparse,
            }) => {
                match ref_type {
                    ReferenceType::SplicedUnspliced => {}
                    ReferenceType::SplicedIntronic => panic!("should be spliceu"),
                };
                assert_eq!(fasta, Some(PathBuf::from("genome.fa")));
                assert_eq!(gtf, Some(PathBuf::from("genes.gtf")));
                assert_eq!(rlen, None);
                assert_eq!(ref_seq, None);
                assert_eq!(spliced, None);
                assert_eq!(unspliced, None);
                assert_eq!(use_piscem, true);
                assert_eq!(minimizer_length, 19);
                assert_eq!(output, PathBuf::from("index_output"));
                assert_eq!(overwrite, true);
                assert_eq!(threads, 16);
                assert_eq!(kmer_length, 31);
                assert_eq!(keep_duplicates, false);
                assert_eq!(sparse, false);
                assert_eq!(dedup, false);
            }
            _ => panic!(),
        };
    }
}
