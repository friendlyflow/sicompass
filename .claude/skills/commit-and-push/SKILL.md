---
name: commit-and-push
description: Commit all changes with a message and push to remote main via gh
disable-model-invocation: true
---

Commit and push all changes.

**IMPORTANT: The shell's working directory persists between Bash calls. Always prefix every git command with `cd PROJECT_ROOT &&` (use the actual absolute project root path).**

**Always work directly on `main`. Never create, switch to, or push a branch — commit on the current `main` checkout and push straight to `origin/main`.**

1. `cd PROJECT_ROOT && git status -u` (never use -uall) and `git diff` to see staged and unstaged changes, and `git log --oneline -10` to see recent commit style.
2. Stage relevant files (prefer specific files over `git add -A`).
3. Draft a concise commit message based on the changes. If `$ARGUMENTS` is provided, use it as the commit message. Do not add a co-authored trailer.
4. Commit on `main`.
5. Push to the `origin` remote. `origin` is the HTTPS URL and git is already configured (globally, via home.nix) to authenticate github.com through `gh`, so a plain push works and — because it targets the named remote — advances the `origin/main` tracking ref so VS Code follows the push:

   ```
   git push origin HEAD:main
   ```

   If the push reports `main` diverged, `git fetch origin` first, then reconcile — never force-push and never move work onto a branch.
6. Report the result.
