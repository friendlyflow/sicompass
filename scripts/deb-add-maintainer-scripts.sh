#!/usr/bin/env bash
#
# Inject src/sicompass/deb/{postinst,postrm} into a .deb built by
# cargo-packager, in place.
#
# Why this exists: a package that ships icons and a .desktop entry has to
# refresh the icon cache and the application database on install, or the menu
# entry shows up with no icon (see the header of src/sicompass/deb/postinst).
# cargo-packager has no way to emit maintainer scripts — `DebianConfig` has no
# field for them, and its control-file writer produces only `control` and
# `md5sums` — so the .deb is unpacked and rebuilt here instead.
#
# Notes on the round trip:
#   - `--root-owner-group` is required. `dpkg-deb -R` run as a normal user
#     extracts the tree owned by that user, and without the flag `dpkg-deb -b`
#     would bake the runner's uid into the package.
#   - `-Zgzip` matches the compression cargo-packager used, so the only
#     difference between the input and the output is the two new files.
#   - `md5sums` is left alone on purpose: nothing under the data tree changes.
#
# Usage: scripts/deb-add-maintainer-scripts.sh <path/to/package.deb>

set -euo pipefail

if [ $# -ne 1 ]; then
    echo "usage: $0 <path/to/package.deb>" >&2
    exit 2
fi

deb=$1
root=$(cd "$(dirname "$0")/.." && pwd)
src=$root/src/sicompass/deb

if [ ! -f "$deb" ]; then
    echo "error: no such file: $deb" >&2
    exit 1
fi

for script in postinst postrm; do
    if [ ! -f "$src/$script" ]; then
        echo "error: $src/$script is missing" >&2
        exit 1
    fi
done

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

dpkg-deb -R "$deb" "$work/pkg"

for script in postinst postrm; do
    install -m 0755 "$src/$script" "$work/pkg/DEBIAN/$script"
done

dpkg-deb --root-owner-group -Zgzip -b "$work/pkg" "$work/out.deb"
mv "$work/out.deb" "$deb"

# Fail loudly rather than shipping a .deb that silently lost the scripts.
listing=$(dpkg-deb --ctrl-tarfile "$deb" | tar t)
for script in postinst postrm; do
    case "$listing" in
        *"./$script"*) ;;
        *)
            echo "error: $script is missing from the rebuilt $deb" >&2
            exit 1
            ;;
    esac
done

echo "added postinst and postrm to $deb"
