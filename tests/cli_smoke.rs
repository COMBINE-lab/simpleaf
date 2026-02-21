use std::process::{Command, Output};

use tempfile::tempdir;

fn run_simpleaf(args: &[&str]) -> Output {
    let binary = env!("CARGO_BIN_EXE_simpleaf");
    let af_home = tempdir().expect("unable to create temp ALEVIN_FRY_HOME");

    Command::new(binary)
        .args(args)
        .env("ALEVIN_FRY_HOME", af_home.path())
        .output()
        .expect("failed to execute simpleaf")
}

fn assert_parse_error(args: &[&str]) {
    let output = run_simpleaf(args);
    assert!(
        !output.status.success(),
        "expected parse error for args {:?}",
        args
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("Usage:"),
        "expected clap usage in stderr for {:?}, got:\n{}",
        args,
        stderr
    );
}

#[test]
fn quant_requires_input_type_group() {
    assert_parse_error(&[
        "quant", "-c", "10xv3", "-o", "/tmp/out", "-r", "cr-like", "--knee",
    ]);
}

#[test]
fn quant_rejects_conflicting_map_inputs() {
    assert_parse_error(&[
        "quant",
        "-c",
        "10xv3",
        "-o",
        "/tmp/out",
        "-r",
        "cr-like",
        "--knee",
        "--map-dir",
        "/tmp/mapped",
        "--index",
        "/tmp/index",
        "-1",
        "r1.fastq",
        "-2",
        "r2.fastq",
    ]);
}

#[test]
fn index_requires_gtf_when_fasta_is_provided() {
    assert_parse_error(&["index", "-f", "genome.fa", "-o", "/tmp/index_out"]);
}

#[test]
fn atac_command_requires_subcommand() {
    assert_parse_error(&["atac"]);
}
