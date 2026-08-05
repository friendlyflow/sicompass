#!/usr/bin/env bash
#
# Fail if a macOS build links anything that will not exist on a user's machine.
#
# ## Why this exists
#
# 0.1.10 shipped a .dmg that could not launch at all. The binary carried
#
#     /opt/homebrew/opt/freetype/lib/libfreetype.6.dylib
#
# as an absolute load path, because the release runner had that keg installed
# and the plain pkg-config probe in scripts/setup-freetype.sh picked the dylib
# over the .a sitting next to it. Every Mac without `brew install freetype`
# got a dyld "Library not loaded" abort before `main` ran.
#
# Nothing in the pipeline noticed. The build succeeded, the .app was signed,
# the .dmg mounted, and the failure only appeared on a machine that was not
# the build machine. This check is the missing step: it asks the one question
# that distinguishes "it links" from "it runs anywhere", and it costs a
# fraction of a second.
#
# ## What counts as acceptable
#
# Only /usr/lib and /System/Library, which are part of macOS itself, and the
# @rpath / @executable_path / @loader_path forms, which resolve inside the
# bundle. Anything else, and in particular anything under a Homebrew prefix
# (/opt/homebrew on Apple Silicon, /usr/local on Intel), is a machine-specific
# path that has no business in a released binary.
#
# A dylib's own install id is reported but not counted. `otool -L` lists it
# first, alongside the real dependencies, and cargo-packager leaves the
# Homebrew id on the MoltenVK it bundles. That is harmless here because
# `render::load_vulkan_entry` dlopens the bundled copy by absolute path, so
# nothing ever resolves MoltenVK *by* its id. Counting it would mean failing
# every release over a string dyld does not read.
#
# ## Usage
#
#     scripts/check-macos-standalone.sh target/release/sicompass
#     scripts/check-macos-standalone.sh target/packages/sicompass.app
#
# A .app expands to its executables plus every dylib bundled beside them, so a
# bad path inside MoltenVK is caught too, not just one in our own binary.
#
# ## Where this runs
#
# ci.yml on both macOS legs, and native-packages.yml both before packaging and
# on the assembled .app. It is deliberately *not* wired into cargo-dist's
# archive build: `github-build-setup` injects steps before the build only, and
# there is no post-build hook. That is acceptable because the archive binary
# comes off the same runner, from the same sources, with the same environment
# from scripts/setup-freetype.sh, so anything wrong with it is wrong with the
# binary this does check.

set -euo pipefail

if [ "$(uname -s)" != "Darwin" ]; then
    echo "error: this check only means anything on macOS" >&2
    exit 1
fi

[ "$#" -gt 0 ] || { echo "usage: $0 <binary-or-.app> [...]" >&2; exit 1; }

# Expand each argument to the Mach-O files it stands for.
targets=()
for arg in "$@"; do
    if [ -d "$arg" ] && [ "${arg##*.}" = "app" ]; then
        while IFS= read -r f; do
            targets+=("$f")
        done < <(find "$arg/Contents/MacOS" -type f 2>/dev/null; \
                 find "$arg/Contents/Frameworks" -name '*.dylib' 2>/dev/null)
    elif [ -f "$arg" ]; then
        targets+=("$arg")
    else
        echo "error: $arg is neither a file nor a .app bundle" >&2
        exit 1
    fi
done

[ "${#targets[@]}" -gt 0 ] || { echo "error: nothing to check" >&2; exit 1; }

problems=0
for bin in "${targets[@]}"; do
    # Not a Mach-O (a script, a resource that landed in MacOS/) -> nothing to say.
    otool -L "$bin" >/dev/null 2>&1 || continue

    echo "$bin"
    # Empty for an executable; the LC_ID_DYLIB string for a dylib.
    install_id="$(otool -D "$bin" 2>/dev/null | tail -n +2 | head -1)"
    while IFS= read -r line; do
        # `otool -L` prints the file itself first, then one indented
        # "<path> (compatibility version ...)" per load command.
        case "$line" in
            $'\t'*) ;;
            *) continue ;;
        esac
        path="${line#$'\t'}"
        path="${path%% (compatibility*}"

        if [ -n "$install_id" ] && [ "$path" = "$install_id" ]; then
            echo "  id   $path"
            continue
        fi

        case "$path" in
            /usr/lib/*|/System/Library/*|@rpath/*|@executable_path/*|@loader_path/*)
                echo "  ok   $path"
                ;;
            *)
                echo "  BAD  $path"
                problems=$((problems + 1))
                ;;
        esac
    done < <(otool -L "$bin")
done

if [ "$problems" -gt 0 ]; then
    echo
    echo "::error::$problems load path(s) point outside macOS and outside the bundle."
    echo "Those directories exist on this build machine and not on a user's, so the"
    echo "binary will abort at launch with a dyld 'Library not loaded' error. Link"
    echo "the library statically or bundle it and rewrite its install name."
    exit 1
fi

echo
echo "All load paths are either macOS itself or inside the bundle."
