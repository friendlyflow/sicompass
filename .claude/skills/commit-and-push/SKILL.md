---
name: commit-and-push
description: Commit all changes with a message and push to remote
disable-model-invocation: true
---

Commit and push all changes.

**IMPORTANT: The shell's working directory persists between Bash calls. Always prefix every git command in steps 3–6 with `cd PROJECT_ROOT &&` (use the actual absolute project root path) to avoid accidentally operating on a submodule.**

1. Check if `sdk/` submodule has uncommitted changes (`cd sdk && git status --porcelain`)
2. If the SDK has changes:
   - Stage and commit them inside the submodule: `cd sdk && git add -A && git commit`
   - Push the submodule: `cd sdk && git push`
   - Stage the updated submodule ref in parent: `cd PROJECT_ROOT && git add sdk`
3. `cd PROJECT_ROOT && git status -u` (never use -uall) and `git diff` to see staged and unstaged changes, and `git log --oneline -10` to see recent commit style. Cross-reference with the gitStatus snapshot from the conversation context. If live status looks clean but the snapshot shows modified files:
   - First check `git log` to see if those files were already committed earlier in this conversation
   - If not accounted for in a recent commit, explicitly verify with `git diff -- <path>` and `git status -- <path>` for each file
   - Do not trust a failed `git diff HEAD <path>` as proof the file is unchanged — use `-- <path>` syntax to avoid ambiguous argument errors
4. Stage relevant files (prefer specific files over `git add -A`)
5. Draft a concise commit message based on the changes. If `$ARGUMENTS` is provided, use it as the commit message. Remove co-authored.
6. Push to the remote with `git push`
7. Report the result
