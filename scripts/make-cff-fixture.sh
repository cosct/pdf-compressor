#!/usr/bin/env bash
# Regenerate the committed CID-keyed CFF fixture under
# crates/pdf-core/assets/. The committed bytes are the source of truth for
# the tests; run this only when the glyph set deliberately changes, then
# re-commit BOTH assets and update the TEST_CFF_CID_* constants in
# src/testutil.rs in lockstep (the exact CIDs come from the source font's
# charset and change with the --text argument).
#
# Requires: python3 with fonttools (e.g. `python3 -m venv /tmp/ftenv &&
# /tmp/ftenv/bin/pip install fonttools`) and a Source Han Serif CN OTF at
# /usr/share/fonts/adobe-source-han-serif/ (any CID-keyed CFF works — adjust
# SRC below).
set -euo pipefail

cd "$(dirname "$0")/.."
SRC=${SRC:-/usr/share/fonts/adobe-source-han-serif/SourceHanSerifCN-Regular.otf}
OUT=crates/pdf-core/assets
PY=${PY:-python3}

$PY - "$SRC" "$OUT" <<'EOF'
import struct
import subprocess
import sys
import tempfile
from pathlib import Path

src, out = Path(sys.argv[1]), Path(sys.argv[2])
text = "PDF压缩测试字体压缩器拉丁ABCDabcd0123456789"

with tempfile.TemporaryDirectory() as work:
    otf = Path(work) / "test-font-cid.otf"
    subprocess.run(
        [
            "pyftsubset", str(src),
            f"--text={text}",
            f"--output-file={otf}",
            "--no-hinting", "--desubroutinize",
            "--name-IDs=0,1,2,3,4,5,6",
        ],
        check=True,
    )

    data = otf.read_bytes()
    # Extract the bare CFF table for the CIDFontType0C asset.
    num_tables = struct.unpack(">H", data[4:6])[0]
    cff = None
    for i in range(num_tables):
        record = 12 + 16 * i
        if data[record:record + 4] == b"CFF ":
            offset, length = struct.unpack(">II", data[record + 8:record + 16])
            cff = data[offset:offset + length]
    assert cff is not None, "subset output has no CFF table"
    (out / "test-font-cid.cff").write_bytes(cff)
    (out / "test-font-cid.otf").write_bytes(data)

    from fontTools.ttLib import TTFont
    font = TTFont(otf)
    top = font["CFF "].cff[font["CFF "].cff.fontNames[0]]
    assert top.ROS == ("Adobe", "Identity", 0), f"unexpected ROS {top.ROS}"
    glyphs = font.getGlyphOrder()
    cmap = font.getBestCmap()
    print(f"numGlyphs: {len(glyphs)}  bare CFF: {len(cff)} bytes")
    for ch in "PDFABC":
        cid = int(cmap[ord(ch)][3:])
        gid = glyphs.index(cmap[ord(ch)])
        assert cid != gid, "fixture must keep a non-identity charset"
        print(f"TEST_CFF_CID_{ch}: {cid}  (gid {gid})")
EOF

echo "regenerated:"
ls -la "$OUT"/test-font-cid.*
