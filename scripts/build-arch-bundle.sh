#!/usr/bin/env bash
# Produce the Arch .pkg.tar.zst from an already-completed `tauri build`
# release build, placing it next to the deb/appimage bundles.
# 从 tauri build 已完成的 release 产物直接生成 Arch zst 包，
# 输出到 <target>/release/bundle/archlinux/（与 deb/appimage 同级）。
#
# Tauri has no pacman bundle target and no afterBundleCommand hook
# (tauri-apps/tauri#9767), so this script is the post-bundle step that
# `pnpm run tauri:arch` chains after `tauri build`. No recompilation:
# the GUI binary and CLI are taken from the cargo target directory.
#
# Usage: scripts/build-arch-bundle.sh   (assumes `tauri build` already ran)
set -euo pipefail

cd "$(dirname "$0")/.."

command -v makepkg >/dev/null 2>&1 || {
  echo "error: makepkg not found — this script is Arch-only" >&2
  exit 1
}

version=$(sed -n 's/^version = "\(.*\)"$/\1/p' src-tauri/Cargo.toml | head -1)
[[ -n "$version" ]] || { echo "error: could not read the version from src-tauri/Cargo.toml" >&2; exit 1; }

# Resolve the cargo target dir instead of assuming ./target — a machine-wide
# shared target (build.target-dir in ~/.cargo/config.toml) redirects it. The
# CI packaging container has no Rust toolchain; the release workflow lays the
# binaries out at ./target there, so fall back to that when cargo is absent.
# Always absolute: makepkg runs package() from its own staging directory, so
# a relative path would resolve against $srcdir and miss the binaries.
if command -v cargo >/dev/null 2>&1; then
  target=$(cargo metadata --format-version 1 --no-deps | sed -n 's/.*"target_directory":"\([^"]*\)".*/\1/p')
fi
[[ -n "${target:-}" ]] || target="$PWD/target"
case "$target" in /*) ;; *) target="$PWD/$target" ;; esac

for bin in app pdf-compressor-cli; do
  [[ -x "$target/release/$bin" ]] || {
    echo "error: $target/release/$bin is missing — run \`pnpm run tauri:arch\` or \`tauri build\` first" >&2
    exit 1
  }
done

stage=$(mktemp -d)
trap 'rm -rf "$stage"' EXIT

# A no-build PKGBUILD: metadata mirrors packaging/archlinux/PKGBUILD (the AUR
# source package); package() installs from the completed build artifacts.
cat > "$stage/PKGBUILD" <<EOF
pkgname=pdf-compressor
pkgver=$version
pkgrel=1
pkgdesc='Local-first desktop PDF compressor built with Vue, Tauri, and Rust (GUI + headless CLI)'
arch=('x86_64')
url='https://github.com/cosct/pdf-compressor'
license=('MIT')
depends=('gtk3' 'webkit2gtk-4.1' 'hicolor-icon-theme')
optdepends=('libnotify: desktop notifications for the right-click quick-compress mode')
# Artifacts come from the completed tauri build — nothing is compiled here.
options=(!debug)

build() { :; }

package() {
  local root='$PWD' target='$target'
  install -Dm755 "\$target/release/app" "\$pkgdir/usr/bin/pdf-compressor"
  install -Dm755 "\$target/release/pdf-compressor-cli" "\$pkgdir/usr/bin/pdf-compressor-cli"
  install -Dm644 "\$root/src-tauri/icons/128x128.png" "\$pkgdir/usr/share/icons/hicolor/128x128/apps/pdf-compressor.png"
  install -Dm644 "\$root/src-tauri/icons/32x32.png" "\$pkgdir/usr/share/icons/hicolor/32x32/apps/pdf-compressor.png"
  install -Dm644 "\$root/README.md" "\$pkgdir/usr/share/doc/\$pkgname/README.md"
  install -Dm644 "\$root/README.zh-CN.md" "\$pkgdir/usr/share/doc/\$pkgname/README.zh-CN.md"
  install -Dm644 "\$root/packaging/pdf-compressor.desktop" "\$pkgdir/usr/share/applications/pdf-compressor.desktop"
  install -Dm644 "\$root/packaging/servicemenus/pdf-compressor.desktop" "\$pkgdir/usr/share/kio/servicemenus/pdf-compressor.desktop"
}
EOF

(cd "$stage" && makepkg -fd >/dev/null)

outdir="$target/release/bundle/archlinux"
mkdir -p "$outdir"
pkg=$(ls -t "$stage"/pdf-compressor-*.pkg.tar.zst | grep -v -- '-debug-' | head -1)
# Stable asset name for GitHub Releases (mirrors the deb's _amd64 naming);
# the AUR -bin PKGBUILD's source URL points at this file.
asset="$outdir/pdf-compressor_${version}_amd64.pkg.tar.zst"
cp "$pkg" "$asset"
echo "==> $asset"
