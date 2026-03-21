use std::fs;
use std::process::Command;

use serde_json::json;
use tempfile::tempdir;

#[test]
fn legacy_salmon_index_metadata_is_rejected_with_migration_error() {
    let binary = env!("CARGO_BIN_EXE_simpleaf");
    let af_home = tempdir().expect("failed to create temp af home");
    let index_dir = af_home.path().join("index");
    fs::create_dir_all(&index_dir).expect("failed to create index dir");

    let t2g = af_home.path().join("t2g.tsv");
    fs::write(&t2g, "tx1\tgene1\n").expect("failed to write t2g");

    let af_info = json!({
        "prog_info": {
            "piscem": {"exe_path": "/bin/echo", "version": "0.18.0"},
            "alevin_fry": {"exe_path": "/bin/echo", "version": "0.13.0"},
            "macs": null
        }
    });
    fs::write(
        af_home.path().join("simpleaf_info.json"),
        serde_json::to_string_pretty(&af_info).expect("failed to serialize af info"),
    )
    .expect("failed to write simpleaf_info.json");

    let index_json = json!({
        "index_type": "salmon",
        "t2g_file": "t2g_3col.tsv"
    });
    fs::write(
        index_dir.join("simpleaf_index.json"),
        serde_json::to_string_pretty(&index_json).expect("failed to serialize index info"),
    )
    .expect("failed to write simpleaf_index.json");

    let output = Command::new(binary)
        .env("ALEVIN_FRY_HOME", af_home.path())
        .args([
            "quant",
            "-c",
            "10xv3",
            "-o",
            "/tmp/out",
            "-r",
            "cr-like",
            "--knee",
            "-i",
            &index_dir.to_string_lossy(),
            "-1",
            "r1.fastq",
            "-2",
            "r2.fastq",
            "-m",
            &t2g.to_string_lossy(),
        ])
        .output()
        .expect("failed to run simpleaf");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("no longer supported"),
        "stderr did not mention migration guidance:\n{}",
        stderr
    );
}
