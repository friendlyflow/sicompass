# Project-management provider strings — English (source/fallback).
#
# Command ids are language-neutral and never come from here: `handle_command`
# matches the raw strings in `CMD_*` by equality. Only their labels translate.

projectmanagement-display-name = project management

# Shown in a level that holds nothing yet. A level is never returned empty: the
# app seeds its own insert placeholder into an empty one, which would then look
# like a row this provider had rendered.
pm-empty-columns = no columns yet, press ctrl+a to add one
pm-empty-cards = no cards yet, press ctrl+a to add one

# The name a column or card gets when it is created but never typed into.

pm-cmd-move-up = move up
pm-cmd-move-down = move down
pm-cmd-move-left = move to previous column
pm-cmd-move-right = move to next column

pm-error-unreadable = your board could not be read, so nothing has been saved, the files on disk are untouched
pm-error-save = your board could not be saved
pm-error-no-column = add a column first, a card has to live in one
pm-error-nothing-to-paste = nothing has been copied yet

pm-board-empty-slot = press ctrl+a for a first card
pm-board-no-columns = no columns yet, add one in the list with ctrl+a

# Screen-reader lines. The dashboard forwards every key to this provider, so
# nothing else in the app is in a position to say where the cursor now is.
pm-say-card = { $column }, card { $index } of { $total }, { $text }
pm-say-column-empty = column { $index } of { $total }, { $title }, empty
pm-say-insert = insert mode, { $text }
pm-say-insert-empty = insert mode, empty
pm-say-board = board mode
pm-say-deleted = deleted, { $text }
pm-say-copied = copied, { $text }
pm-say-cut = cut, { $text }
pm-say-pasted = pasted, { $text }
pm-say-undone = undone, { $what }
pm-say-redone = redone, { $what }
pm-say-edge = no further

# Timeline labels, shown in the undo history screen and spoken back on undo.
pm-op-add-card = add card
pm-op-delete-card = delete card
pm-op-rename-card = rename card
pm-op-move-card = move card
