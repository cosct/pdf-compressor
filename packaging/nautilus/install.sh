#!/usr/bin/env sh
# Install the "PDF Compressor" submenu into Nautilus/Nemo's Scripts menu.
# 将 PDF 压缩脚本安装到 Nautilus/Nemo 的"脚本"右键菜单。
#
# Usage / 用法:
#   ./install.sh [--uninstall]
#
# The scripts call `pdf-compressor-cli quick` (installed system-wide by the
# Arch package, or available anywhere on $PATH) and show a desktop
# notification per run. Supported on GNOME Files (Nautilus) and Nemo; both
# expose the same $NAUTILUS_SCRIPT_SELECTED_FILE_PATHS contract.
set -eu

SCRIPT_NAMES="
compress-settings
compress-maximum
compress-scanned-grayscale
compress-scanned-g4
compress-target-5mb
"

detect_scripts_dir() {
  if command -v nemo >/dev/null 2>&1 && [ ! -d "${XDG_DATA_HOME:-$HOME/.local/share}/nautilus" ]; then
    printf '%s\n' "${XDG_DATA_HOME:-$HOME/.local/share}/nemo/scripts"
  else
    printf '%s\n' "${XDG_DATA_HOME:-$HOME/.local/share}/nautilus/scripts"
  fi
}

TARGET="$(detect_scripts_dir)/PDF Compressor"

if [ "${1:-}" = "--uninstall" ]; then
  rm -rf "$TARGET"
  echo "Removed: $TARGET"
  exit 0
fi

SCRIPT_DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
mkdir -p "$TARGET"
for name in $SCRIPT_NAMES; do
  install -m 755 "$SCRIPT_DIR/$name.sh" "$TARGET/$name"
done

echo "Installed PDF Compressor scripts to: $TARGET"
echo "Restart Nautilus (nautilus -q) or Nemo to see the 'Scripts > PDF Compressor' menu."
