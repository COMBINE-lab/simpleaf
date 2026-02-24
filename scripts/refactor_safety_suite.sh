#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "${ROOT_DIR}"

echo "[refactor-safety] running CLI help snapshot and parse-smoke tests"
cargo test --test cli_help_snapshots --test cli_smoke
