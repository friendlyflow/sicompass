#!/usr/bin/env bash
#
# Build the FFmpeg the release packages link into the binary, and print the
# FFMPEG_DIR that points ffmpeg-sys-next at it.
#
# ## Why this exists
#
# Video plays inside the window, decoded by FFmpeg, and one binary has to work
# from an archive, a .deb, an .rpm, an AppImage and a .app with nothing beside it.
# So FFmpeg links statically, the `bundled-ffmpeg` feature. Neither apt (jammy's
# 4.4, dynamic) nor Homebrew (dylibs in an absolute keg path, the failure that
# broke 0.1.10 with FreeType) can provide that, so it is built here, once per
# cache, from a pinned release.
#
# ## What it builds
#
# Decoding only: no programs, no encoders, no muxers, no network, no devices. Every
# format the player is likely to meet is FFmpeg's own code, so no external codec
# library comes along. Hardware decoding is off, which keeps the binary free of
# VA-API, VDPAU and VideoToolbox link dependencies.
#
# It stays LGPL: the GPL and nonfree options are never passed. The license
# text ships in every package (licenses/LICENSE-FFmpeg.txt).
#
# Windows has no `sh` configure to run comfortably: scripts/setup-ffmpeg.ps1 takes
# vcpkg's static build instead.
#
# ## Usage
#
#     eval "$(scripts/setup-ffmpeg.sh)"
#
# or in a GitHub workflow:
#
#     scripts/setup-ffmpeg.sh >> "$GITHUB_ENV"
#
# Everything informational goes to stderr, so stdout is only `KEY=value` lines.

set -euo pipefail

FFMPEG_VERSION="${FFMPEG_VERSION:-9.0.2}"
PREFIX="${FFMPEG_PREFIX:-$HOME/.local/ffmpeg-$FFMPEG_VERSION}"

log() { echo "$@" >&2; }

if [ -f "$PREFIX/lib/libavcodec.a" ]; then
    log "Reusing FFmpeg already built at $PREFIX"
else
    log "Building FFmpeg $FFMPEG_VERSION from source into $PREFIX"
    tmp="$(mktemp -d)"
    trap 'rm -rf "$tmp"' EXIT

    curl -fsSL --proto '=https' --tlsv1.2 --retry 3 --retry-delay 2 \
        "https://ffmpeg.org/releases/ffmpeg-${FFMPEG_VERSION}.tar.xz" -o "$tmp/ffmpeg.tar.xz"
    tar -xf "$tmp/ffmpeg.tar.xz" -C "$tmp"

    jobs="$(getconf _NPROCESSORS_ONLN 2>/dev/null || echo 2)"
    (
        cd "$tmp/ffmpeg-${FFMPEG_VERSION}"
        # --disable-autodetect is the line that keeps this standalone: without it
        # configure links whatever it happens to find on the runner.
        ./configure \
            --prefix="$PREFIX" \
            --enable-static --disable-shared --enable-pic \
            --disable-autodetect \
            --disable-programs --disable-doc --disable-debug \
            --disable-avdevice --disable-avfilter \
            --disable-encoders --disable-muxers --disable-devices \
            --disable-filters --disable-network --disable-hwaccels \
            --disable-outdevs --disable-indevs >&2
        make -j"$jobs" >&2
        make install >&2
    )
fi

[ -f "$PREFIX/lib/libavcodec.a" ] || { log "error: $PREFIX/lib/libavcodec.a was not produced"; exit 1; }
# A package made with these would quietly stop being standalone.
if ls "$PREFIX"/lib/libav*.so* "$PREFIX"/lib/libav*.dylib >/dev/null 2>&1; then
    log "error: $PREFIX holds shared FFmpeg libraries, and the link would take them"
    exit 1
fi

echo "FFMPEG_DIR=$PREFIX"
