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
# ## Why macOS additionally has to force a static link
#
# Being recent is not enough on macOS. Homebrew's keg holds both
# `libfreetype.a` and `libfreetype.6.dylib`, and a plain pkg-config probe picks
# the dylib, which stamps an absolute `/opt/homebrew/opt/freetype/lib/...` path
# into the binary. That path does not exist on a user's machine, so the app
# died at launch with a dyld "Library not loaded" abort, in 0.1.10, on every
# Mac without `brew install freetype`. `x86_64` is the same bug at
# `/usr/local/...`.
#
# The pkg-config crate emits `rustc-link-lib=static=` instead when
# `FREETYPE2_STATIC` is set, but only for libraries it can find a `.a` for,
# outside `/Library` and `/System` (its macOS notion of "system"). Both
# Homebrew prefixes qualify, so freetype and libpng link statically and only
# `-lz` and `-lbz2` stay dynamic, resolving against `/usr/lib`, which is on
# every macOS.
#
# The catch, and it is the same one `setup-freetype.ps1` documents at length:
# `--static` makes pkg-config resolve every entry in `Requires.private`, which
# for Homebrew's freetype2.pc is `zlib, bzip2, libpng`. macOS ships no
# `zlib.pc`, and Homebrew's bzip2 keg ships `libbz2.a` and no `bzip2.pc` at
# all, so the probe fails outright. freetype-sys swallows that failure and
# quietly takes its vendored PNG-less path, i.e. the exact silent
# emoji-disappearing regression the rest of this script exists to prevent.
# Hence the two generated .pc shims below, which describe the copies of zlib
# and bzip2 that macOS itself provides, and the static probe that refuses to
# continue if any of this stops holding.
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

# The same binary the pkg-config crate will shell out to, so this script and
# the build cannot disagree about what is resolvable.
PKG_CONFIG_BIN="${PKG_CONFIG:-pkg-config}"

# freetype2.pc carries a libtool version, so this is FreeType 2.11.3-ish and
# not, as it reads, some far-future release. Kept in one place because it is
# the number freetype-sys actually checks.
FLOOR="24.3.18"

# Fail here rather than letting the build quietly take the vendored path and
# produce a binary with no colour emoji, which is the failure this whole
# script exists to prevent.
verify() {
    local path="$1"
    if ! PKG_CONFIG_PATH="$path" "$PKG_CONFIG_BIN" --exists freetype2 2>/dev/null; then
        log "error: pkg-config cannot see freetype2 under $path"
        log "       (is pkg-config itself installed?)"
        exit 1
    fi
    local found
    found="$(PKG_CONFIG_PATH="$path" "$PKG_CONFIG_BIN" --modversion freetype2)"
    if ! PKG_CONFIG_PATH="$path" "$PKG_CONFIG_BIN" --atleast-version="$FLOOR" freetype2; then
        log "error: freetype2.pc reports $found, below the $FLOOR floor freetype-sys checks."
        log "       It would be ignored in favour of a vendored build without PNG support."
        exit 1
    fi
    log "freetype2.pc at $path reports $found (floor $FLOOR): OK"
}

# `--exists` and `--modversion` succeeding proves very little. freetype-sys
# calls `pkg_config::Config::find()`, and `atleast_version` makes that crate
# append the constraint as a *second, space-containing argument* next to the
# package name:
#
#     pkg-config --libs --cflags freetype2 "freetype2 >= 24.3.18"
#
# Under FREETYPE2_STATIC it additionally passes `--static`, which pulls in
# every Requires.private entry. Either can fail where the simpler probes pass,
# and freetype-sys swallows the error and falls back. So probe the exact query
# the build will make. Same reasoning as Test-Constrained in setup-freetype.ps1.
verify_static() {
    local path="$1" out
    if ! out="$(PKG_CONFIG_PATH="$path" "$PKG_CONFIG_BIN" --print-errors --static \
        --libs --cflags freetype2 "freetype2 >= $FLOOR" 2>&1)"; then
        log "error: the version-constrained static query freetype-sys makes fails:"
        log "         $out"
        log "       freetype-sys swallows that and falls back to its vendored build"
        log "       without PNG support, so colour emoji would silently vanish."
        exit 1
    fi
    log "static freetype2 flags: $out"
}

case "$(uname -s)" in
    Darwin)
        # Homebrew's freetype is 2.13+ and is built against its libpng.
        brew list freetype >/dev/null 2>&1 || brew install freetype
        command -v "$PKG_CONFIG_BIN" >/dev/null 2>&1 || brew install pkgconf
        ft_prefix="$(brew --prefix freetype)"
        # libpng is a freetype dependency, so installing freetype installs it.
        # Named explicitly rather than left to pkg-config's built-in search
        # path, which varies with how the pkg-config in use was built.
        png_prefix="$(brew --prefix libpng)"
        log "Using Homebrew FreeType at $ft_prefix"

        # Without the archive there is nothing for the pkg-config crate to mark
        # static, so it would emit a plain `-lfreetype`, the linker would pick
        # libfreetype.6.dylib, and the binary would carry an absolute Homebrew
        # path that is not there on a user's machine. That shipped in 0.1.10.
        if [ ! -f "$ft_prefix/lib/libfreetype.a" ]; then
            log "error: $ft_prefix/lib/libfreetype.a is missing."
            log "       The link would take libfreetype.6.dylib instead and hard-code"
            log "       $ft_prefix into the binary, which dyld cannot find on any"
            log "       machine without this exact Homebrew keg."
            exit 1
        fi

        real_path="$ft_prefix/lib/pkgconfig:$png_prefix/lib/pkgconfig"

        # Stand-ins for the Requires.private entries that macOS has the
        # libraries for but ships no .pc files for. They deliberately carry no
        # -L: `-lz` and `-lbz2` then resolve against the SDK, i.e. /usr/lib,
        # which is on every Mac, and the pkg-config crate leaves them dynamic
        # because it finds no .a for them in any -L directory.
        #
        # Written only when nothing real is visible, so an installed keg still
        # wins. Version is required syntax but arbitrary here: freetype2.pc
        # names both without a version bound.
        shim_dir="$PREFIX/pkgconfig-shims"
        mkdir -p "$shim_dir"
        for shim in "zlib:z" "bzip2:bz2"; do
            pkg="${shim%%:*}"
            lib="${shim##*:}"
            rm -f "$shim_dir/$pkg.pc"
            if PKG_CONFIG_PATH="$real_path" "$PKG_CONFIG_BIN" --exists "$pkg" 2>/dev/null; then
                log "$pkg.pc: found, no shim needed"
                continue
            fi
            log "$pkg.pc: not on this system, writing a shim for the macOS -l$lib"
            cat > "$shim_dir/$pkg.pc" <<EOF
Name: $pkg
Description: The $pkg macOS itself provides, described for pkg-config
Version: 0
Libs: -l$lib
Cflags:
EOF
        done

        pc_path="$real_path:$shim_dir"
        verify "$pc_path"
        verify_static "$pc_path"
        echo "PKG_CONFIG_PATH=$pc_path:${PKG_CONFIG_PATH:-}"
        # Link freetype and libpng out of the Homebrew keg statically. See the
        # "Why macOS additionally has to force a static link" section above.
        # Scoped to freetype2 rather than PKG_CONFIG_ALL_STATIC so it cannot
        # change how anything else in the build resolves.
        echo "FREETYPE2_STATIC=1"
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
        verify "$PREFIX/lib/pkgconfig"
        echo "PKG_CONFIG_PATH=$PREFIX/lib/pkgconfig:${PKG_CONFIG_PATH:-}"
        ;;

    *)
        log "error: unsupported platform $(uname -s). Windows uses scripts/setup-freetype.ps1."
        exit 1
        ;;
esac
