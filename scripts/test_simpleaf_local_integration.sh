#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "${ROOT_DIR}"

echo "[local-integration] running deterministic integration tests"

# These tests avoid remote/networked resources and external large datasets.
cargo test --test cli_help_snapshots --test cli_smoke --test phase1_regressions

# Targeted workflow and chemistry regressions that cover refactor-critical paths.
cargo test --all-targets test_execute_commands_external_failure_reports_status_and_stderr
cargo test --all-targets clean_chemistries_dry_run_is_non_destructive_then_removes_unused
