#!/usr/bin/env bash
# Regenerate the committed JPX (JPEG 2000) fixture codestreams under
# crates/pdf-core/assets/. The committed bytes are the source of truth for
# the tests; run this only when a fixture pattern deliberately changes, then
# re-commit the assets (the Rust reference planes in src/testutil.rs must be
# updated in lockstep).
#
# Requires: python3 and opj_compress (the openjpeg CLI tools).
set -euo pipefail

cd "$(dirname "$0")/.."
OUT=crates/pdf-core/assets
WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT

python3 - "$WORK" <<'EOF'
import sys
work = sys.argv[1]

# RGB: 256x192 LCG noise over gradients — the exact pattern of
# testutil::fixture_rgb_image(256, 192) (seed 0x1234_5678), mirrored here so
# lossless decodes compare pixel-identical against the shared reference.
w, h = 256, 192
state = 0x1234_5678
rows = bytearray()
for y in range(h):
    for x in range(w):
        state = (state * 1664525 + 1013904223) & 0xFFFFFFFF
        noise = ((state >> 24) & 0xFF) - 128
        r = max(0, min(255, x * 255 // w + noise))
        g = max(0, min(255, y * 255 // h + noise))
        b = max(0, min(255, (x + y) * 255 // (w + h) + noise))
        rows.extend((r, g, b))
with open(f"{work}/rgb.ppm", "wb") as f:
    f.write(f"P6\n{w} {h}\n255\n".encode())
    f.write(bytes(rows))

# Gray: 256x192 mixed gradient (testutil::jpx_gray_reference).
with open(f"{work}/gray.pgm", "wb") as f:
    f.write(f"P5\n{w} {h}\n255\n".encode())
    f.write(bytes(((x * 7 + y * 13) & 0xFF) for y in range(h) for x in range(w)))

# Bilevel: 640x480 blocky deterministic pattern
# (testutil::jpx_bilevel_reference).
w, h = 640, 480
with open(f"{work}/bilevel.pbm", "wb") as f:
    f.write(f"P4\n{w} {h}\n".encode())
    rows = bytearray()
    for y in range(h):
        row, out = 0, bytearray()
        for x in range(w):
            bit = 1 if (((x * x + y * y) // 32) % 2 == 0) else 0
            row = (row << 1) | (1 - bit)  # PBM: 1 = black
            if (x + 1) % 8 == 0:
                out.append(row)
                row = 0
        rows.extend(out)
    f.write(bytes(rows))
print("patterns ok")
EOF

# Lossless (-r 0), single tile (-n 1): decoded planes compare pixel-identical
# against the reference planes in src/testutil.rs.
opj_compress -i "$WORK/rgb.ppm"      -o "$OUT/jpx-rgb.j2k"      -n 1 -r 0 >/dev/null
opj_compress -i "$WORK/rgb.ppm"      -o "$OUT/jpx-rgb.jp2"      -n 1 -r 0 >/dev/null
opj_compress -i "$WORK/gray.pgm"     -o "$OUT/jpx-gray.j2k"     -n 1 -r 0 >/dev/null
opj_compress -i "$WORK/bilevel.pbm"  -o "$OUT/jpx-bilevel.j2k"  -n 1 -r 0 >/dev/null

echo "regenerated:"
ls -la "$OUT"/jpx-*
