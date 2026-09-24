---
name: release-sdk
description: Publish sicompass-sdk / sicompass-pdk to crates.io (moved to /release sicompass-plugin-sdk)
disable-model-invocation: true
model: sonnet
---

This procedure moved into the SDK repo itself, where it sits next to the code it
releases: `../sicompass-plugin-sdk/.claude/skills/release/SKILL.md`.

Run `/release sicompass-plugin-sdk`, which follows that file (see
[.claude/repo-selection.md](../../repo-selection.md)). Read it and follow it now
if this skill was invoked, with `PROJECT_ROOT` = the absolute path of
`../sicompass-plugin-sdk`.
