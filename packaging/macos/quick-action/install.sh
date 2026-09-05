#!/usr/bin/env bash
# Install the "Compress with PDF Compressor" Quick Action for Finder.
# 为访达安装"Compress with PDF Compressor"快速操作（右键菜单）。
#
# Usage / 用法:
#   ./install.sh [path-to-pdf-compressor-cli] [--uninstall]
#
# Without an argument the service calls the CLI bundled inside the app
# (/Applications/PDF Compressor.app/Contents/Resources/binaries/).
# Pass an explicit path if the app lives elsewhere.
set -euo pipefail

SERVICES_DIR="${HOME}/Library/Services"
WORKFLOW_NAME="Compress with PDF Compressor"
HERE=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)

if [ "${1:-}" = "--uninstall" ]; then
  rm -rf "${SERVICES_DIR:?}/${WORKFLOW_NAME}.workflow"
  echo "Removed the Quick Action."
  exit 0
fi

CLI_PATH="${1:-/Applications/PDF Compressor.app/Contents/Resources/binaries/pdf-compressor-cli}"
if [ ! -x "$CLI_PATH" ]; then
  echo "warning: '$CLI_PATH' not found — install the app first, or pass its CLI path." >&2
  echo "         The service is installed anyway and can be re-run with a path later." >&2
fi

mkdir -p "$SERVICES_DIR"
TARGET="${SERVICES_DIR}/${WORKFLOW_NAME}.workflow"
rm -rf "$TARGET"
cp -R "${HERE}/${WORKFLOW_NAME}.workflow" "$TARGET"

# Bake the resolved CLI path into the RunShellScript source (the template
# carries a @@CLI_PATH@@ placeholder; plist-safe escaping only needs &).
ESC_PATH=${CLI_PATH//&/&amp;}
/usr/bin/sed -i '' "s|@@CLI_PATH@@|${ESC_PATH}|g" "$TARGET/Contents/document.wfdesc"

# Refresh the services registry so the action shows up right away.
/System/Library/CoreServices/pbs -update >/dev/null 2>&1 || true

echo "Installed: $TARGET"
echo "Finder right-click → Quick Actions → ${WORKFLOW_NAME}"
echo "(enable it in System Settings → Keyboard → Keyboard Shortcuts → Services if hidden)"
