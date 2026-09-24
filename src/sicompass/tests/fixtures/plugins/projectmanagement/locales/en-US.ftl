# Project-management provider strings — English (source/fallback).
#
# Command ids are language-neutral and never come from here: `handle_command`
# matches the raw strings in `CMD_*` by equality. Only their labels translate.

projectmanagement-display-name = project management

# Shown in a level that holds nothing yet. A level is never returned empty: the
# app seeds its own insert placeholder into an empty one, which would then look
# like a row this provider had rendered.
projectmanagement-empty-columns = no columns yet, press ctrl+a to add one
projectmanagement-empty-cards = no cards yet, press ctrl+a to add one

# The name a column or card gets when it is created but never typed into.

projectmanagement-cmd-move-up = move up
projectmanagement-cmd-move-down = move down
projectmanagement-cmd-move-left = move to previous column
projectmanagement-cmd-move-right = move to next column
projectmanagement-cmd-archive-card = archive card

# The settings checkbox that turns on mirroring the board to the server.
projectmanagement-checkbox-cloud-backup = enable cloud backup

projectmanagement-error-unreadable = your board could not be read, so nothing has been saved, the files on disk are untouched
projectmanagement-error-save = your board could not be saved
projectmanagement-error-no-column = add a column first, a card has to live in one
projectmanagement-error-nothing-to-paste = nothing has been copied yet

projectmanagement-board-empty-slot = press ctrl+a for a first card
projectmanagement-board-no-columns = no columns yet, add one in the list with ctrl+a

# The archive column's title when it is first created. Only a name: the
# column is identified by its id, so the user can rename it afterwards and it
# is still the archive. No trailing colon, `column_title` strips one.
projectmanagement-archive-title = Archive

# Screen-reader lines. The dashboard forwards every key to this provider, so
# nothing else in the app is in a position to say where the cursor now is.
projectmanagement-say-card = { $column }, card { $index } of { $total }, { $text }
projectmanagement-say-column-empty = column { $index } of { $total }, { $title }, empty
projectmanagement-say-insert = insert mode, { $text }
projectmanagement-say-insert-empty = insert mode, empty
projectmanagement-say-board = board mode
projectmanagement-say-deleted = deleted, { $text }
projectmanagement-say-copied = copied, { $text }
projectmanagement-say-cut = cut, { $text }
projectmanagement-say-pasted = pasted, { $text }
projectmanagement-say-archived = archived, { $text }
projectmanagement-say-nothing-to-archive = nothing to archive, put the cursor on a card first
projectmanagement-say-already-archived = already archived
projectmanagement-say-undone = undone, { $what }
projectmanagement-say-redone = redone, { $what }
projectmanagement-say-edge = no further

# Timeline labels, shown in the undo history screen and spoken back on undo.
projectmanagement-op-add-card = add card
projectmanagement-op-delete-card = delete card
projectmanagement-op-rename-card = rename card
projectmanagement-op-move-card = move card
projectmanagement-op-archive-card = archive card

# The Store's description of this plugin, and of the paid service it offers.
projectmanagement-description = A kanban board you arrange in a list or on a grid, with an optional cloud backup.
projectmanagement-service = keeps a copy of your board on the Sicompass Cloud server

# The row above the columns while cloud backup is on. It never links
# anywhere: buying and redeeming are in store, tiers.
projectmanagement-cloud-needs-payment = cloud backup: needs Sicompass Cloud, see store, tiers
projectmanagement-cloud-active = cloud backup: on, renews in { $days } days
projectmanagement-cloud-grace = cloud backup: subscription expired, still on for { $days } days, renew in store, tiers
projectmanagement-cloud-expired = cloud backup: off, the subscription expired { $days } days ago, see store, tiers
projectmanagement-cloud-needs-subscription = cloud backup needs Sicompass Cloud, see store, tiers
projectmanagement-cloud-failed = cloud backup failed: { $reason }
projectmanagement-error-cloud-row-undeletable = the cloud backup row is not a column, turn cloud backup off in settings to remove it

projectmanagement-cmd-restore-backup = restore cloud backup
projectmanagement-restore-done = cloud backup restored
projectmanagement-restore-empty = there is no cloud backup to restore
projectmanagement-restore-refused = your board is not empty, so nothing was restored
