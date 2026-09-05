#!/usr/bin/env bash
# Nautilus/Nemo script: compress the selected scanned PDFs as grayscale.
# Nautilus/Nemo 脚本：以灰度模式压缩选中的扫描件 PDF。
set -euo pipefail

CLI="${PDF_COMPRESSOR_CLI:-pdf-compressor-cli}"
[ -x "$(command -v "$CLI")" ] || exit 1

mapfile -t FILES <<<"$NAUTILUS_SCRIPT_SELECTED_FILE_PATHS"
ARGS=()
for file in "${FILES[@]}"; do
  [ -n "$file" ] && ARGS+=("$file")
done
[ "${#ARGS[@]}" -gt 0 ] || exit 0

"$CLI" quick --no-notify --grayscale "${ARGS[@]}"
