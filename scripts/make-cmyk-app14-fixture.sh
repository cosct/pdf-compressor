#!/usr/bin/env bash
# Regenerate the Adobe APP14 CMYK JPEG fixtures under
# crates/pdf-core/assets/. Two 64×64 quadrant probes cover the convention
# dispute (PLAN-1.0 §3.2 B1):
#   cmyk-app14-plain.jpg     — Adobe APP14 marker + PLAIN ink samples
#   cmyk-app14-inverted.jpg  — Adobe APP14 marker + INVERTED samples
#                              (255 - ink, the Photoshop storage form)
# Renderers (poppler) and the engine's zune-jpeg path both un-invert
# Adobe-marked CMYK: the plain file must decode inverted and the inverted
# file correctly — pinned pixel-level by tests.rs.
#
# Requires: python3 with Pillow. Deterministic (no randomness).
set -euo pipefail
cd "$(dirname "$0")/.."
OUT=crates/pdf-core/assets

python3 - "$OUT" <<'PY'
import sys
from PIL import Image

out = sys.argv[1]
w = h = 64
# Logical ink layout: TL white (0,0,0,0), TR K-black (0,0,0,255),
# BL Y+M red (0,255,255,0), BR C+Y green (255,0,255,0).
def quadrant(x, y):
    if y < 32:
        return (0, 0, 0, 0) if x < 32 else (0, 0, 0, 255)
    return (0, 255, 255, 0) if x < 32 else (255, 0, 255, 0)

plain = Image.new("CMYK", (w, h))
inverted = Image.new("CMYK", (w, h))
for y in range(h):
    for x in range(w):
        c, m, yy, k = quadrant(x, y)
        plain.putpixel((x, y), (c, m, yy, k))
        inverted.putpixel((x, y), (255 - c, 255 - m, 255 - yy, 255 - k))

plain.save(f"{out}/cmyk-app14-plain.jpg", quality=95, subsampling=0)
inverted.save(f"{out}/cmyk-app14-inverted.jpg", quality=95, subsampling=0)
print("fixtures regenerated (both carry the Adobe APP14 marker)")
PY
