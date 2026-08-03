#!/usr/bin/env bash
#
# Regenerate every application icon from the two master SVGs.
#
# Run this after editing assets/icons/sicompass.svg or sicompass-small.svg,
# then commit the regenerated files.
#
# The outputs are committed rather than generated at build time. They change
# about once a year, and requiring librsvg, imagemagick, icoutils and libicns
# on four CI runners plus every contributor's machine to build a Rust app
# would be a poor trade. This mirrors how shaders/ and THIRD-PARTY-LICENSES
# .html are handled.
#
# Where each output is consumed:
#   sicompass.ico   -> the .exe, via winresource in src/sicompass/build.rs
#   Product.ico     -> the .msi, via src/sicompass/wix/main.wxs
#   sicompass.icns  -> the macOS .app and .dmg, via cargo-packager
#   *.png           -> cargo-packager's `icons` list, which installs them into
#                      /usr/share/icons/hicolor/<size>/apps/ in the .deb and
#                      AppImage, and the same paths in the .rpm and the Nix
#                      derivation
#   sicompass.svg   -> /usr/share/icons/hicolor/scalable/apps/
#
# Tools come from the flake dev shell. Run inside `nix develop`, or prefix
# with `nix develop -c`.

set -euo pipefail

cd "$(dirname "$0")/.."

for tool in rsvg-convert icotool png2icns; do
    if ! command -v "$tool" >/dev/null 2>&1; then
        echo "error: $tool not found. Run inside 'nix develop'." >&2
        exit 1
    fi
done

OUT=assets/icons
FULL=$OUT/sicompass.svg
SMALL=$OUT/sicompass-small.svg

# Below 48 pixels the full-detail artwork's 7-unit outlines fall under a pixel
# and grey out into a smudge, so the simplified solid-key variant is used
# instead. Both share a layout, so the icon gains detail as it grows rather
# than changing shape.
for s in 16 22 24 32; do
    rsvg-convert -w "$s" -h "$s" "$SMALL" -o "$OUT/${s}x${s}.png"
done
for s in 48 64 128 256 512 1024; do
    rsvg-convert -w "$s" -h "$s" "$FULL" -o "$OUT/${s}x${s}.png"
done

# cargo-packager and the freedesktop icon spec both want an explicit @2x for
# the 128 slot. It is the 256 render, not an upscale.
cp "$OUT/256x256.png" "$OUT/128x128@2x.png"

# Windows .ico. 256 is stored PNG-compressed by icotool automatically; the
# rest are raw. Vista and later pick the best slot themselves.
icotool -c -o "$OUT/sicompass.ico" \
    "$OUT/16x16.png" "$OUT/24x24.png" "$OUT/32x32.png" \
    "$OUT/48x48.png" "$OUT/64x64.png" "$OUT/128x128.png" "$OUT/256x256.png"

# macOS .icns. png2icns only accepts the sizes Apple defines slots for, so
# 22, 24 and 64 are deliberately absent from this list.
png2icns "$OUT/sicompass.icns" \
    "$OUT/16x16.png" "$OUT/32x32.png" "$OUT/128x128.png" \
    "$OUT/256x256.png" "$OUT/512x512.png" "$OUT/1024x1024.png"

# WiX reads the icon from beside main.wxs.
mkdir -p src/sicompass/wix
cp "$OUT/sicompass.ico" src/sicompass/wix/Product.ico

echo "Regenerated:"
find "$OUT" -maxdepth 1 -type f -printf '  %f\n' | sort
echo "  src/sicompass/wix/Product.ico"
