# simpleaf Architecture Map

This document summarizes the post-refactor module boundaries and where new code should live.

## CLI Entry and Dispatch
- `src/main.rs`
  - Parses CLI arguments and dispatches to command handlers.
  - Should remain thin: argument parsing, high-level routing, and top-level logging only.
- `src/simpleaf_commands.rs`
  - Defines CLI option structs and subcommands.
  - Avoid business logic here; keep it as schema/contract for command-line interfaces.

## Command Implementations
- `src/simpleaf_commands/indexing.rs`
  - RNA reference/index orchestration.
  - Uses staged outputs for readability and testability.
- `src/simpleaf_commands/quant.rs`
  - RNA mapping/quantification orchestration.
  - Stage-level helpers produce typed intermediate outputs.
- `src/atac/process.rs`
  - ATAC processing pipeline (map/gpl/sort/macs).
  - Stage decomposition mirrors RNA command structure.
- `src/simpleaf_commands/workflow.rs`
  - Workflow command front-end (run/list/get/patch/refresh).
  - Handles run-input resolution before delegating planning/execution.
- `src/simpleaf_commands/chemistry.rs`
  - Chemistry registry and permit-list lifecycle operations.
  - Contains registry merge/update logic and dry-run-safe file operations.

## Core Shared Infrastructure
- `src/core/context.rs`
  - Runtime context loading/validation (e.g., AF home, tool info).
- `src/core/exec.rs`
  - Checked command execution helpers.
- `src/core/index_meta.rs`
  - Shared index metadata discovery and parsing.
- `src/core/runtime.rs`
  - Runtime helpers (e.g., thread capping).
- `src/core/io.rs`
  - JSON file read/write and atomic write helpers.

## Utility Layer
- `src/utils/workflow_utils.rs`
  - Workflow manifest validation, planning, command queue building, execution, and logging.
  - Contains typed workflow execution errors and log schema construction.
- `src/utils/chem_utils.rs`
  - Chemistry model definitions and registry parsing helpers.
- `src/utils/prog_utils.rs`
  - Program/tool discovery and generic process/IO utilities.
- `src/utils/af_utils.rs`
  - Domain utilities for geometry and command-level support logic.

## Testing Layout
- `tests/cli_help_snapshots.rs`
  - Guards user-facing CLI help contract.
- `tests/cli_smoke.rs`
  - Lightweight argument/dispatch integration checks.
- `tests/phase1_regressions.rs`
  - High-value regression checks for prior production bugs.
- Module-local unit tests in `src/**`
  - Stage/helper-focused tests close to implementation.

## CI and Validation Paths
- Strict quality gate:
  - `cargo fmt --check`
  - `cargo clippy --all-targets -- -D warnings`
  - `cargo test --all-targets`
- Deterministic local integration path:
  - `scripts/test_simpleaf_local_integration.sh`
- Full e2e toy dataset path (heavier):
  - `scripts/test_simpleaf.sh`

## Extension Guidelines
- Add reusable primitives under `src/core/` before duplicating orchestration logic.
- Keep command modules focused on stage orchestration and user-facing flow.
- Prefer typed errors (`thiserror`) where multiple internal failure variants matter.
- Use atomic writes for registry/log/state files that are critical for resume/recovery flows.
