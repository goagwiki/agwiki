#!/usr/bin/env bash
# CI/local: fmt, clippy -D warnings, tests (see .github/workflows/test.yml).
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-features "$@"
