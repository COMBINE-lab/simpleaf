use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use tempfile::tempdir;

fn snapshot_cases() -> Vec<(&'static str, Vec<&'static str>)> {
    vec![
        ("simpleaf___help.txt", vec!["--help"]),
        ("simpleaf_index___help.txt", vec!["index", "--help"]),
        ("simpleaf_quant___help.txt", vec!["quant", "--help"]),
        (
            "simpleaf_multiplex_quant___help.txt",
            vec!["multiplex-quant", "--help"],
        ),
        ("simpleaf_chemistry___help.txt", vec!["chemistry", "--help"]),
        (
            "simpleaf_chemistry_add___help.txt",
            vec!["chemistry", "add", "--help"],
        ),
        (
            "simpleaf_chemistry_remove___help.txt",
            vec!["chemistry", "remove", "--help"],
        ),
        (
            "simpleaf_chemistry_clean___help.txt",
            vec!["chemistry", "clean", "--help"],
        ),
        (
            "simpleaf_chemistry_lookup___help.txt",
            vec!["chemistry", "lookup", "--help"],
        ),
        (
            "simpleaf_chemistry_refresh___help.txt",
            vec!["chemistry", "refresh", "--help"],
        ),
        (
            "simpleaf_chemistry_fetch___help.txt",
            vec!["chemistry", "fetch", "--help"],
        ),
        ("simpleaf_inspect___help.txt", vec!["inspect", "--help"]),
        ("simpleaf_set_paths___help.txt", vec!["set-paths", "--help"]),
        (
            "simpleaf_refresh_prog_info___help.txt",
            vec!["refresh-prog-info", "--help"],
        ),
        ("simpleaf_workflow___help.txt", vec!["workflow", "--help"]),
        (
            "simpleaf_workflow_run___help.txt",
            vec!["workflow", "run", "--help"],
        ),
        (
            "simpleaf_workflow_get___help.txt",
            vec!["workflow", "get", "--help"],
        ),
        (
            "simpleaf_workflow_patch___help.txt",
            vec!["workflow", "patch", "--help"],
        ),
        (
            "simpleaf_workflow_list___help.txt",
            vec!["workflow", "list", "--help"],
        ),
        (
            "simpleaf_workflow_refresh___help.txt",
            vec!["workflow", "refresh", "--help"],
        ),
        ("simpleaf_atac___help.txt", vec!["atac", "--help"]),
        (
            "simpleaf_atac_index___help.txt",
            vec!["atac", "index", "--help"],
        ),
        (
            "simpleaf_atac_process___help.txt",
            vec!["atac", "process", "--help"],
        ),
    ]
}

fn snapshots_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("snapshots")
        .join("cli-help")
}

/// Rewrite the snapshots instead of asserting against them:
///
/// ```text
/// UPDATE_CLI_SNAPSHOTS=1 cargo test --test cli_help_snapshots
/// ```
///
/// Adding a flag legitimately changes this output, and without a documented way
/// to regenerate it the snapshots simply drift out of date — which is how
/// `--sample-bc-ori` landed with a failing test. Review the resulting diff
/// before committing it; that review is the point of the snapshots.
fn update_requested() -> bool {
    std::env::var_os("UPDATE_CLI_SNAPSHOTS").is_some_and(|v| !v.is_empty() && v != "0")
}

fn normalize_help(output: &str) -> String {
    let mut normalized = output
        .lines()
        .map(str::trim_end)
        .collect::<Vec<_>>()
        .join("\n");
    if output.ends_with('\n') {
        normalized.push('\n');
    }
    normalized
}

#[test]
fn cli_help_outputs_match_snapshots() {
    let binary = env!("CARGO_BIN_EXE_simpleaf");
    let af_home = tempdir().expect("unable to create temp ALEVIN_FRY_HOME");
    let updating = update_requested();

    for (snapshot_file, args) in snapshot_cases() {
        let output = Command::new(binary)
            .args(&args)
            .env("ALEVIN_FRY_HOME", af_home.path())
            .env("COLUMNS", "100")
            .output()
            .expect("failed to execute simpleaf");

        assert!(
            output.status.success(),
            "expected success for args {:?}, got status {:?}\nstderr:\n{}",
            args,
            output.status.code(),
            String::from_utf8_lossy(&output.stderr)
        );

        // Clap indents otherwise-empty separator lines. Keeping those spaces in
        // tracked snapshots makes `git diff --check` fail while adding no
        // information, so normalize trailing whitespace on both sides.
        let actual =
            normalize_help(&String::from_utf8(output.stdout).expect("stdout was not valid UTF-8"));
        let expected_path = snapshots_dir().join(snapshot_file);

        if updating {
            fs::write(&expected_path, &actual).unwrap_or_else(|e| {
                panic!("failed writing snapshot {}: {}", expected_path.display(), e)
            });
            continue;
        }

        let expected = normalize_help(&fs::read_to_string(&expected_path).unwrap_or_else(|e| {
            panic!("failed reading snapshot {}: {}", expected_path.display(), e)
        }));

        assert_eq!(
            actual, expected,
            "help output drifted for args {:?} against snapshot {}.\n\
             If this change is intended, regenerate with:\n    \
             UPDATE_CLI_SNAPSHOTS=1 cargo test --test cli_help_snapshots",
            args, snapshot_file
        );
    }

    assert!(
        !updating,
        "snapshots were rewritten; re-run without UPDATE_CLI_SNAPSHOTS to verify, \
         and review the diff before committing"
    );
}
