#!/usr/bin/env bash
#
# Soft reminder to keep the help texts in step with what they describe: the
# tutorial (lib/lib_tutorial, docs/tutorial-guidelines.md), the app's beginner's
# manual (manual-NN in ../sicompass-ui/locales, its docs/manual-guidelines.md) and
# the login screen's manual (login-manual-NN in ../loginsicompass/locales).
#
# PostToolUse on Edit|Write. When the edited file is one that changes keys, modes
# or what is said (the triggers below), and a help text that describes it has no
# change yet, the reminder goes to Claude as additionalContext. It never blocks
# (always exit 0), and it reminds once per file per session.
set -uo pipefail

INPUT=$(cat)
FILE_PATH=$(echo "$INPUT" | jq -r '.tool_input.file_path // empty')
SESSION=$(echo "$INPUT" | jq -r '.session_id // "none"')
[ -n "$FILE_PATH" ] || exit 0

ROOT="${CLAUDE_PROJECT_DIR:-.}"
UI="$ROOT/../sicompass-ui"
LOGIN="$ROOT/../loginsicompass"

# ---- Triggers: the files that change keys, modes, element marks or speech ----
# Edit these lists when the code moves.
UI_TRIGGERS='/sicompass-ui/src/(shortcuts|handlers|in_element|events)\.rs$|/sicompass-ui/locales/[^/]+\.ftl$'
LOGIN_TRIGGERS='/loginsicompass/src/(provider|gui)\.rs$|/loginsicompass/locales/[^/]+\.ftl$'

kind=""
if [[ "$FILE_PATH" =~ $UI_TRIGGERS ]]; then
  kind=ui
elif [[ "$FILE_PATH" =~ $LOGIN_TRIGGERS ]]; then
  kind=login
else
  exit 0
fi

# Changed message lines in a diff, keys only, without comments.
changed_keys() { # <repo> <pathspec...>
  local repo=$1
  shift
  git -C "$repo" diff HEAD -U0 -- "$@" 2>/dev/null |
    grep -E '^[+-][a-zA-Z][a-zA-Z0-9_-]* *=' | grep -vE '^(\+\+\+|---)' |
    sed -E 's/^[+-]([a-zA-Z0-9_-]+).*/\1/'
}

# A locale file is a trigger only for lines that are not the manual's own.
if [[ "$FILE_PATH" == *.ftl ]]; then
  manual_prefix=manual-
  [ "$kind" = login ] && manual_prefix=login-manual-
  if ! changed_keys "$(dirname "$FILE_PATH")" "$FILE_PATH" | grep -qv "^$manual_prefix"; then
    exit 0
  fi
fi

# ---- Which help texts already changed ----------------------------------------
tutorial_changed=no
[ -n "$(git -C "$ROOT" status --porcelain -- lib/lib_tutorial 2>/dev/null)" ] && tutorial_changed=yes
app_manual_changed=no
changed_keys "$UI" locales | grep -q '^manual-' && app_manual_changed=yes
login_manual_changed=no
changed_keys "$LOGIN" locales | grep -q '^login-manual-' && login_manual_changed=yes

owed=()
if [ "$kind" = ui ]; then
  [ "$tutorial_changed" = no ] &&
    owed+=("the tutorial (sicompass lib/lib_tutorial, rules in docs/tutorial-guidelines.md)")
  [ "$app_manual_changed" = no ] &&
    owed+=("the app's beginner's manual (manual-NN in ../sicompass-ui/locales, rules in ../sicompass-ui/docs/manual-guidelines.md)")
fi
[ "$login_manual_changed" = no ] &&
  owed+=("the login screen's manual (login-manual-NN in ../loginsicompass/locales, same rules)")
[ ${#owed[@]} -gt 0 ] || exit 0

# ---- Once per file per session ------------------------------------------------
STATE="${TMPDIR:-/tmp}/sicompass-help-texts-$SESSION"
if [ -f "$STATE" ] && grep -qxF "$FILE_PATH" "$STATE"; then
  exit 0
fi
echo "$FILE_PATH" >>"$STATE"

list=$(printf -- '- %s\n' "${owed[@]}")
msg="Help texts: you changed $FILE_PATH, which can change keys, modes, element marks or what is said. These texts describe that and have no change yet:
$list
If what they say is now wrong or missing, update them in all four languages. If they need nothing, say so in your reply. Check README.md too if a feature it lists changed. (Reminder only, once per file.)"

jq -n --arg c "$msg" '{hookSpecificOutput: {hookEventName: "PostToolUse", additionalContext: $c}}'
exit 0
