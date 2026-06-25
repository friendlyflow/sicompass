# Tutorial Guidelines

How to write and extend the Sicompass tutorial (`lib/lib_tutorial/`).

These are the rules we hold the tutorial to. They are general principles for any
good tutorial, adapted to Sicompass, which is a keyboard-driven, screen-reader-first
interface. Read this before adding or restructuring tutorial content, and audit
new sections against it.

## Prose style

The tutorial content lives in the `.ftl` locale files and is referenced from the
`SECTIONS` tree in `src/lib.rs`. Both fall under the project prose rule: do not use
em dashes or semicolons. Use commas, or split into separate sentences. Parentheses
are fine for true parentheticals. This rule exists partly for screen readers, where
punctuation maps to pauses and stray dashes read oddly.

## The rules

### 1. Teach by doing, not by reading

The learner should perform a real action almost immediately. Introduce a concept
only because the next action needs it, never "just so you know". If a paragraph
does not lead to a keystroke or a decision, cut it or move it to reference material.

Sicompass has a strong advantage here: the tutorial is itself a provider, so
navigating it is already practicing navigation. Lean into that. The long
`lorem_ipsum` block is not filler, it is real practice material for PgUp, PgDown,
Home, End, and scroll mode. Practice surfaces like this are good and should be kept.

### 2. One new idea per step

Each step introduces one concept and exercises it. If a step teaches two things,
split it. This is what makes a tutorial feel easy. It is not about dumbing down, it
is about never stacking unknowns.

### 3. Always-valid state

The learner can never get stuck or break things. The tutorial provider is read-only,
which gives us this for free. Preserve that property. Any new interactive step must
be safe to repeat and impossible to dead-end.

### 4. Show, then let them do, then confirm

The loop is demonstrate, prompt the action, confirm success. The confirmation
matters most. In Sicompass the confirmation is what the screen reader announces, so
design each guided step so that doing the action produces a clear, distinct spoken
result the learner can recognize as success.

### 5. Real context, not a toy sandbox

Teach inside the actual interface with realistic data, not a separate mode that
behaves differently. Skills learned in a fake environment do not transfer.

### 6. Make progress and exit visible

The learner should always know where they are, how much is left, and how to leave or
resume. The "w" whereami key and the breadcrumb path support this. A guided sequence
should let the learner skip ahead and leave at any point without losing their place.

### 7. Front-load the payoff

Order steps so an early one produces something the learner actually wanted. A short
guided "Getting Started" path belongs at the very top of the tree, before the
exhaustive reference sections.

### 8. Fail gracefully and instructively

When the learner does the wrong thing, treat it as a teaching moment, not an error.
The wrong action tells you what they expected. Use it to correct the mental model.

## Sicompass-specific conventions

### Keep the manual and the onboarding separate

A guided onboarding and a reference manual want opposite structures (progressive and
completable versus exhaustive and browsable). Do not let one `SECTIONS` tree try to
be both. The current tree is organized as: a guided **Getting Started** path first,
then a single **Shortcuts at a glance** key reference, then lean concept and program
sections, and finally a pointer to the repo/SDK docs for plugin development. Keep it
that way. When you add a topic, give it one short leaf in the relevant section, do not
inline a manual, and do not re-list keys that already live in Shortcuts at a glance.

### Make keyboard shortcuts stand out

Shortcuts are the core vocabulary of the app, and screen-reader users navigate by the
first words announced. Lead each shortcut leaf with the key itself (for example
"S: switch to scroll mode ..."), so the key is the first thing spoken, not buried mid
sentence. Mode-switching keys especially should be easy to find and skim.

### The confirmation is the accessibility surface

Because the app speaks through a small AccessKit node set rather than a visual widget
tree, the spoken announcement after an action is the only feedback a screen-reader
user gets. Treat that announcement as the success signal when designing a step.

### Every string is localized

Tutorial content is keyed through Fluent and must exist in all four locale bundles
(`en-US`, `nl-BE`, `fr-BE`, `de-BE`). A key missing from the active locale renders as
the raw key id to the user. When you add a leaf or branch, add it to every locale
file.

## When you add or change a section

1. Add the branch or leaf to `SECTIONS` in `src/lib.rs`.
2. Add the matching keys to all four `.ftl` locale files.
3. Follow the prose style (no em dashes, no semicolons).
4. Add or update the structural tests in `src/lib.rs` (the `tests` module asserts
   section placement and content).
5. Run `cargo test -p lib_tutorial`.
