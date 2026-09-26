#!/usr/bin/env bash
# Cross-compile the pdf-core-ffi cdylib for Android ABIs into the Gradle
# jniLibs tree (gitignored — always a build artifact, never committed).
# 交叉编译 pdf-core-ffi 的安卓 cdylib 到 Gradle jniLibs 目录（构建产物，
# gitignore，不入库）。
#
# Usage:
#   scripts/build-android-libs.sh                # debug, arm64-v8a + x86_64
#   scripts/build-android-libs.sh --release      # release + NDK strip
#   scripts/build-android-libs.sh --release --targets arm64-v8a
#
# Requires: rustup target add aarch64-linux-android x86_64-linux-android,
#           cargo-ndk (cargo install cargo-ndk), ANDROID_NDK_HOME pointing
#           at an NDK (or android-sdk/ndk/<version> auto-detected).
#
# The build stays pure-Rust: pdf-core-ffi depends on pdf-core with default
# features off, so flate2 resolves to lopdf's miniz_oxide backend and no
# cmake/NDK toolchain file is needed for any C dependency.
set -euo pipefail

cd "$(dirname "$0")/.."

MODE="debug"
TARGETS=("arm64-v8a" "x86_64")
while [[ $# -gt 0 ]]; do
  case "$1" in
    --release) MODE="release";;
    --targets) shift; IFS=',' read -ra TARGETS <<< "$1";;
    *) echo "unknown flag: $1" >&2; exit 1;;
  esac
  shift
done

if ! command -v cargo-ndk >/dev/null 2>&1; then
  echo "error: cargo-ndk not found — cargo install cargo-ndk" >&2
  exit 1
fi

# cargo-ndk accepts ANDROID_NDK_HOME or ANDROID_NDK_LATEST_HOME; fall back
# to the newest ndk/ under ANDROID_SDK_ROOT (the sdkmanager layout).
if [[ -z "${ANDROID_NDK_HOME:-}" && -z "${ANDROID_NDK_LATEST_HOME:-}" && -n "${ANDROID_SDK_ROOT:-${ANDROID_HOME:-}}" ]]; then
  SDK="${ANDROID_SDK_ROOT:-${ANDROID_HOME:-}}"
  NDK="$(ls -d "$SDK"/ndk/* 2>/dev/null | sort -V | tail -1 || true)"
  if [[ -n "$NDK" ]]; then
    export ANDROID_NDK_HOME="$NDK"
  fi
fi
if [[ -z "${ANDROID_NDK_HOME:-}" && -z "${ANDROID_NDK_LATEST_HOME:-}" ]]; then
  echo "error: no NDK found — set ANDROID_NDK_HOME (or ANDROID_SDK_ROOT with an ndk/ inside)" >&2
  exit 1
fi

NDK_ARGS=()
BUILD_ARGS=()
if [[ "$MODE" == "release" ]]; then
  # cargo-ndk 4.x: --release belongs to the cargo build invocation, not the
  # ndk wrapper (putting it before `build` errors out).
  BUILD_ARGS+=(--release)
fi
for target in "${TARGETS[@]}"; do
  NDK_ARGS+=(-t "$target")
done

echo "==> cargo ndk ${NDK_ARGS[*]} build ${BUILD_ARGS[*]} -p pdf-core-ffi"
cargo ndk "${NDK_ARGS[@]}" -o android/app/src/main/jniLibs build "${BUILD_ARGS[@]}" -p pdf-core-ffi

find android/app/src/main/jniLibs -name 'libpdf_core_ffi.so' -exec ls -lh {} \;
