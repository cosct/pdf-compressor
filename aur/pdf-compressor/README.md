# AUR packaging for PDF Compressor

This directory contains the AUR source package files for `pdf-compressor`.

Before publishing to AUR:

1. Push the project to the upstream URL in `PKGBUILD`.
2. Create a `v0.2.0` release tag, or adjust `source` to match the actual release archive URL.
3. Replace `sha256sums=('SKIP')` with the real release archive checksum:

   ```bash
   updpkgsums
   ```

4. Regenerate `.SRCINFO`:

   ```bash
   makepkg --printsrcinfo > .SRCINFO
   ```

5. Build-check the source package:

   ```bash
   makepkg -sf
   ```

For a local test before the upstream release archive exists, create a matching source tarball manually from the project parent directory:

```bash
git archive --format=tar.gz --prefix=pdf-compressor-0.2.0/ -o pdf-compressor-0.2.0.tar.gz HEAD
mv pdf-compressor-0.2.0.tar.gz aur/pdf-compressor/
```
