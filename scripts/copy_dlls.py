#!/usr/bin/env python3
"""Copy Windows runtime DLLs to the build directory for sicompass.exe."""
import shutil
import sys
import os

build_dir = sys.argv[1]

VCPKG_BIN = r"C:\vcpkg\installed\x64-windows\bin"
MSYS2_BIN = r"C:\msys64\mingw64\bin"

# DLLs from vcpkg release bin
VCPKG_DLLS = [
    "SDL3.dll",
    "freetype.dll",
    "harfbuzz.dll",
    "json-c.dll",
    "libwebp.dll",
    "libsharpyuv.dll",
    "utf8proc.dll",
    "brotlicommon.dll",
    "brotlidec.dll",
    "bz2.dll",
    "libcurl.dll",
    "libpng16.dll",
    "lexbor.dll",
    "zlib1.dll",
]

# DLLs from MSYS2 (renamed)
MSYS2_DLLS = [
    ("libaccesskit-c-0.17.dll", "accesskit.dll"),
]

SOURCE_ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))

copied = []
for dll in VCPKG_DLLS:
    src = os.path.join(VCPKG_BIN, dll)
    dst = os.path.join(build_dir, dll)
    if os.path.exists(src):
        shutil.copy2(src, dst)
        copied.append(dll)
    else:
        print(f"WARNING: {src} not found", file=sys.stderr)

for src_name, dst_name in MSYS2_DLLS:
    src = os.path.join(MSYS2_BIN, src_name)
    dst = os.path.join(build_dir, dst_name)
    if os.path.exists(src):
        shutil.copy2(src, dst)
        copied.append(dst_name)
    else:
        print(f"WARNING: {src} not found", file=sys.stderr)

for asset_dir in ("fonts", "shaders"):
    src = os.path.join(SOURCE_ROOT, asset_dir)
    dst = os.path.join(build_dir, asset_dir)
    if os.path.isdir(src):
        if os.path.isdir(dst):
            shutil.rmtree(dst)
        shutil.copytree(src, dst)
        copied.append(asset_dir + "/")
    else:
        print(f"WARNING: {src} not found", file=sys.stderr)

print(f"Copied {len(copied)} items to {build_dir}")
