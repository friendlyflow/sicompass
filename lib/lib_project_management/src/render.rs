//! Drawing the board into a [`DashboardFrame`].
//!
//! # Colour is the only tool, so spend it on one thing
//!
//! The app's cell renderer reads `ch`, `fg` and `bg` and nothing else — the
//! `CellAttrs` flags (bold, underline, reverse) cross the SDK boundary and are
//! then never applied. Every piece of visual structure here is therefore a
//! foreground/background pair, and "emphasis" always means "a different fill".
//!
//! With fill as the only channel, the palette stays deliberately small: two
//! resting fills, one for column heads and one for cards, and a third reserved
//! for **whatever the cursor is on**. Nothing else is coloured. A board that
//! tints each column its own hue spends the one channel that can carry focus on
//! decoration instead, and then focus needs a glyph marker to be found at all.
//!
//! # Only cards are focusable
//!
//! A column head is a label, not a destination. The cursor always sits on a
//! card, or on the placeholder slot an empty column shows in place of one, which
//! is what gives an empty column somewhere to insert the first card. Columns
//! themselves are created, renamed and reordered in the list view.

use crate::board::Board;
use sicompass_sdk::input::{self, InputLine};
use sicompass_sdk::{
    CellAttrs, DashboardCell, DashboardCursor, DashboardFrame, DashboardPalette, DashboardSelection,
};

// ---------------------------------------------------------------------------
// Geometry
// ---------------------------------------------------------------------------

/// Cells between two columns.
///
/// Two, not one, and not only for looks: it is also the separator that makes a
/// row of the grid unambiguous to read back. One space can occur inside a card's
/// own wrapped text, two never do.
pub const GUTTER: u16 = 2;

/// Below this a column is unreadable, so the board scrolls sideways instead of
/// shrinking further.
pub const MIN_COL_W: u16 = 16;

/// Cells of air outside the first and last columns.
///
/// The same on both sides, which is the whole point: the columns used to start
/// flush at zero and end wherever the width happened to divide, so the board sat
/// against the left edge with a ragged gap on the right. Matching the gutter
/// makes one rule for all the horizontal space — before the first column,
/// between any two, and after the last.
pub const MARGIN: u16 = GUTTER;

/// Row the column title sits on: the first one.
///
/// No blank row above it. The grid already starts below the app's header line
/// and its separator rule, and the list puts its own first row straight after
/// that — a blank row here made the board sit a whole line lower than every
/// other view for no reason.
pub const HEAD_TOP: u16 = 0;

/// Rows the title occupies. One line of text, like every other line.
pub const HEAD_H: u16 = 1;

/// First row a card can occupy: the row after the title.
///
/// The air between them is [`half_gap_rows`](DashboardFrame::half_gap_rows), not
/// a blank row — half a line, which a grid of whole cells cannot express on its
/// own. A full blank row read as too much separation for a one-line heading.
pub const CARDS_TOP: u16 = HEAD_TOP + HEAD_H;

// ---------------------------------------------------------------------------
// Palette
// ---------------------------------------------------------------------------
//
// There is none of our own. Every colour comes from `DashboardPalette`, which is
// the app's live palette handed over each frame, so the board is drawn in the
// same colours as the list the user just came from and follows the light/dark
// theme without knowing which one is active.
//
// The mapping is deliberately the list's own vocabulary, not a new one:
//
// * a resting card has **no fill**, exactly like an unselected row;
// * the focused card takes `selected`, exactly like the selected row;
// * a column head takes `header_sep`, the app's own chrome divider, which is
//   subtle against the background in both themes;
// * everything written is `text`.
//
// That is the whole palette. A fourth colour would be a convention the rest of
// the app does not have.

/// A cell with alpha 0 draws no fill, so the window ground shows through.
pub const TRANSPARENT: u32 = 0x00000000;

// ---------------------------------------------------------------------------
// What is focused
// ---------------------------------------------------------------------------

/// The board cursor. Always on a card slot.
///
/// `row` indexes the column's *slots*, which are its cards, except in an empty
/// column where the single slot is the placeholder. [`Board`] cannot express
/// that, so [`slots`] is the one place the difference lives.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Focus {
    pub col: usize,
    pub row: usize,
}

/// How many focusable slots a column has: its cards, or one placeholder.
pub fn slots(board: &Board, col: usize) -> usize {
    board
        .columns
        .get(col)
        .map(|c| c.cards.len().max(1))
        .unwrap_or(0)
}

/// True when the column shows a placeholder rather than cards.
pub fn is_placeholder(board: &Board, col: usize) -> bool {
    board.columns.get(col).is_some_and(|c| c.cards.is_empty())
}

/// What the board is showing, which decides the status line and the cursor.
pub struct View<'a> {
    pub focus: Focus,
    /// The in-progress text when insert mode is open, and the caret's position
    /// as a byte offset into it.
    pub editing: Option<(&'a str, usize)>,
    /// What an empty column shows in place of its cards.
    pub empty_label: &'a str,
    /// What a board with no columns at all shows.
    pub no_columns_label: &'a str,
    /// The app's live palette. Every colour below comes from here.
    pub palette: DashboardPalette,
}

// ---------------------------------------------------------------------------
// Layout
// ---------------------------------------------------------------------------

/// Where each visible column starts, and how wide it is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Layout {
    /// Index of the leftmost visible column.
    pub first: usize,
    /// How many columns are drawn.
    pub visible: usize,
    /// Base width. The first [`Layout::wide`] visible columns get one cell more.
    pub width: u16,
    /// How many leading columns carry the extra cell.
    ///
    /// The width rarely divides evenly, and dropping the remainder leaves it as
    /// dead space past the last column — a margin on the right that the left
    /// edge has no counterpart for. Handing one cell each to the leading columns
    /// spends it instead, so the last column ends exactly at the right edge.
    pub wide: usize,
}

impl Layout {
    /// Width of the column at `index`.
    pub fn width_of(&self, index: usize) -> u16 {
        if index.saturating_sub(self.first) < self.wide {
            self.width + 1
        } else {
            self.width
        }
    }

    /// x of the column at `index`, counted from `first`.
    pub fn x_of(&self, index: usize) -> u16 {
        let vis = index.saturating_sub(self.first);
        let wide_before = vis.min(self.wide) as u16;
        MARGIN + (vis as u16) * (self.width + GUTTER) + wide_before
    }
}

/// Fit `n` columns into `cols` cells, scrolled so `focused` is on screen.
///
/// Columns share the width evenly while they all fit, and stop shrinking at
/// [`MIN_COL_W`]; past that the board scrolls sideways rather than rendering
/// columns too narrow to read.
pub fn layout(n: usize, focused: usize, cols: u16) -> Layout {
    if n == 0 {
        return Layout {
            first: 0,
            visible: 0,
            width: cols.saturating_sub(2 * MARGIN),
            wide: 0,
        };
    }
    // Everything but the two outer margins is column-or-gutter.
    let inner = cols.saturating_sub(2 * MARGIN);
    // How many fit at the minimum width, capped at how many there are. One
    // expression for both cases: when they all fit this is just `n`, and the
    // share-out below then spreads the full width across them.
    let per = MIN_COL_W + GUTTER;
    let visible = (((inner + GUTTER) / per) as usize).max(1).min(n);
    let first = if focused < visible {
        0
    } else {
        (focused + 1).saturating_sub(visible).min(n - visible)
    };
    let v = visible.min(u16::MAX as usize) as u16;
    let usable = inner.saturating_sub(GUTTER.saturating_mul(v.saturating_sub(1)));
    Layout {
        first,
        visible,
        width: (usable / v.max(1)).max(1),
        wide: (usable % v.max(1)) as usize,
    }
}

/// Rows one card needs at `width`: its wrapped text, and nothing more.
///
/// No separator row. A column is a list, and the list this board lives in packs
/// its rows one per line; the cursor's fill is what says where one ends.
pub fn card_height(text: &str, width: u16) -> u16 {
    card_lines(text, width).len().max(1) as u16
}

/// A card's visual lines at `width`: wrapped to the text width, every line
/// starting flush.
///
/// The one layout the drawing, the caret and the editor's Up/Down all read, so
/// a card cannot be drawn one way and navigated another.
pub fn card_lines(text: &str, width: u16) -> Vec<InputLine> {
    let w = text_width(width) as usize;
    input::wrap_cells(text, w, w, 0)
}

/// Cells of air on each side of a card's text inside its focus block.
///
/// The list leaves ground either side of the label it has selected, and a card
/// does the same. Without it the block sat flush against the first character
/// and a cell clear of the last, so the text read as shoved left inside its own
/// fill.
///
/// It is spent out of [`MARGIN`] and [`GUTTER`] — the cells already set aside as
/// space between columns — rather than out of the card. So the text does not
/// move, a card still starts exactly where its title does, and two neighbouring
/// blocks still have a cell between them.
pub const FOCUS_PAD: u16 = 1;

/// Usable text width inside a card: the whole column.
///
/// No padding of its own: text starts flush at the column's left edge, so the
/// space before it is exactly [`MARGIN`] — the same as the space after the last
/// column and, near enough on a cell grid, the blank row above the titles. An
/// extra cell of internal padding made the left inset three cells against one
/// row at the top, which is the asymmetry you actually see.
///
/// Nothing is reserved for a hanging indent either. Continuation lines start
/// flush, which is what the app's own Insert mode does with a field that wraps
/// or that `Ctrl+Enter` has split (`rest_x: text_prefix_x` in its renderer). A
/// card that indented them answered to a rule no other text field in the app
/// has, and put the caret two cells in the moment you pressed `Ctrl+Enter`.
fn text_width(width: u16) -> u16 {
    width.max(1)
}

/// Wrap on word boundaries, breaking a word longer than the line rather than
/// letting it run off the edge. `\n` starts a new line.
pub fn wrap(text: &str, width: u16) -> Vec<String> {
    let w = width.max(1) as usize;
    line_texts(text, &input::wrap_cells(text, w, w, 0))
        .into_iter()
        .map(str::to_owned)
        .collect()
}

/// The text of each visual line.
fn line_texts<'t>(text: &'t str, lines: &[InputLine]) -> Vec<&'t str> {
    lines.iter().map(|l| &text[l.start..l.end]).collect()
}

/// The focus block behind a card drawn at `x`: where it starts, and how wide.
///
/// Fitted to the card, not to the column, because that is what the list does
/// with the row it has selected — its block is the text's own width plus a
/// little air, never the window's. A band the full width of the column read as a
/// different convention, and on a half-empty column it was mostly empty fill.
///
/// [`FOCUS_PAD`] on each side, so the text sits in the middle of its own fill
/// rather than against one edge of it. The right-hand cell is also where `a`
/// parks the caret, one past the last character.
///
/// The same pair feeds the fill and the [`DashboardSelection`], and it has to:
/// the app skips per-cell backgrounds inside the region the frame names, so a
/// cell filled `selected` outside it comes back as a square blob beside the
/// rounded one, and an unfilled cell inside it decides the colour of the whole
/// shape.
fn focus_block(texts: &[&str], lines: &[InputLine], x: u16, width: u16, cols: u16) -> (u16, u16) {
    let widest = texts
        .iter()
        .zip(lines)
        .map(|(t, l)| l.indent_cols + t.chars().count())
        .max()
        .unwrap_or(0)
        .min(width as usize) as u16;
    let bx = x.saturating_sub(FOCUS_PAD);
    let bw = (widest + (x - bx) + FOCUS_PAD)
        .min(cols.saturating_sub(bx))
        .max(1);
    (bx, bw)
}

/// First card to draw in a column so `focused` stays on screen.
///
/// Only the focused column scrolls; the rest start at the top, so the board does
/// not rearrange itself under the cursor as it moves sideways.
fn scroll_for(cards: &[String], focused: Option<usize>, width: u16, avail: u16) -> usize {
    let Some(target) = focused else { return 0 };
    let heights: Vec<u16> = cards.iter().map(|c| card_height(c, width)).collect();
    let mut first = 0usize;
    loop {
        let used: u16 = heights[first..=target].iter().copied().sum();
        let budget = avail.saturating_sub(if first > 0 { 1 } else { 0 });
        if used <= budget || first >= target {
            return first;
        }
        first += 1;
    }
}

// ---------------------------------------------------------------------------
// Drawing
// ---------------------------------------------------------------------------

/// Paint a run of cells, **both** colours.
///
/// Setting only `bg` leaves the blank cells past the end of a line carrying the
/// frame's default foreground, and the app draws its text cursor by filling a
/// cell with that cell's own `fg`. On a light-filled focused card those blanks
/// were near-white on near-white, so the caret vanished exactly where `a` puts
/// it: at the end of the text.
fn fill(frame: &mut DashboardFrame, x: u16, y: u16, w: u16, fg: u32, bg: u32) {
    for dx in 0..w {
        if x + dx < frame.cols && y < frame.rows {
            let cell = frame.cell_mut(x + dx, y);
            cell.fg = fg;
            cell.bg = bg;
        }
    }
}

fn put(frame: &mut DashboardFrame, x: u16, y: u16, s: &str, fg: u32, bg: u32, w: u16) {
    if y >= frame.rows {
        return;
    }
    for (cx, ch) in (x..).zip(s.chars()) {
        if cx >= x + w || cx >= frame.cols {
            break;
        }
        let cell = frame.cell_mut(cx, y);
        cell.ch = ch;
        cell.fg = fg;
        cell.bg = bg;
        cell.attrs = CellAttrs::default();
    }
}

/// Draw the whole board.
pub fn render(board: &Board, view: &View<'_>, cols: u16, rows: u16) -> DashboardFrame {
    let text = view.palette.text;
    let mut frame = DashboardFrame {
        cols,
        rows,
        cells: vec![
            DashboardCell {
                ch: ' ',
                fg: text,
                bg: TRANSPARENT,
                attrs: CellAttrs::default(),
            };
            (cols as usize) * (rows as usize)
        ],
        cursor: None,
        selection: None,
        // Half a line under every column title, which is the same row for all of
        // them.
        half_gap_rows: vec![HEAD_TOP],
        // A card is a text field, so its caret is the app's own thin blinking
        // bar, not a shell's filled cell.
        cursor_style: DashboardCursor::Bar,
    };

    if board.columns.is_empty() {
        put(
            &mut frame,
            MARGIN,
            HEAD_TOP,
            view.no_columns_label,
            text,
            TRANSPARENT,
            cols.saturating_sub(MARGIN),
        );
        return frame;
    }

    let lay = layout(board.columns.len(), view.focus.col, cols);

    for index in lay.first..(lay.first + lay.visible).min(board.columns.len()) {
        let col = &board.columns[index];
        let x = lay.x_of(index);
        let width = lay.width_of(index);
        let is_focus_col = index == view.focus.col;

        // ---- Title: text, and nothing else ---------------------------------
        //
        // No fill. The list has exactly two backgrounds — none, and `selected` on
        // the row the cursor is on — so a filled band here would be a third
        // convention the rest of the app does not have. Uppercase and a blank row
        // below are what make it read as a heading, and neither costs a colour.
        put(
            &mut frame,
            x,
            HEAD_TOP,
            &col.title,
            text,
            TRANSPARENT,
            width,
        );

        // ---- The placeholder an empty column shows in place of cards --------
        if col.cards.is_empty() {
            let focused = is_focus_col;
            let (fg, bg) = if focused {
                (text, view.palette.selected)
            } else {
                (text, TRANSPARENT)
            };
            // A card being typed into an empty column appears in the placeholder
            // slot until it commits, so the hint gives way to what is typed.
            //
            // Gated on `focused`, and that gate is the whole point: `view.editing`
            // is the one edit happening anywhere on the board, so an ungated read
            // made *every* empty column echo whatever the user was typing into
            // some other column entirely.
            let shown = match (focused, view.editing) {
                (true, Some((t, _))) => t,
                _ => view.empty_label,
            };
            // Wrapped, not clipped: the hint is a localized sentence and a
            // narrow column would otherwise cut it mid-word in some languages
            // and not others.
            let cells = card_lines(shown, width);
            let lines = line_texts(shown, &cells);
            let (bx, bw) = focus_block(&lines, &cells, x, width, cols);
            for (li, line) in lines.iter().enumerate() {
                let ly = CARDS_TOP + li as u16;
                fill(&mut frame, bx, ly, bw, fg, bg);
                put(&mut frame, x, ly, line, fg, bg, width);
            }
            if focused {
                frame.selection = Some(DashboardSelection {
                    col: bx,
                    row: CARDS_TOP,
                    cols: bw,
                    rows: lines.len() as u16,
                });
                if let Some((_, caret)) = view.editing {
                    frame.cursor = Some(caret_cell(shown, &cells, caret, x, CARDS_TOP, width));
                }
            }
            continue;
        }

        // ---- Cards ---------------------------------------------------------
        let texts: Vec<String> = col
            .cards
            .iter()
            .enumerate()
            .map(|(i, c)| {
                if is_focus_col && view.focus.row == i {
                    view.editing
                        .map(|(t, _)| t.to_owned())
                        .unwrap_or_else(|| c.text.clone())
                } else {
                    c.text.clone()
                }
            })
            .collect();

        let avail = rows.saturating_sub(CARDS_TOP);
        let focused_row = is_focus_col.then_some(view.focus.row.min(texts.len() - 1));
        let first = scroll_for(&texts, focused_row, width, avail);

        if first > 0 {
            put(
                &mut frame,
                x,
                CARDS_TOP,
                &format!("↑ {first} more"),
                text,
                TRANSPARENT,
                width,
            );
        }
        let mut y = CARDS_TOP + if first > 0 { 1 } else { 0 };

        let mut drawn = first;
        for (i, card_text) in texts.iter().enumerate().skip(first) {
            let cells = card_lines(card_text, width);
            let lines = line_texts(card_text, &cells);
            let h = lines.len() as u16;
            if y + h > rows {
                break;
            }
            let focused = focused_row == Some(i);
            let (fg, bg) = if focused {
                (text, view.palette.selected)
            } else {
                (text, TRANSPARENT)
            };
            let (bx, bw) = focus_block(&lines, &cells, x, width, cols);
            for (li, line) in lines.iter().enumerate() {
                let ly = y + li as u16;
                fill(&mut frame, bx, ly, bw, fg, bg);
                put(&mut frame, x, ly, line, fg, bg, width);
            }
            if focused {
                // Named so the app paints it as one rounded shape rather than a
                // stack of square per-row fills: corner rounding is per
                // rectangle, and a cell grid cannot ask for it at all.
                //
                // The region is exactly the rows the card covers and the cells
                // [`focus_block`] measured, which is what makes it the same
                // shape as the list's selected row. The air around it is named
                // here too, rather than being shaved off by the app.
                frame.selection = Some(DashboardSelection {
                    col: bx,
                    row: y,
                    cols: bw,
                    rows: h,
                });
                if let Some((_, caret)) = view.editing {
                    frame.cursor = Some(caret_cell(card_text, &cells, caret, x, y, width));
                }
            }
            y += h;
            drawn = i + 1;
        }

        let hidden = texts.len().saturating_sub(drawn);
        if hidden > 0 && rows > 0 {
            put(
                &mut frame,
                x,
                rows - 1,
                &format!("↓ {hidden} more"),
                text,
                TRANSPARENT,
                width,
            );
        }
    }

    frame
}

/// Where the text caret lands, given the wrapped lines it sits in.
///
/// `x` is where the *first* line starts; later lines carry the hanging indent, so
/// the caret has to as well or it drifts left of the text it is sitting in.
/// [`input::line_of`] counts that indent in its column.
fn caret_cell(
    text: &str,
    lines: &[InputLine],
    caret: usize,
    x: u16,
    y: u16,
    width: u16,
) -> (u16, u16) {
    let (li, col) = input::line_of(text, lines, caret.min(text.len()));
    let cx = (x as usize + col).min(u16::MAX as usize) as u16;
    (cx.min(x + width.saturating_sub(1)), y + li as u16)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::board::{Card, Column};

    fn board(shape: &[(&str, &[&str])]) -> Board {
        let mut b = Board::new();
        let mut id = 1;
        for (title, cards) in shape {
            let mut col = Column::new(id, *title);
            id += 1;
            for c in *cards {
                col.cards.push(Card::new(id, *c));
                id += 1;
            }
            b.columns.push(col);
        }
        b.reseat_counter();
        b
    }

    /// The palette the app hands over. Named once so a test asserts against the
    /// same values the board is drawn from.
    fn pal() -> DashboardPalette {
        DashboardPalette::default()
    }

    fn view(focus: Focus) -> View<'static> {
        View {
            focus,
            editing: None,
            empty_label: "(empty)",
            no_columns_label: "no columns yet",
            palette: pal(),
        }
    }

    /// The row of text at `y`, trailing blanks trimmed.
    fn row_text(f: &DashboardFrame, y: u16) -> String {
        (0..f.cols)
            .map(|x| f.cell(x, y).ch)
            .collect::<String>()
            .trim_end()
            .to_owned()
    }

    /// Every distinct background actually painted in the frame.
    fn fills(f: &DashboardFrame) -> std::collections::BTreeSet<u32> {
        f.cells.iter().map(|c| c.bg).collect()
    }

    // ---- Layout ---------------------------------------------------------

    #[test]
    fn columns_share_the_width_while_they_all_fit() {
        // 80 cells, two outer margins and two gutters, so 72 across three
        // columns: 24 each.
        let lay = layout(3, 0, 80);
        assert_eq!(lay.first, 0);
        assert_eq!(lay.visible, 3);
        assert_eq!(lay.width_of(0), 24);
        assert_eq!(lay.x_of(0), MARGIN);
        assert_eq!(lay.x_of(1), MARGIN + 26);
        assert_eq!(lay.x_of(2), MARGIN + 52);
    }

    /// The two outer margins must come out equal.
    ///
    /// The share-out rarely divides evenly, and dropping the remainder used to
    /// dump it past the last column: a wide ragged gap on the right against a
    /// flush left edge. Spending it on the leading columns keeps both sides at
    /// exactly [`MARGIN`].
    fn ends_flush(n: usize, cols: u16) {
        let lay = layout(n, 0, cols);
        let last = lay.first + lay.visible - 1;
        assert_eq!(
            lay.x_of(lay.first),
            MARGIN,
            "{n} columns in {cols} cells: wrong margin on the left"
        );
        assert_eq!(
            cols - (lay.x_of(last) + lay.width_of(last)),
            MARGIN,
            "{n} columns in {cols} cells: the right margin does not match the left"
        );
    }

    #[test]
    fn the_margins_stay_equal_whatever_the_remainder() {
        for cols in [20u16, 40, 60, 79, 80, 81, 100, 112, 113, 200] {
            for n in 1..=8usize {
                ends_flush(n, cols);
            }
        }
    }

    #[test]
    fn the_extra_cells_go_to_the_leading_columns_one_each() {
        // 112 cells, two margins and three gutters, 102 across four: 25 each
        // with 2 left over.
        let lay = layout(4, 0, 112);
        assert_eq!(lay.width, 25);
        assert_eq!(lay.wide, 2);
        let widths: Vec<u16> = (0..4).map(|i| lay.width_of(i)).collect();
        assert_eq!(widths, vec![26, 26, 25, 25]);
        assert_eq!(widths.iter().sum::<u16>() + GUTTER * 3 + MARGIN * 2, 112);
    }

    #[test]
    fn a_single_column_takes_the_grid_less_its_margins() {
        assert_eq!(layout(1, 0, 80).width, 80 - 2 * MARGIN);
        ends_flush(1, 80);
    }

    #[test]
    fn columns_stop_shrinking_and_the_board_scrolls_sideways() {
        let lay = layout(7, 0, 80);
        assert!(lay.visible < 7, "not every column fits");
        for i in lay.first..lay.first + lay.visible {
            assert!(
                lay.width_of(i) >= MIN_COL_W,
                "a visible column shrank below the readable minimum"
            );
        }
        ends_flush(7, 80);
    }

    #[test]
    fn scrolling_sideways_keeps_the_focused_column_on_screen() {
        let lay = layout(7, 6, 80);
        assert!(6 >= lay.first && 6 < lay.first + lay.visible);
    }

    // ---- The palette ----------------------------------------------------

    #[test]
    fn every_fill_comes_from_the_apps_palette() {
        // The whole point: the board invents no colours. A fill that is not one
        // of these is one the rest of the app does not have, and it will not
        // follow the light/dark theme either.
        let b = board(&[
            ("A", &["one", "two"]),
            ("B", &["three"]),
            ("C", &[]),
            ("D", &["four"]),
        ]);
        let p = pal();
        let f = render(&b, &view(Focus { col: 0, row: 1 }), 90, 20);
        let used = fills(&f);
        assert!(
            used.is_subset(&[TRANSPARENT, p.selected].into()),
            "the list has two backgrounds, none and selected: {used:x?}"
        );
    }

    #[test]
    fn every_glyph_is_drawn_in_the_palettes_text_colour() {
        let b = board(&[("A", &["one"]), ("B", &[])]);
        let p = pal();
        let f = render(&b, &view(Focus { col: 0, row: 0 }), 60, 20);
        for cell in &f.cells {
            assert_eq!(cell.fg, p.text, "a glyph in a colour the app does not use");
        }
    }

    #[test]
    fn a_theme_switch_reaches_the_board() {
        // The app hands the palette over every frame, so the light theme is not
        // a special case here — it is just a different set of numbers.
        let light = DashboardPalette {
            background: 0xFFFFFFFF,
            text: 0x000000FF,
            header_sep: 0xE0E0E0FF,
            selected: 0xC0ECB8FF,
            ..DashboardPalette::default()
        };
        let b = board(&[("A", &["one"])]);
        let v = View {
            focus: Focus { col: 0, row: 0 },
            editing: None,
            empty_label: "(empty)",
            no_columns_label: "no columns yet",
            palette: light,
        };
        let f = render(&b, &v, 40, 20);
        assert_eq!(
            f.cell(2, HEAD_TOP).fg,
            light.text,
            "the title follows the theme"
        );
        assert_eq!(f.cell(2, CARDS_TOP).bg, light.selected);
        assert_eq!(f.cell(2, CARDS_TOP).fg, light.text);
    }

    #[test]
    fn a_column_title_is_text_on_nothing() {
        let b = board(&[("A", &["x"]), ("B", &["y"]), ("C", &["z"])]);
        let f = render(&b, &view(Focus { col: 0, row: 0 }), 90, 20);
        let lay = layout(3, 0, 90);
        for index in 0..3 {
            let x = lay.x_of(index);
            for dx in 0..lay.width {
                assert_eq!(
                    f.cell(x + dx, HEAD_TOP).bg,
                    TRANSPARENT,
                    "column {index} title at offset {dx} must not be filled"
                );
            }
        }
        assert!(row_text(&f, HEAD_TOP).contains('A'));
    }

    #[test]
    fn half_a_line_of_air_separates_the_title_from_its_first_card() {
        // Half, not a blank row: a blank row is a whole line and reads as too
        // much under a one-line heading. The grid cannot express half a row, so
        // the frame names the gap and the app opens it in pixels.
        let b = board(&[("To do", &["fix login"])]);
        let f = render(&b, &view(Focus { col: 0, row: 0 }), 40, 20);
        assert_eq!(HEAD_H, 1);
        assert_eq!(CARDS_TOP, HEAD_TOP + 1, "no blank row is spent on it");
        assert_eq!(row_text(&f, HEAD_TOP).trim(), "To do");
        assert_eq!(row_text(&f, CARDS_TOP).trim(), "fix login");
        assert_eq!(f.half_gap_rows, vec![HEAD_TOP], "the gap is named instead");
        assert_eq!(f.half_gaps_above(HEAD_TOP), 0);
        assert_eq!(
            f.half_gaps_above(CARDS_TOP),
            1,
            "the cards sit half a line down"
        );
    }

    #[test]
    fn the_board_draws_no_status_line_of_its_own() {
        // The app's header already names the mode and the position; a second one
        // here only repeated it, and cost the board a row of its own.
        let b = board(&[("To do", &["fix login"])]);
        let f = render(&b, &view(Focus { col: 0, row: 0 }), 40, 20);
        assert_eq!(HEAD_TOP, 0, "the titles start on the first row");
        assert_eq!(row_text(&f, 0).trim(), "To do");
    }

    #[test]
    fn exactly_one_thing_wears_the_focus_fill() {
        let b = board(&[("A", &["one", "two"]), ("B", &["three"])]);
        let f = render(&b, &view(Focus { col: 1, row: 0 }), 90, 20);
        let focused: Vec<(u16, u16)> = (0..f.rows)
            .flat_map(|y| (0..f.cols).map(move |x| (x, y)))
            .filter(|(x, y)| f.cell(*x, *y).bg == pal().selected)
            .collect();
        assert!(!focused.is_empty(), "the cursor must be visible");
        let rows: std::collections::BTreeSet<u16> = focused.iter().map(|(_, y)| *y).collect();
        assert_eq!(rows.len(), 1, "one single-line card, one focused row");
        let lay = layout(2, 1, 90);
        let x = lay.x_of(1);
        assert!(
            focused
                .iter()
                .all(|(fx, _)| *fx >= x - FOCUS_PAD && *fx < x + lay.width + FOCUS_PAD),
            "focus must stay inside its own column, give or take its cell of air"
        );
        assert!(
            focused.iter().all(|(fx, _)| *fx < x + lay.width),
            "and the air on the left is the only cell it may borrow"
        );
    }

    #[test]
    fn the_focused_card_is_named_as_one_selection_region() {
        // One region, not one per row: corner rounding is per rectangle, so a
        // multi-row selection drawn row by row comes out as stacked blobs.
        let b = board(&[("To do", &["a card long enough to wrap over", "after"])]);
        let cols = 24;
        let f = render(&b, &view(Focus { col: 0, row: 0 }), cols, 20);
        let lay = layout(1, 0, cols);
        let n = wrap(
            "a card long enough to wrap over",
            text_width(lay.width_of(0)),
        )
        .len() as u16;
        assert!(n > 1, "the fixture must actually wrap");
        let sel = f.selection.expect("the focused card names a selection");
        assert_eq!(sel.row, CARDS_TOP);
        assert_eq!(sel.rows, n, "the region spans the whole card");
        assert_eq!(sel.col, lay.x_of(0) - FOCUS_PAD);
    }

    #[test]
    fn an_unfocused_board_still_names_exactly_one_selection() {
        let b = board(&[("A", &["x"]), ("B", &["y"])]);
        let f = render(&b, &view(Focus { col: 1, row: 0 }), 60, 20);
        let sel = f.selection.expect("something is always focused");
        let lay = layout(2, 1, 60);
        assert_eq!(
            sel.col,
            lay.x_of(1) - FOCUS_PAD,
            "in the focused column, less its cell of air"
        );
        assert_eq!(sel.rows, 1);
    }

    #[test]
    fn an_empty_columns_slot_is_a_selection_region_too() {
        let b = board(&[("Doing", &[])]);
        let f = render(&b, &view(Focus { col: 0, row: 0 }), 40, 20);
        let sel = f.selection.expect("the placeholder holds the cursor");
        assert_eq!(sel.row, CARDS_TOP);
        assert!(sel.rows >= 1);
    }

    #[test]
    fn nothing_marks_the_focus_except_its_fill() {
        // There used to be a `▸` in the gutter. Focus is a fill and nothing else,
        // so the gutter stays empty.
        let b = board(&[("A", &["x"]), ("B", &["y"])]);
        let f = render(&b, &view(Focus { col: 1, row: 0 }), 60, 20);
        let lay = layout(2, 1, 60);
        let x = lay.x_of(1);
        for y in 0..f.rows {
            assert_eq!(f.cell(x - 1, y).ch, ' ', "gutter at row {y} must be blank");
            assert_eq!(f.cell(x - 2, y).ch, ' ', "gutter at row {y} must be blank");
        }
    }

    #[test]
    fn cards_sit_on_consecutive_rows_like_list_rows() {
        // No blank between them: a column is a list, and the list this board
        // lives in packs its rows one per line.
        let b = board(&[("To do", &["one", "two", "three"])]);
        let f = render(&b, &view(Focus { col: 0, row: 0 }), 40, 20);
        assert_eq!(row_text(&f, CARDS_TOP).trim(), "one");
        assert_eq!(row_text(&f, CARDS_TOP + 1).trim(), "two");
        assert_eq!(row_text(&f, CARDS_TOP + 2).trim(), "three");
    }

    #[test]
    fn every_line_of_a_card_starts_flush_like_the_apps_own_text_fields() {
        // A wrapped line used to hang two cells in. No other field in the app
        // does that — Insert mode puts a continuation line back at the row's
        // left edge — so a card no longer does either.
        let b = board(&[("To do", &["a card long enough to wrap over", "after"])]);
        let cols = 24;
        let f = render(&b, &view(Focus { col: 0, row: 1 }), cols, 20);
        // Derived, not hardcoded: the usable width depends on the margins and
        // gutters, so a literal here would only be right for one of them.
        let w = layout(1, 0, cols).width_of(0);
        let n = wrap("a card long enough to wrap over", text_width(w)).len() as u16;
        assert!(n > 1, "the fixture must actually wrap");

        let first = row_text(&f, CARDS_TOP);
        let cont = row_text(&f, CARDS_TOP + 1);
        let next = row_text(&f, CARDS_TOP + n);
        let lead = |s: &str| s.len() - s.trim_start().len();
        // Text starts flush at the column's left edge, so the only space before
        // it is the outer margin.
        let flush = MARGIN as usize;
        assert_eq!(lead(&first), flush, "a card's first line sits flush");
        assert_eq!(lead(&cont), flush, "and so does its continuation");
        assert_eq!(next.trim(), "after");
        assert_eq!(lead(&next), flush, "and the next card");
    }

    #[test]
    fn a_wrapped_card_is_followed_straight_away_by_the_next() {
        let b = board(&[("To do", &["a card long enough to wrap", "after"])]);
        let cols = 24;
        let f = render(&b, &view(Focus { col: 0, row: 0 }), cols, 20);
        let w = layout(1, 0, cols).width_of(0);
        let n = wrap("a card long enough to wrap", text_width(w)).len() as u16;
        assert!(n > 1, "the fixture must actually wrap");
        assert_eq!(row_text(&f, CARDS_TOP + n).trim(), "after");
    }

    #[test]
    fn typing_in_one_column_does_not_echo_into_an_empty_one() {
        // `view.editing` is the single edit happening anywhere on the board, so
        // reading it without checking which column is focused made every empty
        // column show whatever was being typed somewhere else entirely.
        let b = board(&[("Doing", &["kanban ui"]), ("Done", &[])]);
        let v = View {
            focus: Focus { col: 0, row: 0 },
            editing: Some(("secret in progress", 18)),
            empty_label: "(empty)",
            no_columns_label: "no columns yet",
            palette: pal(),
        };
        let f = render(&b, &v, 60, 20);
        let lay = layout(2, 0, 60);
        let right: String = (CARDS_TOP..20)
            .map(|y| {
                (lay.x_of(1)..60)
                    .map(|x| f.cell(x, y).ch)
                    .collect::<String>()
            })
            .collect();
        assert!(
            !right.contains("secret"),
            "the empty column echoed the edit: {right:?}"
        );
        assert!(right.contains("(empty)"), "it should still show its hint");
    }

    #[test]
    fn a_title_is_never_filled_so_it_cannot_be_mistaken_for_a_card() {
        let b = board(&[("To do", &["fix login"])]);
        let f = render(&b, &view(Focus { col: 0, row: 0 }), 40, 20);
        for x in 0..40 {
            assert_eq!(f.cell(x, HEAD_TOP).bg, TRANSPARENT);
        }
    }

    #[test]
    fn the_title_reads_exactly_as_the_user_typed_it() {
        // Not uppercased: the board shows the name, it does not restyle it.
        let b = board(&[("To do", &["x"])]);
        let f = render(&b, &view(Focus { col: 0, row: 0 }), 40, 20);
        assert_eq!(row_text(&f, HEAD_TOP).trim(), "To do");
    }

    // ---- The empty-column placeholder -----------------------------------

    #[test]
    fn an_empty_column_shows_a_slot_that_can_hold_the_focus() {
        let b = board(&[("Doing", &[])]);
        let f = render(&b, &view(Focus { col: 0, row: 0 }), 40, 20);
        assert!(row_text(&f, CARDS_TOP).contains("(empty)"));
        assert_eq!(
            f.cell(2, CARDS_TOP).bg,
            pal().selected,
            "the placeholder is a real slot, so it takes the selection fill"
        );
    }

    #[test]
    fn an_unfocused_empty_column_still_shows_its_slot() {
        let b = board(&[("A", &["x"]), ("Doing", &[])]);
        let f = render(&b, &view(Focus { col: 0, row: 0 }), 60, 20);
        let lay = layout(2, 0, 60);
        let x = lay.x_of(1);
        assert_eq!(
            f.cell(x + 2, CARDS_TOP).bg,
            TRANSPARENT,
            "a resting card has no fill, exactly like an unselected list row"
        );
    }

    #[test]
    fn a_long_placeholder_hint_wraps_rather_than_being_cut_off() {
        let b = board(&[("Doing", &[])]);
        let v = View {
            focus: Focus { col: 0, row: 0 },
            editing: None,
            empty_label: "press ctrl+a to add the first card here",
            no_columns_label: "no columns yet",
            palette: pal(),
        };
        let f = render(&b, &v, 20, 20);
        let shown = (0..4)
            .map(|dy| row_text(&f, CARDS_TOP + dy))
            .collect::<Vec<_>>()
            .join(" ")
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        assert_eq!(
            shown, "press ctrl+a to add the first card here",
            "every word of the hint must survive the wrap"
        );
    }

    #[test]
    fn slots_counts_a_placeholder_as_one() {
        let b = board(&[("A", &["x", "y"]), ("B", &[])]);
        assert_eq!(slots(&b, 0), 2);
        assert_eq!(slots(&b, 1), 1, "the placeholder is a slot");
        assert!(is_placeholder(&b, 1));
        assert!(!is_placeholder(&b, 0));
    }

    #[test]
    fn typing_into_an_empty_column_shows_the_text_in_its_slot() {
        let b = board(&[("Doing", &[])]);
        let v = View {
            focus: Focus { col: 0, row: 0 },
            editing: Some(("ship", 4)),
            empty_label: "(empty)",
            no_columns_label: "no columns yet",
            palette: pal(),
        };
        let f = render(&b, &v, 40, 20);
        assert!(row_text(&f, CARDS_TOP).contains("ship"));
        assert!(!row_text(&f, CARDS_TOP).contains("(empty)"));
        assert_eq!(f.cursor, Some((layout(1, 0, 40).x_of(0) + 4, CARDS_TOP)));
    }

    // ---- Cards ----------------------------------------------------------

    #[test]
    fn a_column_taller_than_the_grid_shows_a_more_marker() {
        // `rows - CARDS_TOP` cards fit, so the fixture has to be longer than that.
        let cards: Vec<&str> = vec!["a", "b", "c", "d", "e", "f", "g", "h", "i", "j", "k", "l"];
        let rows = 12;
        assert!(
            cards.len() as u16 > rows - CARDS_TOP,
            "the fixture must actually overflow"
        );
        let b = board(&[("To do", &cards)]);
        let f = render(&b, &view(Focus { col: 0, row: 0 }), 40, rows);
        assert!(
            row_text(&f, rows - 1).contains("more"),
            "expected a marker, got {:?}",
            row_text(&f, rows - 1)
        );
    }

    #[test]
    fn scrolling_down_keeps_the_focused_card_on_screen() {
        let cards: Vec<&str> = vec!["a", "b", "c", "d", "e", "f", "g", "h", "i", "j", "k", "l"];
        let b = board(&[("To do", &cards)]);
        let f = render(&b, &view(Focus { col: 0, row: 11 }), 40, 12);
        let body: String = (0..12)
            .map(|y| row_text(&f, y))
            .collect::<Vec<_>>()
            .join("\n");
        assert!(body.contains('l'), "focused card must be visible:\n{body}");
    }

    #[test]
    fn long_text_wraps_inside_the_card_rather_than_running_off() {
        let lines = wrap("the quick brown fox jumps over the lazy dog", 10);
        assert!(lines.iter().all(|l| l.chars().count() <= 10), "{lines:?}");
        assert_eq!(
            lines.join(" "),
            "the quick brown fox jumps over the lazy dog"
        );
    }

    #[test]
    fn a_word_longer_than_the_line_is_broken_not_dropped() {
        let lines = wrap("supercalifragilistic", 6);
        assert!(lines.iter().all(|l| l.chars().count() <= 6), "{lines:?}");
        assert_eq!(lines.concat(), "supercalifragilistic");
    }

    #[test]
    fn a_newline_in_a_card_starts_a_flush_line() {
        let b = board(&[("To do", &["fix\nlogin"])]);
        let f = render(&b, &view(Focus { col: 0, row: 0 }), 40, 20);
        let x = layout(1, 0, 40).x_of(0);
        assert_eq!(f.cell(x, CARDS_TOP).ch, 'f');
        assert_eq!(
            f.cell(x + 3, CARDS_TOP).ch,
            ' ',
            "nothing after the newline"
        );
        assert_eq!(
            f.cell(x, CARDS_TOP + 1).ch,
            'l',
            "the second line starts under the first, not indented from it"
        );
        assert_eq!(card_height("fix\nlogin", 38), 2);
    }

    #[test]
    fn the_caret_after_a_newline_sits_where_the_new_line_starts() {
        let b = board(&[("To do", &["fix\nlogin"])]);
        let v = View {
            focus: Focus { col: 0, row: 0 },
            editing: Some(("fix\nlogin", "fix\nlo".len())),
            empty_label: "(empty)",
            no_columns_label: "no columns yet",
            palette: pal(),
        };
        let f = render(&b, &v, 40, 20);
        let x = layout(1, 0, 40).x_of(0);
        assert_eq!(
            f.cursor,
            Some((x + 2, CARDS_TOP + 1)),
            "two characters into the second line, and the line starts flush"
        );
    }

    #[test]
    fn a_wrapped_card_carries_its_fill_down_every_line() {
        let b = board(&[("To do", &["a card whose text is long enough to wrap"])]);
        let f = render(&b, &view(Focus { col: 0, row: 0 }), 24, 20);
        assert_eq!(f.cell(2, CARDS_TOP).bg, pal().selected);
        assert_eq!(
            f.cell(2, CARDS_TOP + 1).bg,
            pal().selected,
            "second line too"
        );
    }

    #[test]
    fn the_focus_block_stops_at_the_text_not_at_the_column_edge() {
        // The list fits its selected-row block to the row's text. A band the
        // full width of the column is a different convention, and on a short
        // card it is mostly empty fill.
        let b = board(&[("To do", &["short"])]);
        let f = render(&b, &view(Focus { col: 0, row: 0 }), 40, 20);
        let sel = f.selection.expect("the focused card names a selection");
        let lay = layout(1, 0, 40);
        let x = lay.x_of(0);
        assert_eq!(
            sel.cols,
            "short".chars().count() as u16 + 2 * FOCUS_PAD,
            "the text, with a cell of air on each side of it"
        );
        assert_eq!(sel.col, x - FOCUS_PAD, "the air starts before the text");
        assert_eq!(
            x - sel.col,
            sel.col + sel.cols - (x + "short".chars().count() as u16),
            "and is the same on both sides, so the text sits in the middle"
        );
        assert!(
            sel.cols < lay.width_of(0),
            "and well short of the column edge"
        );
    }

    #[test]
    fn a_card_with_a_newline_is_one_block_as_wide_as_its_longest_line() {
        // `Ctrl+Enter` makes a card two lines. Both share one block, and it is
        // the longer line that sets its width — the second one here, hanging
        // indent included.
        let b = board(&[("To do", &["short\na longer second line"])]);
        let f = render(&b, &view(Focus { col: 0, row: 0 }), 40, 20);
        let sel = f.selection.expect("the focused card names a selection");
        assert_eq!(sel.rows, 2, "the newline made a second line");
        assert_eq!(
            sel.cols,
            "a longer second line".chars().count() as u16 + 2 * FOCUS_PAD,
            "the longer line sets the width"
        );
    }

    #[test]
    fn every_filled_cell_lies_inside_the_named_region() {
        // The app skips per-cell backgrounds inside the region the frame names
        // and paints it as one rounded shape, so the two have to agree exactly:
        // a `selected` cell outside it comes back as a square blob beside the
        // rounded one, and an unfilled cell inside it picks the wrong colour for
        // the whole shape.
        let b = board(&[(
            "To do",
            &["a card whose text is long enough to wrap", "next"],
        )]);
        let f = render(&b, &view(Focus { col: 0, row: 0 }), 24, 20);
        let sel = f.selection.expect("the focused card names a selection");
        for y in 0..f.rows {
            for x in 0..f.cols {
                let inside = x >= sel.col
                    && x < sel.col + sel.cols
                    && y >= sel.row
                    && y < sel.row + sel.rows;
                let filled = f.cell(x, y).bg == pal().selected;
                assert_eq!(filled, inside, "cell ({x}, {y}) disagrees with the region");
            }
        }
    }

    // ---- The caret ------------------------------------------------------

    #[test]
    fn insert_mode_places_the_cursor_at_the_caret() {
        let b = board(&[("To do", &["fix login"])]);
        let v = View {
            focus: Focus { col: 0, row: 0 },
            editing: Some(("fix login", 3)),
            empty_label: "(empty)",
            no_columns_label: "no columns yet",
            palette: pal(),
        };
        let f = render(&b, &v, 40, 20);
        assert_eq!(f.cursor, Some((layout(1, 0, 40).x_of(0) + 3, CARDS_TOP)));
    }

    #[test]
    fn the_caret_is_visible_at_the_end_of_a_card() {
        // The app draws its text cursor by filling the cell with that cell's own
        // `fg`, so a blank cell whose `fg` matches the fill it sits on renders an
        // invisible caret. `a` puts the caret exactly there, past the last
        // character, which is how this shipped broken.
        let b = board(&[("To do", &["fix login"])]);
        let v = View {
            focus: Focus { col: 0, row: 0 },
            editing: Some(("fix login", 9)),
            empty_label: "(empty)",
            no_columns_label: "no columns yet",
            palette: pal(),
        };
        let f = render(&b, &v, 40, 20);
        let (cx, cy) = f.cursor.expect("insert mode must place a cursor");
        let x = layout(1, 0, 40).x_of(0);
        assert_eq!(
            cx,
            x + "fix login".chars().count() as u16,
            "the caret sits one cell past the last character, not on it"
        );
        assert_eq!(cy, CARDS_TOP);
        assert_eq!(
            f.cell(cx - 1, cy).ch,
            'n',
            "the last character is behind it"
        );
        let cell = f.cell(cx, cy);
        assert_eq!(cell.ch, ' ', "and the caret cell itself is empty");
        assert_ne!(cell.fg, cell.bg, "fg == bg would be an invisible caret");
    }

    // ---- Degenerate shapes ----------------------------------------------

    #[test]
    fn a_board_with_no_columns_says_so() {
        let f = render(&Board::new(), &view(Focus::default()), 40, 20);
        assert_eq!(
            row_text(&f, 0).trim(),
            "no columns yet",
            "on the first row, like every other view"
        );
    }

    #[test]
    fn every_frame_is_exactly_the_size_the_app_asked_for() {
        let b = board(&[("A", &["x"]), ("B", &[]), ("C", &["y", "z"])]);
        for (cols, rows) in [(20u16, 6u16), (80, 24), (200, 60), (1, 1)] {
            let f = render(&b, &view(Focus { col: 0, row: 0 }), cols, rows);
            assert_eq!(f.cols, cols);
            assert_eq!(f.rows, rows);
            assert_eq!(f.cells.len(), cols as usize * rows as usize);
        }
    }
}
