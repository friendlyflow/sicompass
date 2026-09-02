# Notes-provider strings — English (source/fallback).
#
# Storage ids are language-neutral and never come from here: `private` and
# `public` are written to disk and read by a server, so only their *labels* are
# translated. See `tree::Visibility::as_str`.

notes-display-name = notes

# The header row at the top of every list below the top level. Localized on
# purpose, and for one hard reason it must never be the bare word "meta": the
# app skips `pop_path` when leaving an Obj keyed exactly "meta", which would
# leave the provider's path one level deeper than the cursor.
notes-list-meta = list meta:

notes-visibility = visibility
notes-visibility-private = private
notes-visibility-public = public

notes-sha256 = sha256: { $hash }

# Shown in a level that holds nothing yet. A level is never returned empty: the
# app seeds its own insert placeholder into an empty one, which would then look
# like a row this provider had rendered.
notes-empty = no notes yet, press ctrl+a to write one

# The note that used to be here is gone, most likely deleted in another tab.
notes-gone = this note is no longer here

notes-cmd-move-up = move up
notes-cmd-move-down = move down
notes-cmd-duplicate = duplicate

notes-error-meta-undeletable = the list meta row belongs to the list and cannot be deleted
notes-error-meta-readonly = the list meta row cannot be edited, change visibility from inside it
notes-error-unreadable = your notes could not be read, so nothing has been saved, the files on disk are untouched
notes-error-save = your notes could not be saved
