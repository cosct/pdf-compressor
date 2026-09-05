#!/usr/bin/env bash
# Nautilus/Nemo script: compress the selected PDFs with the quick profile
# saved in the desktop app's settings page.
# Nautilus/Nemo 脚本：使用应用设置页保存的快速配置压缩选中的 PDF。
set -euo pipefail

CLI="${PDF_COMPRESSOR_CLI:-pdf-compressor-cli}"
[ -x "$(command -v "$CLI")" ] || {
  notify-send -a "PDF Compressor" -u critical "PDF compression failed" \
    "pdf-compressor-cli is not installed or not on PATH" 2>/dev/null || true
  exit 1
}

# One path per line; spaces are safe because we read whole lines.
mapfile -t FILES <<<"$NAUTILUS_SCRIPT_SELECTED_FILE_PATHS"
ARGS=()
for file in "${FILES[@]}"; do
  [ -n "$file" ] && ARGS+=("$file")
done

[ "${#ARGS[@]}" -gt 0 ] || exit 0

# --no-notify: this script shows its own feedback (quiet mode avoids a
# notification race with the file manager's own popup).
"$CLI" quick --no-notify "${ARGS[@]}"
