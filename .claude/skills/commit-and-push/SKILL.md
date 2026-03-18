---
name: commit-and-push
description: Commit all changes with a message and push to remote
disable-model-invocation: true
---

Commit and push all changes.

1. Check if `sdk/` submodule has uncommitted changes (`cd sdk && git status --porcelain`)
2. If the SDK has changes:
   - Stage and commit them inside the submodule: `cd sdk && git add -A && git commit`
   - Push the submodule: `cd sdk && git push`
   - Stage the updated submodule ref in parent: `git add sdk`
3. Run `git status` to see all untracked files (never use -uall flag) and `git diff` to see staged and unstaged changes and `git log --oneline -5` to see recent commit style. Also cross-reference with the gitStatus snapshot from the conversation context — if the live status shows clean but the snapshot shows changes, re-check before concluding there's nothing to commit.
4. Stage relevant files (prefer specific files over `git add -A`)
5. Draft a concise commit message based on the changes. If `$ARGUMENTS` is provided, use it as the commit message. Remove co-authored.
6. Push to the remote with `git push`
7. Report the result
