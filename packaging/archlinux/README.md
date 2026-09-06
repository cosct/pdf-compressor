# AUR packaging for PDF Compressor

Two AUR packages are maintained from this directory:

- `pdf-compressor-bin` — the **binary** package. Its `source` points at the
  `.pkg.tar.zst` published on GitHub Releases, so installing needs no
  Rust/Node toolchain and no compilation. Template: `PKGBUILD`.
- `pdf-compressor` — the **source** package. Builds the GUI and headless CLI
  from the tagged source tarball with cargo + pnpm. Template:
  `PKGBUILD.source`.

Both `provides`/`conflicts` on the same name, so switching between them
replaces the other cleanly. The zst shipped by `-bin` is produced by
`scripts/build-arch-bundle.sh` — the same makepkg pipeline as a local
`pnpm run tauri:arch` — and both templates install the same file layout
(GUI + CLI, desktop entries, Dolphin service menu, Nautilus/Nemo scripts),
so the packages cannot drift from the deb/appimage bundles.

## Automated publish (aur-publish workflow)

Tag builds (`.github/workflows/release.yml`, `arch-package` job) attach the
zst to a **draft** release. AUR updates wait for `.github/workflows/aur-publish.yml`,
which fires on `release.published`:

1. The release assets are verified to download **anonymously** — the `-bin`
   package's source URL 404s while the release is still a draft, so pushing
   the new pkgver any earlier would break AUR installs.
2. Both templates are rendered with the real `pkgver`/`sha256sums` (the zst
   hash for `-bin`, the tag-tarball hash for the source package), `.SRCINFO`
   is regenerated, and both are pushed to
   `ssh://aur@aur.archlinux.org/<package>.git`. Cloning a not-yet-existing
   AUR package yields an empty repo — the first push creates it.

### Required secret

`AUR_SSH_PRIVATE_KEY` — an OpenSSH private key whose public half is registered
under the AUR account (*My Account → SSH keys*):

```bash
ssh-keygen -t ed25519 -f aur_key -N ''
# aur_key.pub  → AUR account settings
# aur_key      → repository secret AUR_SSH_PRIVATE_KEY (full file contents)
```

Without the secret the tag workflow still builds and attaches the zst, but
the AUR push is skipped (forks work out of the box).

## Manual publish (fallback)

```bash
# fill pkgver + sha256sums in the template, then:
makepkg --printsrcinfo > .SRCINFO
git clone ssh://aur@aur.archlinux.org/pdf-compressor-bin.git   # or pdf-compressor
cp PKGBUILD .SRCINFO pdf-compressor-bin/
cd pdf-compressor-bin && git add . && git commit -m "v<version>" && git push
```

## Local zst without AUR

`pnpm run tauri:arch` — produces the same package next to the deb/appimage
bundles in `target/release/bundle/archlinux/`.
