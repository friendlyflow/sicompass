#!/usr/bin/env bash
#
# Make a PNG-capable FreeType available to freetype-sys, and print the
# PKG_CONFIG_PATH additions needed to find it.
#
# ## Why this exists
#
# NotoColorEmoji stores its strikes as PNG bitmaps, so FreeType can only
# rasterise them when built with FT_CONFIG_OPTION_USE_PNG. freetype-sys 0.20.1
# links the system FreeType when pkg-config reports `freetype2 >= 24.3.18`,
# and otherwise falls back to a vendored `cc` build that has PNG support
# switched off. That fallback does not error: it returns zero-sized bitmaps,
# so every colour emoji silently disappears.
#
# `freetype2.pc` carries a libtool version, not the release version, so the
# 24.3.18 floor is higher than it looks:
#
#     FreeType 2.11.1 (ubuntu 22.04) -> 24.1.18   too old, falls back
#     FreeType 2.13.2 (ubuntu 24.04) -> 26.1.20   fine
#     FreeType 2.13.3                -> 26.2.20   fine
#
# The release builds on ubuntu-22.04 for its glibc 2.35 floor, which is older
# than Debian 12 and worth keeping, so on Linux a new enough FreeType is built
# from source here rather than taken from apt.
#
# It is built static, so the resulting binary does not depend on a
# /usr/local/lib copy that will not exist on a user's machine. libpng and zlib
# stay dynamic: they are present on every desktop Linux and are declared in
# the .deb and .rpm dependencies.
#
# macOS and Windows do not need a source build. Homebrew's and vcpkg's
# FreeType are both recent and both already have PNG support.
#
# ## Usage
#
#     eval "$(scripts/setup-freetype.sh)"
#
# or in a GitHub workflow:
#
#     scripts/setup-freetype.sh >> "$GITHUB_ENV"
#
# Everything informational goes to stderr, so stdout is only `KEY=value` lines.

set -euo pipefail

FREETYPE_VERSION="${FREETYPE_VERSION:-2.13.3}"
PREFIX="${FREETYPE_PREFIX:-$HOME/.local/freetype}"

log() { echo "$@" >&2; }

case "$(uname -s)" in
    Darwin)
        # Homebrew's freetype is 2.13+ and is built against its libpng.
        brew list freetype >/dev/null 2>&1 || brew install freetype
        brew list pkgconf >/dev/null 2>&1 || brew install pkgconf
        ft_prefix="$(brew --prefix freetype)"
        log "Using Homebrew FreeType at $ft_prefix"
        echo "PKG_CONFIG_PATH=$ft_prefix/lib/pkgconfig:${PKG_CONFIG_PATH:-}"
        ;;

    Linux)
        if [ -x "$PREFIX/lib/pkgconfig/../../bin/freetype-config" ] \
            || [ -f "$PREFIX/lib/pkgconfig/freetype2.pc" ]; then
            log "Reusing FreeType already built at $PREFIX"
        else
            log "Building FreeType $FREETYPE_VERSION from source into $PREFIX"
            tmp="$(mktemp -d)"
            trap 'rm -rf "$tmp"' EXIT

            # Savannah is the upstream home but returns 502 often enough to
            # break a build, so SourceForge is tried first and Savannah is the
            # fallback. Both serve the identical release tarball.
            downloaded=""
            for url in \
                "https://downloads.sourceforge.net/project/freetype/freetype2/${FREETYPE_VERSION}/freetype-${FREETYPE_VERSION}.tar.xz" \
                "https://download.savannah.gnu.org/releases/freetype/freetype-${FREETYPE_VERSION}.tar.xz"
            do
                log "Fetching $url"
                if curl -fsSL --proto '=https' --tlsv1.2 --retry 3 --retry-delay 2 \
                    "$url" -o "$tmp/freetype.tar.xz"; then
                    downloaded="$url"
                    break
                fi
                log "  failed, trying the next mirror"
            done
            [ -n "$downloaded" ] || { log "error: could not download FreeType ${FREETYPE_VERSION}"; exit 1; }

            tar -xf "$tmp/freetype.tar.xz" -C "$tmp"

            # PNG and zlib are REQUIRE rather than optional on purpose: this
            # script exists to guarantee PNG support, so a silent "libpng not
            # found, carrying on without it" would defeat the whole point.
            # HarfBuzz, Brotli and bzip2 are off because nothing here uses
            # them and each is another dependency to install on three runners.
            cmake -S "$tmp/freetype-${FREETYPE_VERSION}" -B "$tmp/build" \
                -DCMAKE_BUILD_TYPE=Release \
                -DCMAKE_INSTALL_PREFIX="$PREFIX" \
                -DCMAKE_INSTALL_LIBDIR=lib \
                -DCMAKE_POSITION_INDEPENDENT_CODE=ON \
                -DBUILD_SHARED_LIBS=OFF \
                -DFT_REQUIRE_PNG=ON \
                -DFT_REQUIRE_ZLIB=ON \
                -DFT_DISABLE_HARFBUZZ=ON \
                -DFT_DISABLE_BROTLI=ON \
                -DFT_DISABLE_BZIP2=ON >&2
            cmake --build "$tmp/build" -j"$(nproc)" >&2
            cmake --install "$tmp/build" >&2
        fi

        pc="$PREFIX/lib/pkgconfig/freetype2.pc"
        [ -f "$pc" ] || { log "error: $pc was not produced"; exit 1; }
        log "freetype2.pc version: $(PKG_CONFIG_PATH="$PREFIX/lib/pkgconfig" pkg-config --modversion freetype2)"
        echo "PKG_CONFIG_PATH=$PREFIX/lib/pkgconfig:${PKG_CONFIG_PATH:-}"
        ;;

    *)
        log "error: unsupported platform $(uname -s). Windows uses scripts/setup-freetype.ps1."
        exit 1
        ;;
esac
