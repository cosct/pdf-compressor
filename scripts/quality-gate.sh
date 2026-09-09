#!/usr/bin/env bash
# Full local quality gate — the per-push subset of CI (ci.yml): a green run
# means a green push for the build/test/lint legs. CI additionally runs the
# MSRV checks, scheduled fuzzing, and dependency audits, which are not
# mirrored here. 本地完整质量门：覆盖 CI 的构建/测试/lint 腿；MSRV/模糊
# 测试/依赖审计仅在 CI 执行。
#
# Usage: scripts/quality-gate.sh
set -euo pipefail

cd "$(dirname "$0")/.."

log() { printf '\n\033[1;36m==> %s\033[0m\n' "$1"; }

log "cargo fmt（workspace 格式检查）"
cargo fmt --all -- --check

log "cargo clippy（workspace，-D warnings）"
cargo clippy --workspace --all-targets --locked -- -D warnings

log "cargo test（workspace）"
cargo test --workspace --locked

log "cargo clippy（pdf-core 可选特性 jpx,cmyk-cms，-D warnings）"
cargo clippy -p pdf-core --all-targets --locked --features jpx,cmyk-cms -- -D warnings

log "cargo test（pdf-core 可选特性 jpx,cmyk-cms）"
cargo test -p pdf-core --locked --features jpx,cmyk-cms

log "cargo clippy（桌面壳可选特性 jpx,cmyk-cms，-D warnings）"
cargo clippy -p app --all-targets --locked --features jpx,cmyk-cms -- -D warnings

log "cargo test（桌面壳可选特性 jpx,cmyk-cms）"
cargo test -p app --locked --features jpx,cmyk-cms

log "前端格式化 + lint + 类型检查（vp check：oxfmt/oxlint/tsgo）"
pnpm run check

log "前端单元测试（vp test）"
pnpm test

log "前端类型检查与构建（vue-tsc + vp build）"
pnpm run build

log "质量门全部通过 ✅"
