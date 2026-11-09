# Tutorial (`sicompass-tutorial`)

## Prose

In the tutorial content (`src/lib.rs`), do not use em dashes or semicolons.
Use commas instead, or split into separate sentences (parentheses are fine for
true parentheticals).

## Authoring

When writing or restructuring the in-app tutorial, follow the rules in
[docs/tutorial-guidelines.md](../../docs/tutorial-guidelines.md): teach by
doing, one idea per step, confirm via the screen-reader announcement, keep a short
guided path separate from the reference manual, make keyboard shortcuts lead each
line, and add every new string to all four locale bundles.

## When to update

In the same change as anything it describes: a key, a mode, an element type or its
mark in the list prefix, or what is said after an action, wherever that change is
made (usually `../sicompass-ui`). Check the beginner's manual in the same change, as
its rules in `../sicompass-ui/docs/manual-guidelines.md` say. The tests in
`src/lib.rs` hold the prose rule and the four languages. The root `CLAUDE.md`
("Help texts") has the hook that reminds you.
