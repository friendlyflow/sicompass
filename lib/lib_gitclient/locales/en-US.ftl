# Git-client provider strings — English (source/fallback).

gitclient-display-name = git client

# ---------------------------------------------------------------------------
# Repository root
# ---------------------------------------------------------------------------

gitclient-head-branch = { $repo } on { $branch }
gitclient-head-detached = { $repo }, detached at { $oid }
gitclient-head-unborn = { $repo }, no commits yet
gitclient-head-unreadable = { $repo }, could not be read

gitclient-ahead = { $n } ahead
gitclient-behind = { $n } behind
gitclient-in-sync = up to date
gitclient-no-upstream = no upstream branch

# Section names. Also the labels the cursor is restored by after a refresh, so
# they must not carry counts or anything else that changes.
gitclient-section-changes = changes
gitclient-section-graph = graph
gitclient-section-branches = branches
gitclient-section-stashes = stashes
gitclient-section-remotes = remotes

# ---------------------------------------------------------------------------
# Changes
# ---------------------------------------------------------------------------

# Trailing space is deliberate: the typed message follows it on the same row.
gitclient-message-label = message:{" "}

gitclient-button-commit = commit
gitclient-button-amend = commit, amending the last one
gitclient-button-push = commit and push
gitclient-button-sync = commit and sync

gitclient-clean = working tree clean

# What happened to a file. The verb leads, because that is what differs between
# two rows for the same path.
gitclient-change-modified = modified
gitclient-change-added = added
gitclient-change-deleted = deleted
gitclient-change-renamed = renamed
gitclient-change-copied = copied
gitclient-change-typechanged = type changed
gitclient-change-untracked = untracked
gitclient-change-changed = changed
gitclient-renamed-from = from { $from }

# Conflicts are named by which side did what, because that is the question that
# has to be answered to resolve one.
gitclient-conflict = conflict
gitclient-conflict-both-modified = conflict, both modified
gitclient-conflict-both-added = conflict, both added
gitclient-conflict-both-deleted = conflict, both deleted
gitclient-conflict-added-by-us = conflict, added by us
gitclient-conflict-added-by-them = conflict, added by them
gitclient-conflict-deleted-by-us = conflict, deleted by us
gitclient-conflict-deleted-by-them = conflict, deleted by them

# ---------------------------------------------------------------------------
# Diffs and empty levels
# ---------------------------------------------------------------------------

gitclient-binary = binary file, no text to show
gitclient-unreadable = file could not be read
gitclient-diff-truncated = { $n } more lines, not shown
gitclient-empty = nothing here
gitclient-gone = this is no longer here, press F5

# ---------------------------------------------------------------------------
# Errors
# ---------------------------------------------------------------------------

gitclient-error-not-a-repository = not a git repository

# ---------------------------------------------------------------------------
# Graph
# ---------------------------------------------------------------------------

gitclient-no-commits = no commits yet
gitclient-load-more = load { $n } more commits
gitclient-stats = { $files } files changed, { $ins } insertions, { $del } deletions
gitclient-commit-author = author: { $author }
gitclient-commit-date = date: { $date }
gitclient-commit-refs = refs: { $refs }

# ---------------------------------------------------------------------------
# Branches, stashes, remotes
# ---------------------------------------------------------------------------

gitclient-scope-local = local
gitclient-scope-remote = remote
gitclient-no-branches = no branches
gitclient-branch-current = the branch you are on
gitclient-branch-tracking = tracking { $upstream }
gitclient-branch-gone = its upstream branch is gone
gitclient-branch-elsewhere = checked out in { $path }

gitclient-no-stashes = no stashes
gitclient-no-remotes = no remotes
gitclient-remote-fetch = fetch: { $url }
gitclient-remote-push = push: { $url }

# ---------------------------------------------------------------------------
# Notices and confirmations
# ---------------------------------------------------------------------------

gitclient-working = running { $what }
gitclient-done = { $what } finished
gitclient-stale = the repository changed on disk, press F5 to reload

gitclient-confirm-cancel = cancel, change nothing
gitclient-confirm-yes = yes, and lose the changes to { $what }

# ---------------------------------------------------------------------------
# Errors
# ---------------------------------------------------------------------------

gitclient-error-locked = another git process is running in this repository, try again when it is done
gitclient-error-select-a-file = put the cursor on a file first
gitclient-error-select-a-commit = put the cursor on a commit first
gitclient-error-select-a-branch = put the cursor on a branch first
gitclient-error-select-a-stash = put the cursor on a stash first
gitclient-error-select-a-remote = put the cursor on a remote first
gitclient-error-not-staged = that file is not staged
gitclient-error-remote-syntax = type a name and a url, separated by a space
gitclient-error-no-message = type a commit message first, press i on the message row
gitclient-error-conflicts = resolve the conflicts first, stage each file once it is fixed
gitclient-error-nothing-to-amend = there is no commit to amend yet
gitclient-error-busy = another remote operation is still running
gitclient-error-undo = that could not be undone
