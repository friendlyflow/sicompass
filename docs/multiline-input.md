# Multiline text fields

How to give a provider or plugin a text field that edits well: long text that
wraps, `Ctrl+Enter` for a new line, Up/Down that follow the lines on screen, a
selection that follows them too, and one background block behind the whole
field.

There is one implementation of all of that, and this page is about using it
rather than writing another one. It comes in two forms:

- **`<input>`**, which the app edits for you. Almost every provider uses this.
- **`sicompass_sdk::input`**, the same model as plain data, for a surface the app
  does not draw (a dashboard, native or WASM).

## 1. The default: emit `<input>`

Put the editable part of a row in an `<input>` tag:

```rust
use sicompass_sdk::tags;

FfonElement::new_str(format!("<id>{id}</id>{}", tags::format_input(&text)))
```

Text before and after the tag is drawn as a non-editable prefix and suffix. When
the user presses `i` or `a` on the row, the app's Insert mode takes over, and you
get all of the following without writing any editing code:

| Key | Does |
|---|---|
| typing, Backspace, Delete, Left/Right | edit at the caret |
| `Ctrl+Enter` | insert a new line (`\n`) |
| Up / Down | same column on the previous or next **visual** line (a soft wrap counts as a line). Up on the first line goes to the start, Down on the last goes to the end |
| Shift + arrows, Shift+Home/End | extend the selection |
| Home / End | start or end of the current line |
| `Enter` | confirm: the app calls your `commit_edit(old, new)` |
| Escape | leave Insert mode |
| `Ctrl+Z` / `Ctrl+Y` | undo and redo each typed chunk (see [undo-redo-timeline.md](undo-redo-timeline.md)) |

The field wraps at the content width, draws one rounded `selected` block behind
the whole buffer, and announces itself to the screen reader like any other row.

Use `<password>` instead of `<input>` for a secret. It edits identically and is
drawn masked.

### Committing

`commit_edit(old, new)` receives the text with the tag stripped. It can contain
`\n`, so store it in a format that keeps newlines (JSON does, a one-line-per-record
file does not) and put it back into the tag unchanged on the next `fetch`.

The FFON label is markup, so text that could look like a tag must be escaped.
`lib_gitclient`, and the notes and project management plugins
(`../notes_plugin_sicompass`, `../projectmanagement_plugin_sicompass`), each carry a small
`escape.rs` that escapes `<` and `>` on the way out and undoes it in
`commit_edit`. Copy that pattern.

Two capability flags change what happens around an edit:

- `supports_structural_edit() -> true` lets the app add, delete and reorder your
  rows with its generic keys, and hands you the result in
  `sync_ffon_body_children`. Notes and project management use it.
- `has_editor_semantics() -> true` means "each row is one line of a document":
  Enter adds a row and leaving Insert commits. The text editor and the email body
  use it. Pick it when a line is the natural unit of your data (source code,
  mail). Pick a plain multiline `<input>` when a whole paragraph is one value
  (a note, a commit message, a card).

## 2. A surface the app does not draw: `sicompass_sdk::input`

A dashboard receives raw keys in `dashboard_key` and text in `dashboard_text`, and
draws its own cell grid. It has no `<input>` to lean on, so it edits text itself.
Build that on the SDK model, not on a fresh `String` and caret. The project
management board (`../projectmanagement_plugin_sicompass`) is the worked example. A WASM
plugin gets the same module as `sicompass_pdk::input`.

The model has two parts:

- `InputLine { start, end, indent_cols }`: one visual line, as a byte range into
  the text and the columns it is indented by on screen.
- `InputState { text, caret, anchor, goal_col }`: the field. Positions are **byte
  offsets**, columns are **characters**. That is exact for the app's monospace
  font and for a cell grid.

### The recipe

1. **Lay out once.** Turn the text into lines with
   `input::wrap_cells(text, first_cols, rest_cols, rest_indent)`. It honours `\n`,
   keeps repeated spaces, and splits words longer than the line. The board wraps a
   card with a two-cell hanging indent (`render::card_lines`).
2. **Draw from those lines.** One row of cells per `InputLine`, indented by
   `indent_cols`.
3. **Draw one background block.** Set `DashboardFrame::selection` to a single
   `DashboardSelection` covering every line of the field. The app paints it as one
   rounded shape. Do not fill each line separately.
4. **Place the caret from the same lines.** `input::line_of(text, &lines, caret)`
   gives `(line, column)`, indent included. Set `DashboardFrame::cursor`, and in a
   native provider `cursor_style: DashboardCursor::Bar` (a WASM plugin's cursor
   is always a filled cell, see [wasm-plugins.md](wasm-plugins.md#known-limits)).
5. **Route keys to `InputState`.** `insert_str`, `backspace`, `delete_forward`,
   `left`, `right`, `home`, `end`, and `up`/`down(&lines, extend)`. Pass the lines
   from step 1 **for the width you last drew at** (remember it in
   `dashboard_render`), so Up/Down move through what the user saw.
6. **Keep the keys the app uses.** Enter confirms, `Ctrl+Enter` inserts `"\n"`,
   Escape leaves. A field that answers to different keys than every other field is
   a field users get wrong.

```rust
fn insert_key(&mut self, key: DashboardKey) -> bool {
    use DashboardKeysym as K;
    let lines = self.edit_lines(); // wrap_cells at the last drawn width
    let Some(field) = self.field.as_mut() else { return false };
    match key.keysym {
        K::Enter if key.ctrl => field.insert_str("\n"),
        K::Enter | K::Escape => { self.commit(); return true }
        K::Backspace => field.backspace(),
        K::Up => field.up(&lines, key.shift),
        K::Down => field.down(&lines, key.shift),
        // ...
        _ => return false,
    }
    true
}
```

## 3. Customisation points

| You want | Do |
|---|---|
| a label before the value | put it before the `<input>` tag. It is the prefix, and the first line's `indent_cols` |
| a unit or hint after the value | put it after the tag (the suffix) |
| a hanging indent on wrapped lines | `rest_indent` in `wrap_cells` |
| a narrower first line | `first_cols` smaller than `rest_cols` |
| one row per line of a document | `has_editor_semantics() -> true` instead of a multiline value |
| a secret | `<password>` |

## 4. Checklist

- One layout (a single `Vec<InputLine>`) feeds drawing, caret, selection and
  Up/Down. If you compute wrapping twice, they will disagree on some input.
- One background block for the field, not one per line.
- Up on the first line goes to the start, Down on the last goes to the end.
- A run of Up/Down keeps its column across short lines (`goal_col`).
- Byte offsets for positions, characters for columns. Test with `é` and an
  emoji.
- `\n` survives `commit_edit`, storage and the next `fetch`.
- Tests: a wrapped single line walked with Up/Down, a `Ctrl+Enter` line crossed with
  Up/Down, both edges, and a render of text containing `\n`.
