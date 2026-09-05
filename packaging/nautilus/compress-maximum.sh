#!/usr/bin/env bash
# Nautilus/Nemo script: compress the selected PDFs with the maximum preset.
# Nautilus/Nemo 脚本：以最大化预设压缩选中的 PDF。
set -euo pipefail

CLI="${PDF_COMPRESSOR_CLI:-pdf-compressor-cli}"
[ -x "$(command -v "$CLI")" ] || exit 1

mapfile -t FILES <<<"$NAUTILUS_SCRIPT_SELECTED_FILE_PATHS"
ARGS=()
for file in "${FILES[@]}"; do
  [ -n "$file" ] && ARGS+=("$file")
done
[ "${#ARGS[@]}" -gt 0 ] || exit 0

"$CLI" quick --no-notify --preset maximum "${ARGS[@]}"
