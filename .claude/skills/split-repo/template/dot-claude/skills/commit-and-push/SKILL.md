---
name: commit-and-push
description: Commit all changes in this repo with a message and push to origin main
argument-hint: "[message]"
model: sonnet
---

Commit and push all changes in this repo.

This file is also what `/commit-and-push <this repo>` follows when it is run
from a sicompass checkout, so `PROJECT_ROOT` is this repo's root, which is not
necessarily the session's working directory.

**IMPORTANT: The shell's working directory persists between Bash calls. Prefix
every command with `cd PROJECT_ROOT &&` (the absolute path of this repo).**

**Always work directly on `main`.** Never create, switch to or push a branch.

1. `git status -u` (never `-uall`), `git diff`, and `git log --oneline -10` for
   the message style.
2. Stage the relevant files, specific paths rather than `git add -A`. If
   `CLAUDE.md` has a "Generated files that are committed" section, every file it
   lists must be committed together with the source it was generated from.
   Check that section before staging.
3. Make sure no `[patch]` section in `Cargo.toml` is uncommented. Those point at
   sibling checkouts (`../sicompass-ui`, `../sicompass-plugin-sdk`, ...) and must
   never reach `main`, because CI and `nix build` only check out this repo.
4. Draft a concise message from the changes. If `$ARGUMENTS` is given, use it as
   the message. No co-author trailer.
5. Commit on `main`, then `git push origin HEAD:main`. If the push reports that
   `main` diverged, `git fetch origin` and reconcile. Never force-push.
6. Report the commit hash and what was pushed.
