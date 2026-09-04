#!/usr/bin/env bash
# Build an Arch package from the committed tree into pdf-compressor-local/.
# 从已提交的代码树构建 Arch 本地安装包，产物输出到 pdf-compressor-local/。
#
# The source tarball is cut with `git archive HEAD`, so the package always
# contains exactly what is committed — no build artifacts, no nested copies
# from a previous run. The PKGBUILD is generated from the release template
# (packaging/archlinux/PKGBUILD) with the local source line and checksum
# substituted, keeping a single source of truth.
#
# Usage: scripts/build-arch-local.sh [--install]
#   --install  additionally install the built package via pkexec pacman -U
set -euo pipefail

cd "$(dirname "$0")/.."

version=$(sed -n 's/^version = "\(.*\)"$/\1/p' src-tauri/Cargo.toml | head -1)
if [[ -z "$version" ]]; then
  echo "error: could not read the version from src-tauri/Cargo.toml" >&2
  exit 1
fi

outdir="pdf-compressor-local"
mkdir -p "$outdir"

tarball="$outdir/pdf-compressor-${version}.tar.gz"
git archive --format=tar.gz --prefix="pdf-compressor-${version}/" HEAD -o "$tarball"
checksum=$(sha256sum "$tarball" | cut -d' ' -f1)
echo "==> source tarball: $tarball (sha256 $checksum)"

# Local PKGBUILD from the release template: point the source at the local
# tarball and pin its checksum.
sed \
  -e "s|source=(\"[^\"]*\")|source=(\"pdf-compressor-${version}.tar.gz\")|" \
  -e "s|sha256sums=('[^']*')|sha256sums=('${checksum}')|" \
  "packaging/archlinux/PKGBUILD" > "$outdir/PKGBUILD"

# makepkg unpacks and builds inside $outdir; the globs keep previous outputs
# out of the way of a fresh verification.
(cd "$outdir" && makepkg -sf)

# The debug companion package (OPTIONS=(debug)) is written last — exclude it
# so the reported path is the real installable package.
package=$(ls -t "$outdir"/pdf-compressor-*.pkg.tar.zst | grep -v -- '-debug-' | head -1)
echo "==> built: $package"

if [[ "${1:-}" == "--install" ]]; then
  pkexec pacman -U "$package"
fi
