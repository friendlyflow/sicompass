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

# The header row at the top of every list. Localized on purpose, and for one
# hard reason it must never be the bare word "meta": the app skips `pop_path`
# when leaving an Obj keyed exactly "meta", which would leave the provider's
# path one level deeper than the cursor.
projectmanagement-header = header
projectmanagement-sha256 = sha256: { $hash }
projectmanagement-error-header-undeletable = the header row belongs to the list and cannot be deleted
projectmanagement-error-header-readonly = the header row cannot be edited

# The name a column or card gets when it is created but never typed into.

projectmanagement-cmd-move-up = move up
projectmanagement-cmd-move-down = move down
projectmanagement-cmd-move-left = move to previous column
projectmanagement-cmd-move-right = move to next column
projectmanagement-cmd-archive-card = archive card

# The settings checkbox that turns on syncing the board through the server.
projectmanagement-checkbox-cloud-backup = enable cloud sync

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
projectmanagement-description = A kanban board you arrange in a list or on a grid, with an optional cloud sync.
projectmanagement-service = keeps your board in sync between your computers through the Sicompass Cloud server

# The row above the columns while cloud sync is on. It never links
# anywhere: buying and redeeming are in store, tiers.
projectmanagement-cloud-needs-payment = cloud sync: needs Sicompass Cloud, see store, tiers
projectmanagement-cloud-active = cloud sync: on, renews in { $days } days
projectmanagement-cloud-grace = cloud sync: subscription expired, still on for { $days } days, renew in store, tiers
projectmanagement-cloud-expired = cloud sync: off, the subscription expired { $days } days ago, see store, tiers
projectmanagement-cloud-needs-subscription = cloud sync needs Sicompass Cloud, see store, tiers
projectmanagement-cloud-failed = cloud sync failed: { $reason }
projectmanagement-error-cloud-row-undeletable = the cloud sync row is not a column, turn cloud sync off in settings to remove it

projectmanagement-cmd-sync-now = sync with the cloud now
projectmanagement-sync-pulled = changes from your other computers were added
projectmanagement-sync-conflicts = changes from your other computers were added, { $count } edited on both, the latest edit was kept
# The header's line about the cloud: whether this list is as it was at
# the last sync.
projectmanagement-sync-status-synced = cloud: in sync
projectmanagement-sync-status-changed = cloud: changed since the last sync
projectmanagement-sync-status-new = cloud: not synced yet

# The tutorial's paragraphs about this program, under its programs section:
# <name>-tutorial, then <name>-tutorial-2 and so on, read until one is missing.
projectmanagement-tutorial = Project management, from the Store: a kanban board. Press ctrl+a to add a column, Right to open it, and ctrl+a inside to add a card. A column holds cards and nothing deeper, so a card is the last level. The first row of each list is its header, with the list's hash and, with cloud sync on, whether it changed since the last sync.
projectmanagement-tutorial-2 = d: open the board itself, with the columns side by side. The cursor sits on a card, and the one card it is on is the only thing highlighted. Left and Right move between columns, Up and Down through the cards in one. i and a edit the card, o and shift+o open a new one below or above, ctrl+d deletes, and ctrl+x, ctrl+c and ctrl+v cut, copy and paste. An empty column shows one slot you can stand on to add its first card. Columns themselves are managed in the list. Escape goes back.
