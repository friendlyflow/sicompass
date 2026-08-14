#!/usr/bin/env bash
# Snapshots the live sicompass config before anything that can overwrite it.
#
# The same machine runs sicompass in production and in development, and both
# read/write ~/.config/sicompass/settings.json. A test run (or a dev run) can
# rewrite the production file, so the real settings are kept as an ever-growing
# chain of numbered copies:
#
#   settings.json -> settings_copy0.json -> settings_copy1.json -> ...
#
# On every call: compare settings.json with the highest-numbered copy. Identical
# means nothing new to keep, so do nothing. Different means the live file holds
# state no copy has, so it becomes the next copy in the chain. Nothing is ever
# overwritten and nothing is ever restored automatically, so a bad test run can
# only ever add a copy, never destroy one.
#
# Numbering starts at 0 when no numbered copy exists yet.
set -u

DIR="${XDG_CONFIG_HOME:-$HOME/.config}/sicompass"
LIVE="$DIR/settings.json"

[ -f "$LIVE" ] || exit 0

last=""
lastn=-1
for f in "$DIR"/settings_copy[0-9]*.json; do
  [ -e "$f" ] || continue
  n=${f##*/settings_copy}
  n=${n%.json}
  case "$n" in '' | *[!0-9]*) continue ;; esac
  n=$((10#$n))
  if [ "$n" -gt "$lastn" ]; then
    lastn=$n
    last=$f
  fi
done

# Already captured: the live file matches the newest copy.
if [ -n "$last" ] && cmp -s "$LIVE" "$last"; then
  exit 0
fi

if [ "$lastn" -lt 0 ]; then
  next=0
else
  next=$((lastn + 1))
fi

cp -p "$LIVE" "$DIR/settings_copy$next.json" || exit 0
printf '{"systemMessage":"sicompass settings.json changed since %s - saved a copy as settings_copy%s.json before running","suppressOutput":true}\n' \
  "$(basename "${last:-<none>}")" "$next"
