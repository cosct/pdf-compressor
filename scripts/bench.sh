#!/usr/bin/env bash
# One-command engine benchmark (docs/BENCHMARK.md): build the release CLI
# and the cancel probe, generate the corpus on first run, then measure.
# `--check` compares against scripts/bench/baseline.json with the 10%
# relative regression threshold (human review, not an automatic block).
set -euo pipefail

cd "$(dirname "$0")/.."

cargo build --release --bin pdf-compressor-cli
cargo build --release -p pdf-core --example cancel_latency

if [[ ! -f target/bench/corpus/manifest.json ]]; then
  python3 scripts/bench/corpus.py
fi

python3 scripts/bench/run.py "$@"
