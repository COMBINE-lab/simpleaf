use std::fs;
use std::process::Command;

use serde_json::json;
use tempfile::tempdir;

#[test]
fn salmon_missing_error_mentions_salmon_executable() {
    let binary = env!("CARGO_BIN_EXE_simpleaf");
    let af_home = tempdir().expect("failed to create temp af home");
    let t2g = af_home.path().join("t2g.tsv");
    fs::write(&t2g, "tx1\tgene1\n").expect("failed to write t2g");

    let af_info = json!({
        "prog_info": {
            "salmon": null,
            "piscem": null,
            "alevin_fry": {"exe_path": "/bin/echo", "version": "0.11.2"},
            "macs": null
        }
    });
    fs::write(
        af_home.path().join("simpleaf_info.json"),
        serde_json::to_string_pretty(&af_info).expect("failed to serialize af info"),
    )
    .expect("failed to write simpleaf_info.json");

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
            "/tmp/fake_index",
            "--no-piscem",
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
        stderr.contains("no salmon executable is provided"),
        "stderr did not mention salmon executable:\n{}",
        stderr
    );
}
