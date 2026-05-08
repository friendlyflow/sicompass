//! Types describing the interactive-dashboard surface a provider can opt into.
//!
//! The legacy fullscreen mode (`Provider::dashboard_image_path`) renders a
//! static image and is unaffected by anything in this module. Providers that
//! return [`DashboardKind::Interactive`] from `dashboard_kind()` instead
//! receive raw input events and supply a [`DashboardFrame`] every frame.
//!
//! All graphics are described in *cells* — the app turns each cell into a
//! background rectangle plus an optional glyph using its existing font and
//! rectangle pipelines. The SDK never references the windowing or graphics
//! crates directly.

// ---------------------------------------------------------------------------
// Opt-in
// ---------------------------------------------------------------------------

/// What kind of fullscreen view, if any, a provider supports.
///
/// The default is `None`: pressing the `d` key while this provider is active
/// is a no-op. The two opt-in variants are mutually exclusive and route the
/// app down completely separate render and input paths.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DashboardKind {
    /// No fullscreen view.
    #[default]
    None,
    /// Static image — provider supplies a path via `dashboard_image_path()`.
    /// The app reads/scales/centers the image and only listens for `Escape`.
    Image,
    /// Cell-grid view driven by per-frame `dashboard_render(cols, rows)`.
    /// The app forwards keystrokes, text input, and resize events to the
    /// provider; `Escape` still exits back to the previous coordinate.
    Interactive,
}

// ---------------------------------------------------------------------------
// Cell grid
// ---------------------------------------------------------------------------

/// Per-cell text attributes. Reserved for future SGR support — Phase 2a only
/// honours `reverse` (swap fg/bg).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CellAttrs {
    pub bold: bool,
    pub underline: bool,
    pub reverse: bool,
}

/// One character cell in a [`DashboardFrame`].
///
/// Colors are packed `0xRRGGBBAA`, matching the rest of the app's palette
/// representation. An alpha of 0 in `bg` means "draw no background fill"
/// (the window clear colour shows through).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DashboardCell {
    pub ch: char,
    pub fg: u32,
    pub bg: u32,
    pub attrs: CellAttrs,
}

impl Default for DashboardCell {
    fn default() -> Self {
        DashboardCell {
            ch: ' ',
            fg: 0xFFFFFFFF,
            bg: 0x00000000,
            attrs: CellAttrs::default(),
        }
    }
}

/// A snapshot of the provider's terminal grid for one frame.
///
/// `cells` is row-major with length `cols * rows`. `cursor` is `(col, row)`
/// in cell coordinates (0-indexed); `None` hides the cursor.
#[derive(Debug, Clone)]
pub struct DashboardFrame {
    pub cols: u16,
    pub rows: u16,
    pub cells: Vec<DashboardCell>,
    pub cursor: Option<(u16, u16)>,
}

impl DashboardFrame {
    /// A frame filled with the default cell (space, white-on-transparent).
    pub fn empty(cols: u16, rows: u16) -> Self {
        let len = (cols as usize) * (rows as usize);
        DashboardFrame {
            cols,
            rows,
            cells: vec![DashboardCell::default(); len],
            cursor: None,
        }
    }

    fn idx(&self, col: u16, row: u16) -> usize {
        (row as usize) * (self.cols as usize) + (col as usize)
    }

    pub fn cell(&self, col: u16, row: u16) -> &DashboardCell {
        &self.cells[self.idx(col, row)]
    }

    pub fn cell_mut(&mut self, col: u16, row: u16) -> &mut DashboardCell {
        let i = self.idx(col, row);
        &mut self.cells[i]
    }

    /// Set the character + fg color of a cell, preserving its existing bg/attrs.
    pub fn set_char(&mut self, col: u16, row: u16, ch: char, fg: u32) {
        if col < self.cols && row < self.rows {
            let c = self.cell_mut(col, row);
            c.ch = ch;
            c.fg = fg;
        }
    }

    /// Write a string starting at `(col, row)`, left-to-right, clipping at
    /// the right edge. Tabs and newlines are not interpreted.
    pub fn write_str(&mut self, col: u16, row: u16, s: &str, fg: u32) {
        if row >= self.rows {
            return;
        }
        let mut c = col;
        for ch in s.chars() {
            if c >= self.cols {
                break;
            }
            self.set_char(c, row, ch, fg);
            c += 1;
        }
    }
}

// ---------------------------------------------------------------------------
// Input
// ---------------------------------------------------------------------------

/// A non-printable / modified key event forwarded while the provider is in
/// the interactive dashboard. Printable text arrives separately through
/// `Provider::dashboard_text`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DashboardKey {
    pub keysym: DashboardKeysym,
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
}

/// Keysym variants the app maps SDL keycodes onto. Kept deliberately small —
/// providers handle printable input through `dashboard_text` and reach for
/// `Char(c)` only when modifiers (e.g. Ctrl+letter) suppress the text-input
/// event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DashboardKeysym {
    Enter,
    Backspace,
    Tab,
    Up,
    Down,
    Left,
    Right,
    Home,
    End,
    PageUp,
    PageDown,
    Insert,
    Delete,
    F(u8),
    /// Letter / digit when the text-input event is suppressed (e.g. Ctrl held).
    /// Stored lowercase; modifier flags live on the surrounding `DashboardKey`.
    Char(char),
    /// Anything we don't recognise. Providers should generally ignore this.
    Unknown,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_frame_has_correct_dimensions() {
        let f = DashboardFrame::empty(80, 24);
        assert_eq!(f.cols, 80);
        assert_eq!(f.rows, 24);
        assert_eq!(f.cells.len(), 80 * 24);
        assert!(f.cursor.is_none());
    }

    #[test]
    fn write_str_clips_at_right_edge() {
        let mut f = DashboardFrame::empty(5, 1);
        f.write_str(3, 0, "hello", 0xFFFFFFFF);
        assert_eq!(f.cell(3, 0).ch, 'h');
        assert_eq!(f.cell(4, 0).ch, 'e');
    }

    #[test]
    fn write_str_no_op_past_last_row() {
        let mut f = DashboardFrame::empty(5, 1);
        f.write_str(0, 5, "x", 0xFFFFFFFF);
        assert_eq!(f.cell(0, 0).ch, ' ');
    }

    #[test]
    fn set_char_out_of_bounds_is_no_op() {
        let mut f = DashboardFrame::empty(2, 2);
        f.set_char(99, 99, 'x', 0xFFFFFFFF);
        assert_eq!(f.cell(0, 0).ch, ' ');
    }

    #[test]
    fn dashboard_kind_default_is_none() {
        assert_eq!(DashboardKind::default(), DashboardKind::None);
    }
}
