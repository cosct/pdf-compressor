# AUR packaging for PDF Compressor

`pdf-compressor-bin` — the AUR **binary** package. Its `source` points at the
`.pkg.tar.zst` published on GitHub Releases, so installing needs no Rust/Node
toolchain and no compilation. The zst itself is produced by
`scripts/build-arch-bundle.sh` — the same makepkg pipeline as a local
`pnpm run tauri:arch` — so the AUR package cannot drift from the
deb/appimage bundles.

> The former source package (`pdf-compressor`) is no longer updated;
> `pdf-compressor-bin` provides/conflicts the old name, so upgrading replaces
> it cleanly.

## Automated publish (release workflow)

On every `v*` tag (`.github/workflows/release.yml`, `arch-package` job):

1. The ubuntu build uploads the raw release binaries (GUI `app` +
   `pdf-compressor-cli`) as a workflow artifact.
2. An `archlinux:base-devel` container restores them into `target/release/`,
   runs `scripts/build-arch-bundle.sh`, and attaches
   `pdf-compressor_<version>_amd64.pkg.tar.zst` to the GitHub release.
3. The same job renders this `PKGBUILD` with the real `pkgver`/`sha256sums`,
   regenerates `.SRCINFO`, and pushes both to
   `ssh://aur@aur.archlinux.org/pdf-compressor-bin.git`. Cloning a not-yet-
   existing AUR package yields an empty repo — the first push creates it.

### Required secret

`AUR_SSH_PRIVATE_KEY` — an OpenSSH private key whose public half is registered
under the AUR account (*My Account → SSH keys*):

```bash
ssh-keygen -t ed25519 -f aur_key -N ''
# aur_key.pub  → AUR account settings
# aur_key      → repository secret AUR_SSH_PRIVATE_KEY (full file contents)
```

Without the secret the job still builds and attaches the zst, but skips the
AUR push (forks work out of the box).

## Manual publish (fallback)

```bash
# fill pkgver + sha256sums in PKGBUILD, then:
makepkg --printsrcinfo > .SRCINFO
git clone ssh://aur@aur.archlinux.org/pdf-compressor-bin.git
cp PKGBUILD .SRCINFO pdf-compressor-bin/
cd pdf-compressor-bin && git add . && git commit -m "v<version>" && git push
```

## Local zst without AUR

`pnpm run tauri:arch` — produces the same package next to the deb/appimage
bundles in `target/release/bundle/archlinux/`.
