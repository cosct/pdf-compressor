#!/usr/bin/env bash
# Nautilus/Nemo script: compress the selected scanned PDFs as lossless
# CCITT Group 4 (best for black-and-white text scans).
# Nautilus/Nemo 脚本：以无损 CCITT G4 压缩选中的黑白文字扫描件。
set -euo pipefail

CLI="${PDF_COMPRESSOR_CLI:-pdf-compressor-cli}"
[ -x "$(command -v "$CLI")" ] || exit 1

mapfile -t FILES <<<"$NAUTILUS_SCRIPT_SELECTED_FILE_PATHS"
ARGS=()
for file in "${FILES[@]}"; do
  [ -n "$file" ] && ARGS+=("$file")
done
[ "${#ARGS[@]}" -gt 0 ] || exit 0

"$CLI" quick --no-notify --bilevel g4 "${ARGS[@]}"
