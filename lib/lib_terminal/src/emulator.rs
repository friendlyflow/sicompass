//! Minimal terminal emulator backed by `vte::Parser`.
//!
//! Reads ANSI/VT bytes coming off a PTY and maintains a 2D cell grid that
//! can be snapshotted into a `DashboardFrame`. Supports enough of the xterm
//! repertoire to drive `vim`, `less`, `htop`, `cargo build`, and similar:
//!
//! * Printable text + UTF-8.
//! * `\r`, `\n`, `\b`, `\t` C0 controls.
//! * CSI cursor moves (CUP, CUU/CUD/CUF/CUB), erase (EL, ED), DECSTBM
//!   (scroll region), DECSC/DECRC (save/restore cursor).
//! * SGR: reset, bold, underline, reverse, 8/16-color fg+bg, 256-color and
//!   24-bit truecolor extensions.
//! * DEC private modes: `?1049` (alternate screen), `?25` (cursor visibility),
//!   `?7` (autowrap), `?2004` (bracketed paste — tracked so the provider can
//!   wrap pasted text correctly).
//!
//! Out of scope (Phase 2b): mouse reporting, wide-char widths, GR character
//! sets, sixel/SGR mouse, OSC titles, scrollback. The emulator's grid is
//! always exactly `cols × rows`; nothing scrolls off into a backbuffer.

use sicompass_sdk::{CellAttrs, DashboardCell, DashboardFrame, DashboardKey, DashboardKeysym};
use vte::{Params, Parser, Perform};

const DEFAULT_FG: u32 = 0xE0E0E0FF;
const DEFAULT_BG: u32 = 0x101820FF;

// ---------------------------------------------------------------------------
// Public façade
// ---------------------------------------------------------------------------

/// A live terminal grid driven by ANSI bytes.
pub struct Emulator {
    parser: Parser,
    state: EmulatorState,
}

impl Emulator {
    pub fn new(cols: u16, rows: u16) -> Self {
        let cols = cols.max(1);
        let rows = rows.max(1);
        Emulator {
            parser: Parser::new(),
            state: EmulatorState::new(cols, rows),
        }
    }

    /// Feed bytes from the PTY. Updates the grid in place.
    pub fn feed(&mut self, bytes: &[u8]) {
        self.parser.advance(&mut self.state, bytes);
    }

    /// Resize the grid. Cells that fall outside the new size are dropped;
    /// new cells are blanked. The cursor is clamped.
    pub fn resize(&mut self, cols: u16, rows: u16) {
        self.state.resize(cols.max(1), rows.max(1));
    }

    /// Capture the current grid + cursor into a `DashboardFrame`.
    pub fn snapshot(&self) -> DashboardFrame {
        self.state.snapshot()
    }

    /// Whether the child program has enabled bracketed-paste mode (`?2004h`).
    /// The provider consults this to decide whether to bracket pasted text.
    pub fn bracketed_paste(&self) -> bool {
        self.state.bracketed_paste
    }

    /// The primary-screen grid as text rows — trailing spaces and trailing
    /// blank rows trimmed. Empty when the primary screen is blank. Used to
    /// flush a finished main-screen session into the terminal scrollback.
    pub fn primary_text(&self) -> Vec<String> {
        self.state.primary_text()
    }

    /// Blank the primary screen and home its cursor. Called at the start of a
    /// dashboard session so a later `primary_text()` flush captures only that
    /// session's output, not a previous program's.
    pub fn reset_primary(&mut self) {
        self.state.reset_primary();
    }

    #[cfg(test)]
    pub fn cols(&self) -> u16 {
        self.state.cols
    }

    #[cfg(test)]
    pub fn rows(&self) -> u16 {
        self.state.rows
    }
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

#[derive(Clone)]
struct Screen {
    cells: Vec<DashboardCell>,
    cursor_col: u16,
    cursor_row: u16,
    saved_cursor: Option<(u16, u16)>,
}

impl Screen {
    fn new(cols: u16, rows: u16) -> Self {
        let blank = blank_cell();
        Screen {
            cells: vec![blank; (cols as usize) * (rows as usize)],
            cursor_col: 0,
            cursor_row: 0,
            saved_cursor: None,
        }
    }
}

struct EmulatorState {
    cols: u16,
    rows: u16,
    primary: Screen,
    alt: Screen,
    on_alt: bool,
    fg: u32,
    bg: u32,
    attrs: CellAttrs,
    cursor_visible: bool,
    autowrap: bool,
    pending_wrap: bool,
    /// Whether the child program has enabled bracketed-paste mode (`?2004`).
    /// Not used by the grid itself — read by the provider so it knows whether
    /// to wrap pasted text in `ESC[200~`/`ESC[201~`.
    bracketed_paste: bool,
    scroll_top: u16,    // inclusive, 0-indexed
    scroll_bot: u16,    // inclusive, 0-indexed
}

impl EmulatorState {
    fn new(cols: u16, rows: u16) -> Self {
        EmulatorState {
            cols,
            rows,
            primary: Screen::new(cols, rows),
            alt: Screen::new(cols, rows),
            on_alt: false,
            fg: DEFAULT_FG,
            bg: DEFAULT_BG,
            attrs: CellAttrs::default(),
            cursor_visible: true,
            autowrap: true,
            pending_wrap: false,
            bracketed_paste: false,
            scroll_top: 0,
            scroll_bot: rows.saturating_sub(1),
        }
    }

    fn screen(&mut self) -> &mut Screen {
        if self.on_alt { &mut self.alt } else { &mut self.primary }
    }

    fn screen_ref(&self) -> &Screen {
        if self.on_alt { &self.alt } else { &self.primary }
    }

    fn idx(&self, col: u16, row: u16) -> usize {
        (row as usize) * (self.cols as usize) + (col as usize)
    }

    fn current_cell(&self) -> DashboardCell {
        DashboardCell { ch: ' ', fg: self.fg, bg: self.bg, attrs: self.attrs }
    }

    fn put_char(&mut self, ch: char) {
        // Defer wrap: writing into the rightmost column leaves the cursor
        // there with `pending_wrap` set, mirroring xterm's behaviour. The
        // *next* printable character then wraps to the next line first.
        if self.pending_wrap && self.autowrap {
            self.cursor_carriage_return();
            self.linefeed();
            self.pending_wrap = false;
        }

        let cell = DashboardCell { ch, fg: self.fg, bg: self.bg, attrs: self.attrs };
        let cols = self.cols;
        let rows = self.rows;
        let col = self.screen_ref().cursor_col;
        let row = self.screen_ref().cursor_row;
        if col < cols && row < rows {
            let i = self.idx(col, row);
            self.screen().cells[i] = cell;
        }

        if col + 1 >= cols {
            self.pending_wrap = self.autowrap;
            // Cursor stays at last column.
        } else {
            self.screen().cursor_col = col + 1;
            self.pending_wrap = false;
        }
    }

    fn cursor_carriage_return(&mut self) {
        self.screen().cursor_col = 0;
        self.pending_wrap = false;
    }

    /// Move cursor down one row, scrolling within `[scroll_top, scroll_bot]`
    /// when the bottom is reached.
    fn linefeed(&mut self) {
        let bot = self.scroll_bot;
        let row = self.screen_ref().cursor_row;
        if row >= bot {
            self.scroll_up(1);
        } else {
            self.screen().cursor_row = row + 1;
        }
        self.pending_wrap = false;
    }

    fn reverse_index(&mut self) {
        let top = self.scroll_top;
        let row = self.screen_ref().cursor_row;
        if row <= top {
            self.scroll_down(1);
        } else {
            self.screen().cursor_row = row - 1;
        }
        self.pending_wrap = false;
    }

    fn backspace(&mut self) {
        let col = self.screen_ref().cursor_col;
        if col > 0 {
            self.screen().cursor_col = col - 1;
        }
        self.pending_wrap = false;
    }

    fn tab(&mut self) {
        let col = self.screen_ref().cursor_col;
        let next = ((col / 8) + 1) * 8;
        let target = next.min(self.cols.saturating_sub(1));
        self.screen().cursor_col = target;
        self.pending_wrap = false;
    }

    fn move_cursor_to(&mut self, col: u16, row: u16) {
        let cols = self.cols;
        let rows = self.rows;
        let s = self.screen();
        s.cursor_col = col.min(cols.saturating_sub(1));
        s.cursor_row = row.min(rows.saturating_sub(1));
        self.pending_wrap = false;
    }

    fn move_cursor_rel(&mut self, dcol: i32, drow: i32) {
        let cols = self.cols as i32;
        let rows = self.rows as i32;
        let s = self.screen_ref();
        let ncol = (s.cursor_col as i32 + dcol).clamp(0, cols.saturating_sub(1));
        let nrow = (s.cursor_row as i32 + drow).clamp(0, rows.saturating_sub(1));
        let s = self.screen();
        s.cursor_col = ncol as u16;
        s.cursor_row = nrow as u16;
        self.pending_wrap = false;
    }

    fn scroll_up(&mut self, n: u16) {
        let top = self.scroll_top as usize;
        let bot = self.scroll_bot as usize;
        let cols = self.cols as usize;
        let blank = self.current_cell();
        let s = self.screen();
        for _ in 0..n {
            if top >= bot { break; }
            // Move row top+1 → top, top+2 → top+1, ..., bot → bot-1.
            for r in top..bot {
                let src_start = (r + 1) * cols;
                let dst_start = r * cols;
                for c in 0..cols {
                    s.cells[dst_start + c] = s.cells[src_start + c].clone();
                }
            }
            // Blank the bottom row of the scroll region.
            let bot_start = bot * cols;
            for c in 0..cols {
                s.cells[bot_start + c] = blank.clone();
            }
        }
    }

    fn scroll_down(&mut self, n: u16) {
        let top = self.scroll_top as usize;
        let bot = self.scroll_bot as usize;
        let cols = self.cols as usize;
        let blank = self.current_cell();
        let s = self.screen();
        for _ in 0..n {
            if top >= bot { break; }
            // Move row bot-1 → bot, bot-2 → bot-1, ..., top → top+1.
            for r in (top..bot).rev() {
                let src_start = r * cols;
                let dst_start = (r + 1) * cols;
                for c in 0..cols {
                    s.cells[dst_start + c] = s.cells[src_start + c].clone();
                }
            }
            // Blank the top row of the scroll region.
            let top_start = top * cols;
            for c in 0..cols {
                s.cells[top_start + c] = blank.clone();
            }
        }
    }

    fn erase_in_line(&mut self, mode: u16) {
        let cols = self.cols;
        let row = self.screen_ref().cursor_row;
        let col = self.screen_ref().cursor_col;
        let blank = self.current_cell();
        let cols_usize = cols as usize;
        let row_start = (row as usize) * cols_usize;
        let s = self.screen();
        match mode {
            0 => {
                for c in (col as usize)..cols_usize {
                    s.cells[row_start + c] = blank.clone();
                }
            }
            1 => {
                for c in 0..=(col as usize).min(cols_usize.saturating_sub(1)) {
                    s.cells[row_start + c] = blank.clone();
                }
            }
            2 => {
                for c in 0..cols_usize {
                    s.cells[row_start + c] = blank.clone();
                }
            }
            _ => {}
        }
    }

    fn erase_in_display(&mut self, mode: u16) {
        let cols = self.cols as usize;
        let rows = self.rows as usize;
        let row = self.screen_ref().cursor_row as usize;
        let col = self.screen_ref().cursor_col as usize;
        let blank = self.current_cell();
        let s = self.screen();
        match mode {
            0 => {
                let row_start = row * cols;
                for c in col..cols {
                    s.cells[row_start + c] = blank.clone();
                }
                for r in (row + 1)..rows {
                    for c in 0..cols {
                        s.cells[r * cols + c] = blank.clone();
                    }
                }
            }
            1 => {
                for r in 0..row {
                    for c in 0..cols {
                        s.cells[r * cols + c] = blank.clone();
                    }
                }
                let row_start = row * cols;
                for c in 0..=col.min(cols.saturating_sub(1)) {
                    s.cells[row_start + c] = blank.clone();
                }
            }
            2 | 3 => {
                for cell in s.cells.iter_mut() {
                    *cell = blank.clone();
                }
            }
            _ => {}
        }
    }

    fn save_cursor(&mut self) {
        let s = self.screen();
        s.saved_cursor = Some((s.cursor_col, s.cursor_row));
    }

    fn restore_cursor(&mut self) {
        let saved = self.screen().saved_cursor;
        if let Some((c, r)) = saved {
            self.move_cursor_to(c, r);
        }
    }

    fn switch_screen(&mut self, alt: bool) {
        if self.on_alt == alt { return; }
        self.on_alt = alt;
        self.pending_wrap = false;
        // When entering alt screen, blank it. Leaving doesn't blank; the
        // primary buffer retains whatever was there.
        if alt {
            let blank = self.current_cell();
            for cell in self.alt.cells.iter_mut() {
                *cell = blank.clone();
            }
            self.alt.cursor_col = 0;
            self.alt.cursor_row = 0;
            self.alt.saved_cursor = None;
        }
    }

    fn reset_attrs(&mut self) {
        self.fg = DEFAULT_FG;
        self.bg = DEFAULT_BG;
        self.attrs = CellAttrs::default();
    }

    fn apply_sgr(&mut self, params: &Params) {
        // Flatten so we can step through colon- and semicolon-separated
        // 256/truecolor extensions uniformly.
        let flat: Vec<u16> = params.iter().flat_map(|p| p.iter().copied()).collect();
        if flat.is_empty() {
            self.reset_attrs();
            return;
        }
        let mut i = 0;
        while i < flat.len() {
            let p = flat[i];
            match p {
                0 => self.reset_attrs(),
                1 => self.attrs.bold = true,
                4 => self.attrs.underline = true,
                7 => self.attrs.reverse = true,
                22 => self.attrs.bold = false,
                24 => self.attrs.underline = false,
                27 => self.attrs.reverse = false,
                30..=37 => self.fg = palette_color((p - 30) as u8),
                90..=97 => self.fg = palette_color((p - 90 + 8) as u8),
                40..=47 => self.bg = palette_color((p - 40) as u8),
                100..=107 => self.bg = palette_color((p - 100 + 8) as u8),
                39 => self.fg = DEFAULT_FG,
                49 => self.bg = DEFAULT_BG,
                38 | 48 => {
                    let is_fg = p == 38;
                    if i + 1 >= flat.len() { break; }
                    match flat[i + 1] {
                        5 => {
                            if i + 2 >= flat.len() { break; }
                            let c = palette_256(flat[i + 2] as u8);
                            if is_fg { self.fg = c; } else { self.bg = c; }
                            i += 2;
                        }
                        2 => {
                            if i + 4 >= flat.len() { break; }
                            let r = flat[i + 2] as u32;
                            let g = flat[i + 3] as u32;
                            let b = flat[i + 4] as u32;
                            let c = (r << 24) | (g << 16) | (b << 8) | 0xFF;
                            if is_fg { self.fg = c; } else { self.bg = c; }
                            i += 4;
                        }
                        _ => {}
                    }
                }
                _ => {}
            }
            i += 1;
        }
    }

    fn resize(&mut self, cols: u16, rows: u16) {
        self.cols = cols;
        self.rows = rows;
        self.scroll_top = 0;
        self.scroll_bot = rows.saturating_sub(1);
        let blank = self.current_cell();
        self.primary.cells.resize((cols as usize) * (rows as usize), blank.clone());
        self.alt.cells.resize((cols as usize) * (rows as usize), blank);
        self.primary.cursor_col = self.primary.cursor_col.min(cols.saturating_sub(1));
        self.primary.cursor_row = self.primary.cursor_row.min(rows.saturating_sub(1));
        self.alt.cursor_col = self.alt.cursor_col.min(cols.saturating_sub(1));
        self.alt.cursor_row = self.alt.cursor_row.min(rows.saturating_sub(1));
        self.pending_wrap = false;
    }

    fn primary_text(&self) -> Vec<String> {
        let cols = self.cols as usize;
        let mut rows: Vec<String> = Vec::with_capacity(self.rows as usize);
        for r in 0..self.rows as usize {
            let mut line = String::with_capacity(cols);
            for c in 0..cols {
                line.push(self.primary.cells[r * cols + c].ch);
            }
            rows.push(line.trim_end().to_owned());
        }
        while rows.last().map(|s| s.is_empty()).unwrap_or(false) {
            rows.pop();
        }
        rows
    }

    fn reset_primary(&mut self) {
        let blank = blank_cell();
        for cell in self.primary.cells.iter_mut() {
            *cell = blank.clone();
        }
        self.primary.cursor_col = 0;
        self.primary.cursor_row = 0;
        self.primary.saved_cursor = None;
    }

    fn snapshot(&self) -> DashboardFrame {
        let s = self.screen_ref();
        let mut frame = DashboardFrame {
            cols: self.cols,
            rows: self.rows,
            cells: s.cells.clone(),
            cursor: None,
        };
        if self.cursor_visible
            && s.cursor_col < self.cols
            && s.cursor_row < self.rows
        {
            frame.cursor = Some((s.cursor_col, s.cursor_row));
        }
        frame
    }
}

// ---------------------------------------------------------------------------
// vte::Perform impl
// ---------------------------------------------------------------------------

impl Perform for EmulatorState {
    fn print(&mut self, c: char) {
        self.put_char(c);
    }

    fn execute(&mut self, byte: u8) {
        match byte {
            0x08 => self.backspace(),     // BS
            0x09 => self.tab(),           // HT
            0x0A | 0x0B | 0x0C => self.linefeed(), // LF, VT, FF
            0x0D => self.cursor_carriage_return(), // CR
            0x07 => {} // BEL — silent
            _ => {}
        }
    }

    fn csi_dispatch(
        &mut self,
        params: &Params,
        intermediates: &[u8],
        _ignore: bool,
        action: char,
    ) {
        // DEC private modes use `?` as an intermediate.
        if intermediates.first() == Some(&b'?') {
            match action {
                'h' | 'l' => {
                    let on = action == 'h';
                    for p in params.iter() {
                        match p.first().copied().unwrap_or(0) {
                            7  => self.autowrap = on,
                            25 => self.cursor_visible = on,
                            1049 | 47 | 1047 => self.switch_screen(on),
                            2004 => self.bracketed_paste = on,
                            _ => {}
                        }
                    }
                }
                _ => {}
            }
            return;
        }

        // Plain CSI (no intermediates).
        if !intermediates.is_empty() {
            return;
        }

        let first_param = params.iter().next().and_then(|p| p.first().copied());
        let n = first_param.unwrap_or(0).max(1);

        match action {
            'A' => self.move_cursor_rel(0, -(n as i32)),
            'B' | 'e' => self.move_cursor_rel(0, n as i32),
            'C' | 'a' => self.move_cursor_rel(n as i32, 0),
            'D' => self.move_cursor_rel(-(n as i32), 0),
            'E' => {
                self.move_cursor_rel(0, n as i32);
                self.cursor_carriage_return();
            }
            'F' => {
                self.move_cursor_rel(0, -(n as i32));
                self.cursor_carriage_return();
            }
            'G' | '`' => {
                let col = first_param.unwrap_or(1).max(1) - 1;
                let row = self.screen_ref().cursor_row;
                self.move_cursor_to(col, row);
            }
            'H' | 'f' => {
                let mut it = params.iter();
                let row = it.next().and_then(|p| p.first().copied()).unwrap_or(1).max(1) - 1;
                let col = it.next().and_then(|p| p.first().copied()).unwrap_or(1).max(1) - 1;
                self.move_cursor_to(col, row);
            }
            'd' => {
                let row = first_param.unwrap_or(1).max(1) - 1;
                let col = self.screen_ref().cursor_col;
                self.move_cursor_to(col, row);
            }
            'J' => self.erase_in_display(first_param.unwrap_or(0)),
            'K' => self.erase_in_line(first_param.unwrap_or(0)),
            'm' => self.apply_sgr(params),
            'r' => {
                let mut it = params.iter();
                let top = it.next().and_then(|p| p.first().copied()).unwrap_or(1).max(1) - 1;
                let bot = it.next().and_then(|p| p.first().copied()).unwrap_or(self.rows as u16);
                let bot = bot.max(1) - 1;
                self.scroll_top = top.min(self.rows.saturating_sub(1));
                self.scroll_bot = bot.min(self.rows.saturating_sub(1));
                if self.scroll_top > self.scroll_bot {
                    self.scroll_top = 0;
                    self.scroll_bot = self.rows.saturating_sub(1);
                }
                self.move_cursor_to(0, 0);
            }
            's' => self.save_cursor(),
            'u' => self.restore_cursor(),
            'S' => self.scroll_up(n),
            'T' => self.scroll_down(n),
            _ => {}
        }
    }

    fn esc_dispatch(&mut self, _intermediates: &[u8], _ignore: bool, byte: u8) {
        match byte {
            b'7' => self.save_cursor(),
            b'8' => self.restore_cursor(),
            b'D' => self.linefeed(),
            b'M' => self.reverse_index(),
            b'E' => {
                self.linefeed();
                self.cursor_carriage_return();
            }
            b'c' => {
                // Full reset
                self.fg = DEFAULT_FG;
                self.bg = DEFAULT_BG;
                self.attrs = CellAttrs::default();
                self.scroll_top = 0;
                self.scroll_bot = self.rows.saturating_sub(1);
                let blank = self.current_cell();
                for cell in self.primary.cells.iter_mut() { *cell = blank.clone(); }
                for cell in self.alt.cells.iter_mut() { *cell = blank.clone(); }
                self.primary.cursor_col = 0;
                self.primary.cursor_row = 0;
                self.alt.cursor_col = 0;
                self.alt.cursor_row = 0;
                self.on_alt = false;
                self.cursor_visible = true;
                self.autowrap = true;
                self.pending_wrap = false;
            }
            _ => {}
        }
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn blank_cell() -> DashboardCell {
    DashboardCell {
        ch: ' ',
        fg: DEFAULT_FG,
        bg: DEFAULT_BG,
        attrs: CellAttrs::default(),
    }
}

/// Standard xterm 16-color palette → 0xRRGGBBAA.
fn palette_color(idx: u8) -> u32 {
    const PALETTE: [u32; 16] = [
        0x000000FF, 0xCD0000FF, 0x00CD00FF, 0xCDCD00FF,
        0x0000EEFF, 0xCD00CDFF, 0x00CDCDFF, 0xE5E5E5FF,
        0x808080FF, 0xFF0000FF, 0x00FF00FF, 0xFFFF00FF,
        0x5C5CFFFF, 0xFF00FFFF, 0x00FFFFFF, 0xFFFFFFFF,
    ];
    PALETTE[(idx & 0x0F) as usize]
}

/// xterm 256-color palette → 0xRRGGBBAA. 0..=15 are the standard 16 colors,
/// 16..=231 are a 6×6×6 RGB cube, 232..=255 are 24 grayscale levels.
fn palette_256(idx: u8) -> u32 {
    if idx < 16 {
        return palette_color(idx);
    }
    if idx >= 232 {
        let level = 8u32 + 10u32 * (idx as u32 - 232);
        return (level << 24) | (level << 16) | (level << 8) | 0xFF;
    }
    const STEP: [u32; 6] = [0, 95, 135, 175, 215, 255];
    let i = idx as u32 - 16;
    let r = STEP[((i / 36) % 6) as usize];
    let g = STEP[((i / 6) % 6) as usize];
    let b = STEP[(i % 6) as usize];
    (r << 24) | (g << 16) | (b << 8) | 0xFF
}

// ---------------------------------------------------------------------------
// Key encoding (DashboardKey → PTY bytes)
// ---------------------------------------------------------------------------

/// Translate a [`DashboardKey`] into the byte sequence a Unix terminal would
/// transmit for that keypress. Returns `None` for keysyms that produce no
/// output (e.g. `Unknown`, or unmodified printable chars that arrive
/// separately via `dashboard_text`).
pub fn encode_dashboard_key(key: &DashboardKey) -> Option<Vec<u8>> {
    match key.keysym {
        DashboardKeysym::Enter => Some(b"\r".to_vec()),
        DashboardKeysym::Backspace => Some(b"\x7F".to_vec()),
        DashboardKeysym::Tab => Some(b"\t".to_vec()),
        DashboardKeysym::Escape => Some(b"\x1b".to_vec()),
        DashboardKeysym::Up => Some(b"\x1b[A".to_vec()),
        DashboardKeysym::Down => Some(b"\x1b[B".to_vec()),
        DashboardKeysym::Right => Some(b"\x1b[C".to_vec()),
        DashboardKeysym::Left => Some(b"\x1b[D".to_vec()),
        DashboardKeysym::Home => Some(b"\x1b[H".to_vec()),
        DashboardKeysym::End => Some(b"\x1b[F".to_vec()),
        DashboardKeysym::PageUp => Some(b"\x1b[5~".to_vec()),
        DashboardKeysym::PageDown => Some(b"\x1b[6~".to_vec()),
        DashboardKeysym::Insert => Some(b"\x1b[2~".to_vec()),
        DashboardKeysym::Delete => Some(b"\x1b[3~".to_vec()),
        DashboardKeysym::F(n) => Some(encode_function_key(n)),
        DashboardKeysym::Char(c) => {
            if key.ctrl {
                // Ctrl+letter → 0x01..=0x1A (standard caret notation).
                let lower = c.to_ascii_lowercase();
                if lower.is_ascii_alphabetic() {
                    let b = (lower as u8) - b'a' + 1;
                    return Some(if key.alt { vec![0x1B, b] } else { vec![b] });
                }
                // Ctrl+space / Ctrl+@ → NUL.
                if c == ' ' || c == '@' {
                    return Some(if key.alt { vec![0x1B, 0x00] } else { vec![0x00] });
                }
                // Ctrl+[ → ESC. Ctrl+] → GS (0x1D). Ctrl+\ → FS (0x1C).
                let b = match c {
                    '[' => Some(0x1B),
                    '\\' => Some(0x1C),
                    ']' => Some(0x1D),
                    '^' => Some(0x1E),
                    '_' => Some(0x1F),
                    '?' => Some(0x7F),
                    _ => None,
                }?;
                return Some(if key.alt { vec![0x1B, b] } else { vec![b] });
            }
            if key.alt {
                // Alt+letter → ESC <letter>.
                let mut buf = [0u8; 4];
                let s = c.encode_utf8(&mut buf).as_bytes().to_vec();
                let mut out = Vec::with_capacity(s.len() + 1);
                out.push(0x1B);
                out.extend(s);
                return Some(out);
            }
            // Plain printable char arrives via `dashboard_text`; skip here.
            None
        }
        DashboardKeysym::Unknown => None,
    }
}

fn encode_function_key(n: u8) -> Vec<u8> {
    match n {
        1 => b"\x1bOP".to_vec(),
        2 => b"\x1bOQ".to_vec(),
        3 => b"\x1bOR".to_vec(),
        4 => b"\x1bOS".to_vec(),
        5 => b"\x1b[15~".to_vec(),
        6 => b"\x1b[17~".to_vec(),
        7 => b"\x1b[18~".to_vec(),
        8 => b"\x1b[19~".to_vec(),
        9 => b"\x1b[20~".to_vec(),
        10 => b"\x1b[21~".to_vec(),
        11 => b"\x1b[23~".to_vec(),
        12 => b"\x1b[24~".to_vec(),
        _ => Vec::new(),
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn cell_ch(em: &Emulator, col: u16, row: u16) -> char {
        let s = em.snapshot();
        s.cell(col, row).ch
    }

    #[test]
    fn bracketed_paste_mode_tracks_2004() {
        let mut em = Emulator::new(10, 2);
        assert!(!em.bracketed_paste());
        em.feed(b"\x1b[?2004h");
        assert!(em.bracketed_paste());
        em.feed(b"\x1b[?2004l");
        assert!(!em.bracketed_paste());
    }

    #[test]
    fn primary_text_reads_grid_and_trims_blanks() {
        let mut em = Emulator::new(20, 5);
        em.feed(b"abc\r\ndef");
        // Trailing spaces per row and trailing blank rows are trimmed.
        assert_eq!(em.primary_text(), vec!["abc".to_owned(), "def".to_owned()]);
    }

    #[test]
    fn reset_primary_blanks_the_screen() {
        let mut em = Emulator::new(10, 3);
        em.feed(b"stuff");
        assert!(!em.primary_text().is_empty());
        em.reset_primary();
        assert!(em.primary_text().is_empty());
        assert_eq!(em.snapshot().cursor, Some((0, 0)));
    }

    #[test]
    fn primary_text_ignores_alt_screen_content() {
        // Content drawn on the alternate screen must not leak into the flush.
        let mut em = Emulator::new(20, 4);
        em.feed(b"\x1b[?1049h");
        em.feed(b"alt-only content");
        em.feed(b"\x1b[?1049l");
        assert!(em.primary_text().is_empty());
    }

    #[test]
    fn print_advances_cursor_left_to_right() {
        let mut em = Emulator::new(10, 2);
        em.feed(b"abc");
        assert_eq!(cell_ch(&em, 0, 0), 'a');
        assert_eq!(cell_ch(&em, 1, 0), 'b');
        assert_eq!(cell_ch(&em, 2, 0), 'c');
        assert_eq!(em.snapshot().cursor, Some((3, 0)));
    }

    #[test]
    fn cr_lf_advances_to_next_line() {
        let mut em = Emulator::new(10, 3);
        em.feed(b"hi\r\nyo");
        assert_eq!(cell_ch(&em, 0, 0), 'h');
        assert_eq!(cell_ch(&em, 1, 0), 'i');
        assert_eq!(cell_ch(&em, 0, 1), 'y');
        assert_eq!(cell_ch(&em, 1, 1), 'o');
    }

    #[test]
    fn backspace_moves_cursor_left() {
        let mut em = Emulator::new(10, 2);
        em.feed(b"ab\x08c");
        assert_eq!(cell_ch(&em, 0, 0), 'a');
        // BS does not erase; the next print overwrites column 1.
        assert_eq!(cell_ch(&em, 1, 0), 'c');
    }

    #[test]
    fn csi_cup_positions_cursor_one_indexed() {
        let mut em = Emulator::new(10, 5);
        em.feed(b"\x1b[3;4Hx");
        // CSI 3;4H → row 3, col 4 (1-based) → row 2, col 3 (0-based)
        assert_eq!(cell_ch(&em, 3, 2), 'x');
    }

    #[test]
    fn csi_cup_no_args_goes_home() {
        let mut em = Emulator::new(10, 5);
        em.feed(b"\x1b[3;4H");
        em.feed(b"\x1b[Hy");
        assert_eq!(cell_ch(&em, 0, 0), 'y');
    }

    #[test]
    fn csi_cuu_cud_cuf_cub_move_cursor() {
        let mut em = Emulator::new(10, 5);
        em.feed(b"\x1b[3;3H"); // row 3, col 3 (1-based) → (2, 2)
        em.feed(b"\x1b[A");    // up 1 → (2, 1)
        em.feed(b"u");
        em.feed(b"\x1b[2;2H"); // (1, 1)
        em.feed(b"\x1b[2C");   // right 2 → (1, 3)
        em.feed(b"r");
        assert_eq!(cell_ch(&em, 2, 1), 'u');
        assert_eq!(cell_ch(&em, 3, 1), 'r');
    }

    #[test]
    fn csi_el_clears_to_end_of_line() {
        let mut em = Emulator::new(10, 2);
        em.feed(b"abcdef");
        em.feed(b"\x1b[3G"); // CHA → col 3 (1-based) → 2
        em.feed(b"\x1b[K");  // erase from cursor to end
        assert_eq!(cell_ch(&em, 0, 0), 'a');
        assert_eq!(cell_ch(&em, 1, 0), 'b');
        assert_eq!(cell_ch(&em, 2, 0), ' ');
        assert_eq!(cell_ch(&em, 5, 0), ' ');
    }

    #[test]
    fn csi_ed_2_clears_whole_screen() {
        let mut em = Emulator::new(5, 3);
        em.feed(b"hello\r\nworld");
        em.feed(b"\x1b[2J");
        for r in 0..3 { for c in 0..5 { assert_eq!(cell_ch(&em, c, r), ' '); } }
    }

    #[test]
    fn sgr_color_applies_to_following_chars() {
        let mut em = Emulator::new(5, 1);
        em.feed(b"\x1b[31mR\x1b[0mn");
        let f = em.snapshot();
        assert_eq!(f.cell(0, 0).fg, palette_color(1)); // red
        assert_eq!(f.cell(1, 0).fg, DEFAULT_FG);       // reset
    }

    #[test]
    fn sgr_truecolor_24bit() {
        let mut em = Emulator::new(2, 1);
        em.feed(b"\x1b[38;2;10;20;30mZ");
        let c = em.snapshot().cell(0, 0).fg;
        assert_eq!(c, (10u32 << 24) | (20u32 << 16) | (30u32 << 8) | 0xFF);
    }

    #[test]
    fn sgr_256_palette_index() {
        let mut em = Emulator::new(2, 1);
        em.feed(b"\x1b[38;5;196mX");
        assert_eq!(em.snapshot().cell(0, 0).fg, palette_256(196));
    }

    #[test]
    fn sgr_attrs_bold_underline_reverse() {
        let mut em = Emulator::new(4, 1);
        em.feed(b"\x1b[1;4;7mz");
        let frame = em.snapshot();
        let c = frame.cell(0, 0);
        assert!(c.attrs.bold);
        assert!(c.attrs.underline);
        assert!(c.attrs.reverse);
    }

    #[test]
    fn dec_private_alt_screen_preserves_primary() {
        let mut em = Emulator::new(4, 2);
        em.feed(b"primary");
        em.feed(b"\x1b[?1049h");
        em.feed(b"\x1b[Halt");
        // We're on alt screen now; primary is still "prim" (4 cols, wrap).
        assert_eq!(cell_ch(&em, 0, 0), 'a');
        em.feed(b"\x1b[?1049l"); // back to primary
        assert_eq!(cell_ch(&em, 0, 0), 'p');
        assert_eq!(cell_ch(&em, 1, 0), 'r');
    }

    #[test]
    fn dec_private_cursor_visibility() {
        let mut em = Emulator::new(4, 1);
        em.feed(b"\x1b[?25l");
        assert!(em.snapshot().cursor.is_none());
        em.feed(b"\x1b[?25h");
        assert!(em.snapshot().cursor.is_some());
    }

    #[test]
    fn linefeed_at_bottom_scrolls_up() {
        let mut em = Emulator::new(3, 2);
        em.feed(b"AB\r\nCD\r\nEF");
        assert_eq!(cell_ch(&em, 0, 0), 'C');
        assert_eq!(cell_ch(&em, 0, 1), 'E');
    }

    #[test]
    fn esc_save_restore_cursor() {
        let mut em = Emulator::new(5, 2);
        em.feed(b"\x1b[1;3H");  // (0, 2)
        em.feed(b"\x1b7");       // save
        em.feed(b"\x1b[2;1Hx"); // jump and write
        em.feed(b"\x1b8");       // restore
        em.feed(b"y");
        assert_eq!(cell_ch(&em, 0, 1), 'x');
        assert_eq!(cell_ch(&em, 2, 0), 'y');
    }

    #[test]
    fn resize_grows_and_clamps_cursor() {
        let mut em = Emulator::new(4, 2);
        em.feed(b"abcd");
        assert_eq!(em.snapshot().cursor, Some((3, 0))); // pending wrap state
        em.resize(8, 3);
        assert_eq!(em.cols(), 8);
        assert_eq!(em.rows(), 3);
        let s = em.snapshot();
        assert_eq!(s.cells.len(), 8 * 3);
        assert_eq!(s.cell(0, 0).ch, 'a');
    }

    #[test]
    fn utf8_chars_render() {
        let mut em = Emulator::new(5, 1);
        em.feed("héllo".as_bytes());
        assert_eq!(cell_ch(&em, 1, 0), 'é');
    }

    // ---- key encoding -----------------------------------------------------

    #[test]
    fn encode_special_keys() {
        assert_eq!(
            encode_dashboard_key(&DashboardKey {
                keysym: DashboardKeysym::Enter, ctrl: false, shift: false, alt: false,
            }).unwrap(),
            b"\r",
        );
        assert_eq!(
            encode_dashboard_key(&DashboardKey {
                keysym: DashboardKeysym::Up, ctrl: false, shift: false, alt: false,
            }).unwrap(),
            b"\x1b[A",
        );
        assert_eq!(
            encode_dashboard_key(&DashboardKey {
                keysym: DashboardKeysym::Backspace, ctrl: false, shift: false, alt: false,
            }).unwrap(),
            b"\x7F",
        );
        assert_eq!(
            encode_dashboard_key(&DashboardKey {
                keysym: DashboardKeysym::PageUp, ctrl: false, shift: false, alt: false,
            }).unwrap(),
            b"\x1b[5~",
        );
        assert_eq!(
            encode_dashboard_key(&DashboardKey {
                keysym: DashboardKeysym::F(1), ctrl: false, shift: false, alt: false,
            }).unwrap(),
            b"\x1bOP",
        );
        assert_eq!(
            encode_dashboard_key(&DashboardKey {
                keysym: DashboardKeysym::F(5), ctrl: false, shift: false, alt: false,
            }).unwrap(),
            b"\x1b[15~",
        );
    }

    #[test]
    fn encode_escape_keysym_emits_esc_byte() {
        let bytes = encode_dashboard_key(&DashboardKey {
            keysym: DashboardKeysym::Escape, ctrl: false, shift: false, alt: false,
        }).unwrap();
        assert_eq!(bytes, b"\x1b");
    }

    #[test]
    fn encode_ctrl_letter_to_control_byte() {
        let bytes = encode_dashboard_key(&DashboardKey {
            keysym: DashboardKeysym::Char('c'), ctrl: true, shift: false, alt: false,
        }).unwrap();
        assert_eq!(bytes, vec![0x03]); // SIGINT
        let bytes = encode_dashboard_key(&DashboardKey {
            keysym: DashboardKeysym::Char('d'), ctrl: true, shift: false, alt: false,
        }).unwrap();
        assert_eq!(bytes, vec![0x04]); // EOF
    }

    #[test]
    fn encode_alt_letter_prefixes_esc() {
        let bytes = encode_dashboard_key(&DashboardKey {
            keysym: DashboardKeysym::Char('b'), ctrl: false, shift: false, alt: true,
        }).unwrap();
        assert_eq!(bytes, vec![0x1B, b'b']);
    }

    #[test]
    fn encode_unmodified_char_is_none() {
        // Plain printable letters arrive through `dashboard_text`; the keysym
        // path should produce no bytes.
        assert!(encode_dashboard_key(&DashboardKey {
            keysym: DashboardKeysym::Char('a'), ctrl: false, shift: false, alt: false,
        }).is_none());
    }
}
