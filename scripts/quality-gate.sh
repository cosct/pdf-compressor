#!/usr/bin/env bash
# Full local quality gate — runs exactly what CI runs (ci.yml), so a green
# local run means a green push. 本地完整质量门：步骤与 CI 一一对应。
#
# Usage: scripts/quality-gate.sh
set -euo pipefail

cd "$(dirname "$0")/.."

log() { printf '\n\033[1;36m==> %s\033[0m\n' "$1"; }

log "cargo clippy（workspace，-D warnings）"
cargo clippy --workspace --all-targets --locked -- -D warnings

log "cargo test（workspace）"
cargo test --workspace --locked

log "cargo clippy（pdf-core 可选特性 jpx,cmyk-cms，-D warnings）"
cargo clippy -p pdf-core --all-targets --locked --features jpx,cmyk-cms -- -D warnings

log "cargo test（pdf-core 可选特性 jpx,cmyk-cms）"
cargo test -p pdf-core --locked --features jpx,cmyk-cms

log "前端格式化 + lint + 类型检查（vp check：oxfmt/oxlint/tsgo）"
pnpm run check

log "前端单元测试（vp test）"
pnpm test

log "前端类型检查与构建（vue-tsc + vp build）"
pnpm run build

log "质量门全部通过 ✅"
