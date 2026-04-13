---
name: commit-and-push
description: Commit all changes with a message and push to remote
disable-model-invocation: true
---

Commit and push all changes.

**IMPORTANT: The shell's working directory persists between Bash calls. Always prefix every git command with `cd PROJECT_ROOT &&` (use the actual absolute project root path).**

1. `cd PROJECT_ROOT && git status -u` (never use -uall) and `git diff` to see staged and unstaged changes, and `git log --oneline -10` to see recent commit style.
2. Stage relevant files (prefer specific files over `git add -A`)
3. Draft a concise commit message based on the changes. If `$ARGUMENTS` is provided, use it as the commit message. Remove co-authored.
4. Push to the remote with `git push`
5. Report the result
