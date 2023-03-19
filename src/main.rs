use tracing_subscriber::{filter::LevelFilter, fmt, prelude::*, EnvFilter};

use anyhow::bail;
use clap::Parser;

// use std::io::{Seek, SeekFrom};
use std::env;
use std::path::PathBuf;

mod utils;

mod simpleaf_commands;
use simpleaf_commands::*;

/// simplifying alevin-fry workflows
#[derive(Debug, Parser)]
#[command(author, version, about)]
#[command(propagate_version = true)]
pub struct Cli {
    #[command(subcommand)]
    command: Commands,
}

fn main() -> anyhow::Result<()> {
    tracing_subscriber::registry()
        .with(fmt::layer())
        .with(
            EnvFilter::builder()
                .with_default_directive(LevelFilter::INFO.into())
                .from_env_lossy(),
        )
        .init();
    const AF_HOME: &str = "ALEVIN_FRY_HOME";
    let af_home_path = match env::var(AF_HOME) {
        Ok(p) => PathBuf::from(p),
        Err(e) => {
            bail!(
                "${} is unset {}, please set this environment variable to continue.",
                AF_HOME,
                e
            );
        }
    };

    let cli_args = Cli::parse();

    match cli_args.command {
        // set the paths where the relevant tools live
        Commands::SetPaths {
            salmon,
            piscem,
            alevin_fry,
            pyroe,
        } => set_paths(
            af_home_path,
            Commands::SetPaths {
                salmon,
                piscem,
                alevin_fry,
                pyroe,
            },
        ),
        Commands::AddChemistry { name, geometry } => {
            add_chemistry(af_home_path, Commands::AddChemistry { name, geometry })
        }
        Commands::Inspect {} => inspect_simpleaf(af_home_path),
        // if we are building the reference and indexing
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
        } => build_ref_and_index(
            af_home_path.as_path(),
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
        } => map_and_quant(
            af_home_path.as_path(),
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
        Commands::Workflow {
            config_path,
            workflow_path,
            output,
            no_execution,
            start_at,
            resume,
            lib_paths,
            skip_step,
        } => workflow(
            af_home_path.as_path(),
            Commands::Workflow {
                config_path,
                workflow_path,
                output,
                no_execution,
                start_at,
                resume,
                lib_paths,
                skip_step,
            },
        ),
        Commands::GetWorkflowConfig {
            output,
            workflow,
            // essential_only,
        } => get_workflow_config(
            af_home_path.as_path(),
            Commands::GetWorkflowConfig {
                output,
                workflow,
                // essential_only,
            },
        ),
    }
    // success, yay!
}
