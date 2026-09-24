# Choosing the repo a skill runs in

sicompass is one of several sibling repos under the same parent directory
(`../desicompass`, `../sicompass-ui`, `../<name>_plugin_sicompass`, ...). Work
is driven from this checkout, so `/commit-and-push`, `/release`, `/sync` and
`/update-cargo` take the target repo as their **first argument**.
`.claude/repos.json` is the list of names they accept.

Resolve it before doing anything else:

1. Read `.claude/repos.json` from this repo's root.
2. Take the first word of `$ARGUMENTS`.
   - **It matches a `name`.** That repo is the target. Remove the word from
     `$ARGUMENTS`, and treat what is left as the arguments for the rest of the
     skill.
   - **It is `all`** (only `/sync` and `/update-cargo` accept it). Run the skill
     once per repo, in the order listed, and end with one table covering all of
     them: repo, what moved, build, tests. A failure in one repo is reported
     and does not stop the others. For `/update-cargo all`, each repo still gets
     its own commit.
   - **Anything else, or nothing.** The target is `sicompass`, and `$ARGUMENTS`
     is left as it is. A commit message that happens to start with a repo name
     is the one ambiguous case: if a message was clearly meant, ask.
3. `PROJECT_ROOT` is the target's `path`, resolved against this repo's root to
   an absolute path. Check that it exists and is a git repository
   (`git -C <path> rev-parse --show-toplevel`). If it is not, stop and say so.
   Never clone or create it here, since `/split-repo` does that.

## Then which steps to follow

- **Target is `sicompass`.** Follow the steps in the skill you are running.
- **Target is another repo that has its own
  `<path>/.claude/skills/<skill>/SKILL.md`.** Read that file and follow it
  **instead** of the steps in this one. The sibling's skill knows that repo's
  generated files, its release mechanism and its test command, and this one does
  not. Do not mix steps from the two. Every command still gets
  `cd <absolute PROJECT_ROOT> &&`, because the shell's working directory
  persists between calls and a stray command lands in the wrong repo.
- **Target has no skill of its own.** At the moment this only applies to
  `sicompass-plugin-sdk`, which has no `.claude/skills/`.
  - `/commit-and-push` and `/sync`: follow the steps in this skill in that repo,
    but skip everything that names sicompass files (shaders, icons,
    `THIRD-PARTY-LICENSES.html`, `release.yml`, `-p sicompass`, `--workspace`
    Pulley runs).
  - `/release`: for the SDK, use `/release-sdk`. Otherwise stop and say the repo
    has no release procedure yet.
  - `/update-cargo`: stop. The SDK is covered by the canary in sicompass's own
    `/update-cargo`.

Report which repo you acted on in the first line of the result.
