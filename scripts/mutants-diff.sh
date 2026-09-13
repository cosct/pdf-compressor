#!/usr/bin/env bash
# Mutation-testing diff gate (optional, 0.9.0): run cargo-mutants against
# only the code changed relative to a baseline (default: origin/main), so a
# PR author can check "did my change introduce logic that no test kills?"
# without paying for a full-corpus mutants run.
#
# Usage:
#   scripts/mutants-diff.sh                 # diff against origin/main
#   scripts/mutants-diff.sh main            # diff against a local branch
#
# Requirements: cargo-mutants installed (cargo install cargo-mutants) and a
# nightly toolchain (mutants builds with its own profile; the crate already
# carries .cargo/mutants.toml config).
set -euo pipefail

cd "$(dirname "$0")/.."

BASELINE="${1:-origin/main}"

if ! command -v cargo mutants >/dev/null 2>&1; then
    echo "error: cargo-mutants is not installed (cargo install cargo-mutants)" >&2
    exit 1
fi

# Refresh the baseline ref without touching the working tree.
git fetch origin "${BASELINE#origin/}" 2>/dev/null || true

if ! git rev-parse --verify --quiet "${BASELINE}^{commit}" >/dev/null; then
    echo "error: baseline '${BASELINE}' is not a resolvable commit" >&2
    exit 1
fi

echo "==> mutants diff against ${BASELINE}"
cargo mutants -p pdf-core --in-diff <(git diff -U0 "${BASELINE}...HEAD")
