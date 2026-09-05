#!/usr/bin/env bash
# Nautilus/Nemo script: compress the selected PDFs under 5 MB (the engine
# searches quality/edge parameters until the budget is met).
# Nautilus/Nemo 脚本：将选中的 PDF 压缩到 5MB 以内（自动搜索参数）。
set -euo pipefail

CLI="${PDF_COMPRESSOR_CLI:-pdf-compressor-cli}"
[ -x "$(command -v "$CLI")" ] || exit 1

mapfile -t FILES <<<"$NAUTILUS_SCRIPT_SELECTED_FILE_PATHS"
ARGS=()
for file in "${FILES[@]}"; do
  [ -n "$file" ] && ARGS+=("$file")
done
[ "${#ARGS[@]}" -gt 0 ] || exit 0

"$CLI" quick --no-notify --target-size 5MB "${ARGS[@]}"
