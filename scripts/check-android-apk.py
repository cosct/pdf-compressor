#!/usr/bin/env python3
"""Fail packaging if either supported ABI is missing its native engine."""
import sys
import zipfile

with zipfile.ZipFile(sys.argv[1]) as apk:
    for abi in ("arm64-v8a", "x86_64"):
        name = f"lib/{abi}/libpdf_core_ffi.so"
        info = apk.getinfo(name)
        if info.file_size == 0:
            raise SystemExit(f"Empty native engine: {name}")
        with apk.open(name) as library:
            if library.read(4) != b"\x7fELF":
                raise SystemExit(f"Invalid native engine: {name}")
        print(f"OK {name}: {info.file_size} bytes")
