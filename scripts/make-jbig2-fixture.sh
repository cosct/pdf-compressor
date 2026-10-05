#!/usr/bin/env bash
# Regenerate the committed JBIG2 fixture under crates/pdf-core/assets/.
# The committed bytes are the source of truth for the tests; run this only
# when swapping the sample, then update `jbig2_scan_dimensions()` in
# src/testutil.rs in lockstep (the page size is printed below).
#
# Source: the JBIG2 conformance corpus mirror served by the hayro project
# (Apache-2.0 OR MIT). `power_jbig2/042_2.jb2` is a sequential-organization
# A4 scan at ~200dpi — the embedded (Annex D.3) form used by PDFs is the
# standalone file minus its variable-length file header.
set -euo pipefail

cd "$(dirname "$0")/.."
OUT=crates/pdf-core/assets
SRC_URL=${SRC_URL:-https://hayro-assets.dev/jbig2/power_jbig2/042_2.jb2}

curl -fsSL -o "$OUT/jbig2-scan-file.jbig2" "$SRC_URL"

python3 - "$OUT/jbig2-scan-file.jbig2" "$OUT/jbig2-scan.bin" <<'EOF'
import struct
import sys

src, dst = sys.argv[1], sys.argv[2]
data = open(src, "rb").read()
assert data[:8] == b"\x97JB2\r\n\x1a\n", "not a JBIG2 standalone file"
flags = data[8]
assert flags & 0x01, "sample must use the sequential organization (embedded parsing walks it)"
skip = 8 + 1 + (0 if flags & 0x02 else 4)
open(dst, "wb").write(data[skip:])
print(f"file header: {skip} bytes; embedded segments: {len(data) - skip} bytes")

# Page dimensions come from the first page-information segment (type 48).
i = skip
while i < len(data):
    seg_num = struct.unpack(">I", data[i : i + 4])[0]
    fl = data[i + 4]
    count_flags = data[i + 5]
    short_count = (count_flags >> 5) & 0x07
    assert short_count != 5 and short_count != 6, "invalid referred-to count"
    j = i + 6
    j += 0 if short_count == 7 else short_count * 7
    if short_count == 7:
        while data[j] & 0x80:
            j += 1
        j += 1
    j += 4 if fl & 0x40 else 1
    data_len = struct.unpack(">I", data[j : j + 4])[0]
    data_start = j + 4
    if (fl & 0x3F) == 48:
        w, h = struct.unpack(">II", data[data_start : data_start + 8])
        print(f"page dimensions: {w} x {h}  -> update jbig2_scan_dimensions()")
        break
    assert data_len not in (0, 0xFFFFFFFF), "unknown segment length mid-walk"
    i = data_start + data_len
EOF

echo "regenerated:"
ls -la "$OUT"/jbig2-scan.*
