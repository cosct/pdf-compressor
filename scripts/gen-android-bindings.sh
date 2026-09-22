#!/usr/bin/env bash
# Regenerate the UniFFI Kotlin bindings for pdf-core-ffi and refresh the
# committed copy under android/app/src/main/uniffi/ — the same "generated
# artifacts live in the repo, freshness gated by diff" discipline as the
# desktop's frontend/src/lib/bindings.ts.
# 重新生成 pdf-core-ffi 的 UniFFI Kotlin 绑定并刷新入库副本 —— 与桌面端
# bindings.ts 相同的"生成物入库 + diff 新鲜度门禁"纪律。
#
# Usage:
#   scripts/gen-android-bindings.sh           # regenerate + refresh
#   scripts/gen-android-bindings.sh --check   # CI freshness gate: fail on drift
#
# Requires: cargo, network (first run fetches uniffi), a host toolchain.
# The bindgen binary in uniffi 0.3x ships inside the `uniffi` crate behind
# its `cli` feature, so the script builds an ephemeral runner crate pinned
# to the same version as crates/pdf-core-ffi.
set -euo pipefail

cd "$(dirname "$0")/.."
REPO="$(pwd -P)"

CHECK=0
if [[ "${1:-}" == "--check" ]]; then
  CHECK=1
fi

# Keep in lockstep with the `uniffi = "=..."` pin in crates/pdf-core-ffi/Cargo.toml.
BINDGEN_VERSION="$(sed -n 's/^uniffi = "=\(.*\)"/\1/p' crates/pdf-core-ffi/Cargo.toml)"
if [[ -z "$BINDGEN_VERSION" ]]; then
  echo "error: could not read the uniffi pin from crates/pdf-core-ffi/Cargo.toml" >&2
  exit 1
fi

TARGET_DIR="$(cargo metadata --format-version 1 | python3 -c 'import json,sys; print(json.load(sys.stdin)["target_directory"])')"
LIB="$TARGET_DIR/debug/libpdf_core_ffi.so"

echo "==> cargo build -p pdf-core-ffi --lib (host cdylib for library mode)"
cargo build -p pdf-core-ffi --lib
if [[ ! -f "$LIB" ]]; then
  echo "error: expected host cdylib at $LIB" >&2
  exit 1
fi

RUNNER="$(mktemp -d)"
OUT="$(mktemp -d)"
trap 'rm -rf "$RUNNER" "$OUT"' EXIT

echo "==> building ephemeral uniffi-bindgen runner (=uniffi $BINDGEN_VERSION, cli feature)"
cat > "$RUNNER/Cargo.toml" <<TOML
[package]
name = "uniffi-cli-runner"
version = "0.0.0"
edition = "2021"

[dependencies]
uniffi = { version = "=$BINDGEN_VERSION", features = ["cli"] }

[workspace]

[profile.release]
strip = true
TOML
mkdir -p "$RUNNER/src"
echo 'fn main() { uniffi::uniffi_bindgen_main() }' > "$RUNNER/src/main.rs"

echo "==> uniffi-bindgen generate --library (kotlin)"
(cd "$RUNNER" && cargo run --release --quiet -- generate \
  --library "$LIB" \
  --language kotlin \
  --config "$REPO/crates/pdf-core-ffi/uniffi.toml" \
  --no-format \
  --out-dir "$OUT")

GENERATED="$OUT/uniffi/pdfcompressor/pdfcompressor.kt"
COMMITTED="android/app/src/main/uniffi/uniffi/pdfcompressor/pdfcompressor.kt"

if [[ "$CHECK" -eq 1 ]]; then
  if ! diff -u "$COMMITTED" "$GENERATED"; then
    echo ""
    echo "error: committed Kotlin bindings are stale." >&2
    echo "       run scripts/gen-android-bindings.sh and commit the result." >&2
    exit 1
  fi
  echo "bindings fresh ✅"
else
  mkdir -p "$(dirname "$COMMITTED")"
  cp "$GENERATED" "$COMMITTED"
  echo "refreshed $COMMITTED"
fi
