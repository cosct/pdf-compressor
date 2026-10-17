#!/usr/bin/env python3
"""Deterministic benchmark corpus for the engine (docs/BENCHMARK.md).

Writes PDFs + manifest.json into the output directory (default
target/bench/corpus). Regeneration is byte-stable per fixture: same seeds,
same encoders, so input hashes are comparable across machines only when the
PIL/jpeg stack matches — record them in the manifest and compare on reruns.
"""
import hashlib
import io
import json
import sys
from pathlib import Path

import numpy as np
from PIL import Image


def photo_page(width, height, seed):
    """A photo-like scanned page: smooth gradient + sensor noise, JPEG q95."""
    rng = np.random.default_rng(seed)
    x = np.linspace(0, 255, width, dtype=np.float32)[None, :]
    y = np.linspace(0, 255, height, dtype=np.float32)[:, None]
    base = np.stack(
        [
            np.broadcast_to(x, (height, width)),
            np.broadcast_to(y, (height, width)),
            (x + y) / 2,
        ],
        axis=-1,
    )
    noise = rng.normal(0, 22, base.shape).astype(np.float32)
    pixels = np.clip(base + noise, 0, 255).astype(np.uint8)
    return Image.fromarray(pixels, "RGB")


def bw_page(width, height, seed):
    """A near-bilevel text-scan page: dark strokes on light paper."""
    rng = np.random.default_rng(seed)
    pixels = np.full((height, width, 3), 238, dtype=np.uint8)
    for _ in range(90):
        row = rng.integers(0, height)
        length = rng.integers(width // 4, width - 40)
        thickness = rng.integers(2, 9)
        shade = rng.integers(0, 70)
        pixels[row : row + thickness, 20 : 20 + length] = shade
    pixels = np.clip(pixels.astype(np.int16) + rng.normal(0, 3, pixels.shape), 0, 255)
    return Image.fromarray(pixels.astype(np.uint8), "RGB")


def save_pdf(pages, path, quality=95):
    pages[0].save(path, "PDF", resolution=150.0, save_all=True, append_images=pages[1:], quality=quality)


def build_text_pdf(pages, path):
    """Minimal text-only PDF with `pages` Helvetica pages."""
    bodies = []

    def add(body):
        bodies.append(body)
        return len(bodies)

    font = add(b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>")
    page_ids = []
    content_ids = []
    for i in range(pages):
        stream = f"BT /F1 18 Tf 72 720 Td (Benchmark text page {i + 1}) Tj ET".encode()
        content_ids.append(add(b"<< /Length " + str(len(stream)).encode() + b" >>\nstream\n" + stream + b"\nendstream"))
        page_ids.append(add(b""))
    pages_id = add(b"")
    catalog = add(b"<< /Type /Catalog /Pages " + f"{pages_id} 0 R".encode() + b" >>")
    kids = b" ".join(f"{pid} 0 R".encode() for pid in page_ids)
    bodies[pages_id - 1] = b"<< /Type /Pages /Count " + str(pages).encode() + b" /Kids [" + kids + b"] >>"
    for index, pid in enumerate(page_ids):
        bodies[pid - 1] = (
            b"<< /Type /Page /Parent " + f"{pages_id} 0 R".encode()
            + b" /MediaBox [0 0 612 792] /Resources << /Font << /F1 "
            + f"{font} 0 R".encode() + b" >> >> /Contents "
            + f"{content_ids[index]} 0 R".encode() + b" >>"
        )
    out = io.BytesIO()
    out.write(b"%PDF-1.4\n")
    offsets = [0]
    for number, body in enumerate(bodies, start=1):
        offsets.append(out.tell())
        out.write(f"{number} 0 obj\n".encode() + body + b"\nendobj\n")
    xref = out.tell()
    out.write(f"xref\n0 {len(bodies) + 1}\n".encode())
    out.write(b"0000000000 65535 f \n")
    for offset in offsets[1:]:
        out.write(f"{offset:010d} 00000 n \n".encode())
    out.write(
        b"trailer\n<< /Size " + str(len(bodies) + 1).encode()
        + b" /Root " + f"{catalog} 0 R".encode() + b" >>\nstartxref\n"
        + str(xref).encode() + b"\n%%EOF\n"
    )
    path.write_bytes(out.getvalue())


def sha256(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main():
    out = Path(sys.argv[1] if len(sys.argv) > 1 else "target/bench/corpus")
    out.mkdir(parents=True, exist_ok=True)
    fixtures = {}

    def register(name, path, kind):
        fixtures[name] = {
            "file": path.name,
            "kind": kind,
            "bytes": path.stat().st_size,
            "sha256": sha256(path),
        }
        print(f"{name}: {path.stat().st_size / 1e6:.2f} MB")

    for name, pages, size in [
        ("scan-photo-small", 3, (1600, 2200)),
        ("scan-photo-medium", 10, (2400, 3200)),
        ("scan-photo-large", 20, (2400, 3200)),
    ]:
        path = out / f"{name}.pdf"
        save_pdf([photo_page(*size, seed=1000 + i) for i in range(pages)], path)
        register(name, path, "scan-photo")

    path = out / "scan-bw-medium.pdf"
    save_pdf([bw_page(2000, 2800, seed=2000 + i) for i in range(10)], path, quality=90)
    register("scan-bw-medium", path, "scan-bw")

    path = out / "mixed-medium.pdf"
    save_pdf([photo_page(1600, 1200, seed=3000 + i) for i in range(6)], path)
    register("mixed-medium", path, "mixed")

    path = out / "text-small.pdf"
    build_text_pdf(5, path)
    register("text-small", path, "text")

    for pages in (1, 5, 10, 20, 40, 80):
        path = out / f"scale-{pages:02d}.pdf"
        # Unique image per page: dedup cannot collapse the work, so decode
        # and cache pressure scale with the true page count.
        save_pdf([photo_page(2400, 3200, seed=4000 + i) for i in range(pages)], path)
        register(f"scale-{pages:02d}", path, "scale")

    path = out / "corrupt.pdf"
    rng = np.random.default_rng(99)
    path.write_bytes(b"%PDF-1.4\n" + rng.bytes(2 * 1024 * 1024))
    register("corrupt", path, "corrupt")

    (out / "manifest.json").write_text(json.dumps(fixtures, indent=2) + "\n")
    print(f"manifest: {out / 'manifest.json'}")


if __name__ == "__main__":
    main()
