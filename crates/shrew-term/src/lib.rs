//! Terminal state for Shrew, backed by libghostty-vt.
//!
//! A Rex terminal stream starts with a Ghostty snapshot (`GHOSTSNP...`) and
//! continues with raw program output. [`Terminal::feed_stream`] frames the
//! snapshot out of the byte stream, restores it exactly, and then writes the
//! rest through Ghostty's VT parser. [`Terminal::frame`] turns the viewport into
//! style runs that any renderer (Core Text, Android Canvas, GTK/Pango) can draw.

#[allow(clippy::all, unsafe_op_in_unsafe_fn)]
mod sys;

use std::mem::{MaybeUninit, size_of};
use std::ptr;

pub use shrew_proto::snapshot::{Framed, SnapshotFramer};
use sys::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash)]
pub struct Rgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Rgb {
    pub const fn new(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b }
    }
    pub fn parse_hex(s: &str) -> Option<Self> {
        let s = s.trim().trim_start_matches('#');
        if s.len() != 6 {
            return None;
        }
        let v = u32::from_str_radix(s, 16).ok()?;
        Some(Self::new((v >> 16) as u8, (v >> 8) as u8, v as u8))
    }
    pub fn to_u32(self) -> u32 {
        0xff00_0000 | (self.r as u32) << 16 | (self.g as u32) << 8 | self.b as u32
    }
    /// Relative luminance, for picking light/dark chrome around a terminal.
    pub fn luminance(self) -> f32 {
        let f = |c: u8| {
            let c = c as f32 / 255.0;
            if c <= 0.03928 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
        };
        0.2126 * f(self.r) + 0.7152 * f(self.g) + 0.0722 * f(self.b)
    }
}

impl From<GhosttyColorRgb> for Rgb {
    fn from(c: GhosttyColorRgb) -> Self {
        Self { r: c.r, g: c.g, b: c.b }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Underline {
    #[default]
    None,
    Single,
    Double,
    Curly,
    Dotted,
    Dashed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CursorStyle {
    #[default]
    Block,
    Bar,
    Underline,
    HollowBlock,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Cursor {
    pub col: u16,
    pub row: u16,
    pub style: CursorStyle,
    pub blinking: bool,
    /// The program is reading a password (echo off); show a lock, not a caret.
    pub password: bool,
    pub wide: bool,
}

/// A run of cells on one row that share a style.
#[derive(Debug, Clone, PartialEq)]
pub struct Run {
    pub col: u16,
    /// Number of cells the run covers.
    pub cells: u16,
    pub text: String,
    /// Cell width (1 or 2) of each grapheme in `text`, in order. Renderers
    /// that place glyphs individually use this to stay on the grid.
    pub widths: Vec<u8>,
    pub fg: Rgb,
    /// None means the terminal's default background (don't paint).
    pub bg: Option<Rgb>,
    pub bold: bool,
    pub italic: bool,
    pub faint: bool,
    pub underline: Underline,
    pub underline_color: Option<Rgb>,
    pub strikethrough: bool,
    pub overline: bool,
    pub selected: bool,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Row {
    pub runs: Vec<Run>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Frame {
    pub cols: u16,
    pub rows: u16,
    pub background: Rgb,
    pub foreground: Rgb,
    pub cursor_color: Option<Rgb>,
    pub cursor: Option<Cursor>,
    pub lines: Vec<Row>,
    pub scrollbar: Scrollbar,
    pub alternate_screen: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Scrollbar {
    /// Rows in the whole buffer (scrollback + screen).
    pub total: u64,
    /// First visible row.
    pub offset: u64,
    /// Visible rows.
    pub len: u64,
}

impl Scrollbar {
    pub fn at_bottom(&self) -> bool {
        self.offset + self.len >= self.total
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StreamState {
    /// Waiting for a snapshot to start the stream.
    AwaitingSnapshot,
    /// Writing program output.
    Live,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TermError {
    Ghostty(i32),
    Snapshot(String),
}

impl std::fmt::Display for TermError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Ghostty(c) => write!(f, "libghostty-vt returned {c}"),
            Self::Snapshot(s) => write!(f, "snapshot could not be restored: {s}"),
        }
    }
}
impl std::error::Error for TermError {}

fn check(r: GhosttyResult) -> Result<(), TermError> {
    if r == GhosttyResult::GHOSTTY_SUCCESS { Ok(()) } else { Err(TermError::Ghostty(r.0)) }
}

/// Zeroed struct with its leading `size` field set, like `GHOSTTY_INIT_SIZED`.
unsafe fn sized<T>() -> T {
    let mut v = MaybeUninit::<T>::zeroed();
    unsafe {
        ptr::write(v.as_mut_ptr() as *mut usize, size_of::<T>());
        v.assume_init()
    }
}

/// One terminal emulator instance.
pub struct Terminal {
    raw: GhosttyTerminal,
    render: GhosttyRenderState,
    rows_it: GhosttyRenderStateRowIterator,
    cells: GhosttyRenderStateRowCells,
    framer: Option<SnapshotFramer>,
    scrollback_bytes: usize,
    cell_px: (u32, u32),
}

// The handles are only touched through &mut self, so moving the whole
// terminal between threads is fine.
unsafe impl Send for Terminal {}

impl Drop for Terminal {
    fn drop(&mut self) {
        unsafe {
            ghostty_render_state_row_cells_free(self.cells);
            ghostty_render_state_row_iterator_free(self.rows_it);
            ghostty_render_state_free(self.render);
            ghostty_terminal_free(self.raw);
        }
    }
}

impl Terminal {
    pub fn new(cols: u16, rows: u16) -> Self {
        unsafe {
            let mut raw = ptr::null_mut();
            check(ghostty_terminal_new(ptr::null(), &mut raw, cols.max(1), rows.max(1))).expect("ghostty_terminal_new");
            let mut render = ptr::null_mut();
            check(ghostty_render_state_new(ptr::null(), &mut render)).expect("render state");
            let mut rows_it = ptr::null_mut();
            check(ghostty_render_state_row_iterator_new(ptr::null(), &mut rows_it)).expect("row iterator");
            let mut cells = ptr::null_mut();
            check(ghostty_render_state_row_cells_new(ptr::null(), &mut cells)).expect("row cells");
            let mut t = Self { raw, render, rows_it, cells, framer: None, scrollback_bytes: 10 << 20, cell_px: (0, 0) };
            t.apply_options();
            t
        }
    }

    fn apply_options(&mut self) {
        unsafe {
            let max = self.scrollback_bytes;
            ghostty_terminal_set(
                self.raw,
                GhosttyTerminalOption::GHOSTTY_TERMINAL_OPT_SCROLLBACK_MAX_BYTES,
                &max as *const usize as *const _,
            );
        }
    }

    /// Cap scrollback memory (bytes). Phones want this lower than desktops.
    pub fn set_scrollback_limit(&mut self, bytes: usize) {
        self.scrollback_bytes = bytes;
        self.apply_options();
    }

    /// Expect a snapshot at the start of the next bytes fed (a new stream).
    pub fn begin_stream(&mut self) {
        self.framer = Some(SnapshotFramer::new());
    }

    pub fn stream_state(&self) -> StreamState {
        if self.framer.is_some() { StreamState::AwaitingSnapshot } else { StreamState::Live }
    }

    /// Feed bytes from a Rex terminal stream (after in-band messages are removed).
    pub fn feed_stream(&mut self, bytes: &[u8]) -> Result<(), TermError> {
        let Some(framer) = self.framer.as_mut() else {
            self.write_vt(bytes);
            return Ok(());
        };
        match framer.push(bytes) {
            Framed::Pending => Ok(()),
            Framed::NotSnapshot(raw) => {
                // Older/other servers may start with plain VT. Treat it as output.
                self.framer = None;
                self.write_vt(&raw);
                Ok(())
            }
            Framed::Complete { snapshot, rest } => {
                self.framer = None;
                self.restore(&snapshot)?;
                if !rest.is_empty() {
                    self.write_vt(&rest);
                }
                Ok(())
            }
            Framed::Invalid(msg) => {
                self.framer = None;
                Err(TermError::Snapshot(msg))
            }
        }
    }

    /// Replace the whole terminal state with a complete Ghostty snapshot.
    pub fn restore(&mut self, snapshot: &[u8]) -> Result<(), TermError> {
        unsafe {
            let mut dec = ptr::null_mut();
            check(ghostty_snapshot_decoder_new_buf(ptr::null(), &mut dec, snapshot.as_ptr(), snapshot.len()))?;
            let mut term = ptr::null_mut();
            let r = ghostty_snapshot_decoder_decode(dec, &mut term);
            ghostty_snapshot_decoder_free(dec);
            if r != GhosttyResult::GHOSTTY_SUCCESS {
                return Err(TermError::Snapshot(format!("decoder returned {}", r.0)));
            }
            ghostty_terminal_free(self.raw);
            self.raw = term;
        }
        self.apply_options();
        if self.cell_px != (0, 0) {
            let (c, r) = self.size();
            self.resize(c, r, self.cell_px.0, self.cell_px.1);
        }
        Ok(())
    }

    /// Write raw VT bytes.
    pub fn write_vt(&mut self, bytes: &[u8]) {
        unsafe { ghostty_terminal_vt_write(self.raw, bytes.as_ptr(), bytes.len()) }
    }

    pub fn resize(&mut self, cols: u16, rows: u16, cell_w_px: u32, cell_h_px: u32) {
        self.cell_px = (cell_w_px, cell_h_px);
        unsafe {
            ghostty_terminal_resize(self.raw, cols.max(1), rows.max(1), cell_w_px, cell_h_px);
        }
    }

    pub fn size(&self) -> (u16, u16) {
        (
            self.get::<u16>(GhosttyTerminalData::GHOSTTY_TERMINAL_DATA_COLS).unwrap_or(80),
            self.get::<u16>(GhosttyTerminalData::GHOSTTY_TERMINAL_DATA_ROWS).unwrap_or(24),
        )
    }

    fn get<T: Copy>(&self, key: GhosttyTerminalData) -> Option<T> {
        unsafe {
            let mut v = MaybeUninit::<T>::zeroed();
            (ghostty_terminal_get(self.raw, key, v.as_mut_ptr() as *mut _) == GhosttyResult::GHOSTTY_SUCCESS).then(|| v.assume_init())
        }
    }

    fn get_string(&self, key: GhosttyTerminalData) -> Option<String> {
        let s: GhosttyString = self.get(key)?;
        if s.ptr.is_null() || s.len == 0 {
            return None;
        }
        let bytes = unsafe { std::slice::from_raw_parts(s.ptr, s.len) };
        Some(String::from_utf8_lossy(bytes).into_owned())
    }

    pub fn title(&self) -> Option<String> {
        self.get_string(GhosttyTerminalData::GHOSTTY_TERMINAL_DATA_TITLE)
    }

    pub fn pwd(&self) -> Option<String> {
        self.get_string(GhosttyTerminalData::GHOSTTY_TERMINAL_DATA_PWD)
    }

    pub fn mouse_tracking(&self) -> bool {
        self.get::<bool>(GhosttyTerminalData::GHOSTTY_TERMINAL_DATA_MOUSE_TRACKING).unwrap_or(false)
    }

    pub fn alternate_screen(&self) -> bool {
        self.get::<GhosttyTerminalScreen>(GhosttyTerminalData::GHOSTTY_TERMINAL_DATA_ACTIVE_SCREEN)
            .is_some_and(|s| s == GhosttyTerminalScreen::GHOSTTY_TERMINAL_SCREEN_ALTERNATE)
    }

    pub fn scrollbar(&self) -> Scrollbar {
        self.get::<GhosttyTerminalScrollbar>(GhosttyTerminalData::GHOSTTY_TERMINAL_DATA_SCROLLBAR)
            .map(|s| Scrollbar { total: s.total, offset: s.offset, len: s.len })
            .unwrap_or_default()
    }

    /// Scroll the viewport by `delta` rows (negative is up, into history).
    pub fn scroll(&mut self, delta: isize) {
        self.scroll_raw(GhosttyTerminalScrollViewportTag::GHOSTTY_SCROLL_VIEWPORT_DELTA, GhosttyTerminalScrollViewportValue { delta });
    }

    /// Jump `delta` shell prompts (needs shell integration).
    pub fn scroll_prompts(&mut self, delta: isize) {
        self.scroll_raw(
            GhosttyTerminalScrollViewportTag::GHOSTTY_SCROLL_VIEWPORT_DELTA_PROMPT,
            GhosttyTerminalScrollViewportValue { delta_prompt: delta },
        );
    }

    pub fn scroll_to_bottom(&mut self) {
        self.scroll_raw(GhosttyTerminalScrollViewportTag::GHOSTTY_SCROLL_VIEWPORT_BOTTOM, GhosttyTerminalScrollViewportValue { row: 0 });
    }

    pub fn scroll_to_top(&mut self) {
        self.scroll_raw(GhosttyTerminalScrollViewportTag::GHOSTTY_SCROLL_VIEWPORT_TOP, GhosttyTerminalScrollViewportValue { row: 0 });
    }

    fn scroll_raw(&mut self, tag: GhosttyTerminalScrollViewportTag, value: GhosttyTerminalScrollViewportValue) {
        unsafe { ghostty_terminal_scroll_viewport(self.raw, GhosttyTerminalScrollViewport { tag, value }) }
    }

    fn viewport_ref(&self, col: u16, row: u16) -> Option<GhosttyGridRef> {
        unsafe {
            let mut r: GhosttyGridRef = sized();
            let p = GhosttyPoint {
                tag: GhosttyPointTag::GHOSTTY_POINT_TAG_VIEWPORT,
                value: GhosttyPointValue { coordinate: GhosttyPointCoordinate { x: col, y: row as u32 } },
            };
            (ghostty_terminal_grid_ref(self.raw, p, &mut r) == GhosttyResult::GHOSTTY_SUCCESS).then_some(r)
        }
    }

    /// Select from one viewport cell to another (inclusive).
    pub fn select(&mut self, from: (u16, u16), to: (u16, u16)) -> bool {
        let (Some(a), Some(b)) = (self.viewport_ref(from.0, from.1), self.viewport_ref(to.0, to.1)) else {
            return false;
        };
        unsafe {
            let mut sel: GhosttySelection = sized();
            sel.start = a;
            sel.end = b;
            ghostty_terminal_set(self.raw, GhosttyTerminalOption::GHOSTTY_TERMINAL_OPT_SELECTION, &sel as *const _ as *const _)
                == GhosttyResult::GHOSTTY_SUCCESS
        }
    }

    /// Select the word under a viewport cell (double-tap).
    pub fn select_word(&mut self, col: u16, row: u16) -> bool {
        let Some(r) = self.viewport_ref(col, row) else { return false };
        unsafe {
            let mut opts: GhosttyTerminalSelectWordOptions = sized();
            opts.ref_ = r;
            let mut sel: GhosttySelection = sized();
            if ghostty_terminal_select_word(self.raw, &opts, &mut sel) != GhosttyResult::GHOSTTY_SUCCESS {
                return false;
            }
            ghostty_terminal_set(self.raw, GhosttyTerminalOption::GHOSTTY_TERMINAL_OPT_SELECTION, &sel as *const _ as *const _)
                == GhosttyResult::GHOSTTY_SUCCESS
        }
    }

    /// Select a whole line (triple-tap).
    pub fn select_line(&mut self, row: u16) -> bool {
        let (cols, _) = self.size();
        self.select((0, row), (cols.saturating_sub(1), row))
    }

    pub fn clear_selection(&mut self) {
        unsafe {
            ghostty_terminal_set(self.raw, GhosttyTerminalOption::GHOSTTY_TERMINAL_OPT_SELECTION, ptr::null());
        }
    }

    pub fn has_selection(&self) -> bool {
        self.get::<GhosttySelection>(GhosttyTerminalData::GHOSTTY_TERMINAL_DATA_SELECTION).is_some()
    }

    /// The selected text, unwrapped and trimmed.
    pub fn selection_text(&self) -> Option<String> {
        unsafe {
            let mut opts: GhosttyTerminalSelectionFormatOptions = sized();
            opts.emit = GhosttyFormatterFormat::GHOSTTY_FORMATTER_FORMAT_PLAIN;
            opts.unwrap = true;
            opts.trim = true;
            let mut p = ptr::null_mut();
            let mut len = 0usize;
            if ghostty_terminal_selection_format_alloc(self.raw, ptr::null(), opts, &mut p, &mut len) != GhosttyResult::GHOSTTY_SUCCESS
                || p.is_null()
            {
                return None;
            }
            let s = String::from_utf8_lossy(std::slice::from_raw_parts(p, len)).into_owned();
            ghostty_free(ptr::null(), p, len);
            (!s.is_empty()).then_some(s)
        }
    }

    /// Everything in the terminal (scrollback + screen) as plain text.
    pub fn all_text(&mut self) -> String {
        if !unsafe {
            let mut sel: GhosttySelection = sized();
            ghostty_terminal_select_all(self.raw, &mut sel) == GhosttyResult::GHOSTTY_SUCCESS
                && ghostty_terminal_set(self.raw, GhosttyTerminalOption::GHOSTTY_TERMINAL_OPT_SELECTION, &sel as *const _ as *const _)
                    == GhosttyResult::GHOSTTY_SUCCESS
        } {
            return String::new();
        }
        let text = self.selection_text().unwrap_or_default();
        self.clear_selection();
        text
    }

    /// Build a drawable frame of the current viewport.
    pub fn frame(&mut self) -> Frame {
        unsafe {
            ghostty_render_state_update(self.render, self.raw);
            let mut colors: GhosttyRenderStateColors = sized();
            ghostty_render_state_get(
                self.render,
                GhosttyRenderStateData::GHOSTTY_RENDER_STATE_DATA_COLORS,
                &mut colors as *mut _ as *mut _,
            );
            let mut cur: GhosttyRenderStateCursor = sized();
            ghostty_render_state_get(self.render, GhosttyRenderStateData::GHOSTTY_RENDER_STATE_DATA_CURSOR, &mut cur as *mut _ as *mut _);
            let mut cols: u16 = 0;
            let mut rows: u16 = 0;
            ghostty_render_state_get(self.render, GhosttyRenderStateData::GHOSTTY_RENDER_STATE_DATA_COLS, &mut cols as *mut _ as *mut _);
            ghostty_render_state_get(self.render, GhosttyRenderStateData::GHOSTTY_RENDER_STATE_DATA_ROWS, &mut rows as *mut _ as *mut _);

            let fg_default: Rgb = colors.foreground.into();
            let bg_default: Rgb = colors.background.into();
            let palette = |i: u8| -> Rgb { colors.palette[i as usize].into() };

            let mut lines = Vec::with_capacity(rows as usize);
            ghostty_render_state_get(
                self.render,
                GhosttyRenderStateData::GHOSTTY_RENDER_STATE_DATA_ROW_ITERATOR,
                &mut self.rows_it as *mut _ as *mut _,
            );
            while ghostty_render_state_row_iterator_next(self.rows_it) {
                ghostty_render_state_row_get(
                    self.rows_it,
                    GhosttyRenderStateRowData::GHOSTTY_RENDER_STATE_ROW_DATA_CELLS,
                    &mut self.cells as *mut _ as *mut _,
                );
                let mut row = Row::default();
                let mut col: u16 = 0;
                let mut cps = [0u32; 32];
                while ghostty_render_state_row_cells_next(self.cells) {
                    let this_col = col;
                    col += 1;
                    let mut raw: GhosttyCell = 0;
                    ghostty_render_state_row_cells_get(
                        self.cells,
                        GhosttyRenderStateRowCellsData::GHOSTTY_RENDER_STATE_ROW_CELLS_DATA_RAW,
                        &mut raw as *mut _ as *mut _,
                    );
                    let mut wide = GhosttyCellWide::GHOSTTY_CELL_WIDE_NARROW;
                    ghostty_cell_get(raw, GhosttyCellData::GHOSTTY_CELL_DATA_WIDE, &mut wide as *mut _ as *mut _);
                    if wide == GhosttyCellWide::GHOSTTY_CELL_WIDE_SPACER_TAIL || wide == GhosttyCellWide::GHOSTTY_CELL_WIDE_SPACER_HEAD {
                        continue; // covered by the wide cell before it
                    }
                    let width: u8 = if wide == GhosttyCellWide::GHOSTTY_CELL_WIDE_WIDE { 2 } else { 1 };

                    let mut style: GhosttyStyle = sized();
                    ghostty_render_state_row_cells_get(
                        self.cells,
                        GhosttyRenderStateRowCellsData::GHOSTTY_RENDER_STATE_ROW_CELLS_DATA_STYLE,
                        &mut style as *mut _ as *mut _,
                    );
                    let mut fg_c = GhosttyColorRgb { r: 0, g: 0, b: 0 };
                    let has_fg = ghostty_render_state_row_cells_get(
                        self.cells,
                        GhosttyRenderStateRowCellsData::GHOSTTY_RENDER_STATE_ROW_CELLS_DATA_FG_COLOR,
                        &mut fg_c as *mut _ as *mut _,
                    ) == GhosttyResult::GHOSTTY_SUCCESS;
                    let mut bg_c = GhosttyColorRgb { r: 0, g: 0, b: 0 };
                    let has_bg = ghostty_render_state_row_cells_get(
                        self.cells,
                        GhosttyRenderStateRowCellsData::GHOSTTY_RENDER_STATE_ROW_CELLS_DATA_BG_COLOR,
                        &mut bg_c as *mut _ as *mut _,
                    ) == GhosttyResult::GHOSTTY_SUCCESS;
                    let mut selected = false;
                    ghostty_render_state_row_cells_get(
                        self.cells,
                        GhosttyRenderStateRowCellsData::GHOSTTY_RENDER_STATE_ROW_CELLS_DATA_SELECTED,
                        &mut selected as *mut _ as *mut _,
                    );

                    let mut fg: Rgb = if has_fg { fg_c.into() } else { fg_default };
                    let mut bg: Option<Rgb> = has_bg.then(|| bg_c.into());
                    if style.inverse {
                        let new_fg = bg.unwrap_or(bg_default);
                        bg = Some(fg);
                        fg = new_fg;
                    }
                    if style.invisible {
                        fg = bg.unwrap_or(bg_default);
                    }

                    let mut glen: u32 = 0;
                    ghostty_render_state_row_cells_get(
                        self.cells,
                        GhosttyRenderStateRowCellsData::GHOSTTY_RENDER_STATE_ROW_CELLS_DATA_GRAPHEMES_LEN,
                        &mut glen as *mut _ as *mut _,
                    );
                    let mut text = String::new();
                    if glen == 0 {
                        text.push(' ');
                    } else if (glen as usize) <= cps.len() {
                        ghostty_render_state_row_cells_get(
                            self.cells,
                            GhosttyRenderStateRowCellsData::GHOSTTY_RENDER_STATE_ROW_CELLS_DATA_GRAPHEMES_BUF,
                            cps.as_mut_ptr() as *mut _,
                        );
                        for &cp in &cps[..glen as usize] {
                            text.push(char::from_u32(cp).unwrap_or('\u{fffd}'));
                        }
                    } else {
                        text.push('\u{fffd}');
                    }

                    let underline = match style.underline {
                        0 => Underline::None,
                        1 => Underline::Single,
                        2 => Underline::Double,
                        3 => Underline::Curly,
                        4 => Underline::Dotted,
                        5 => Underline::Dashed,
                        _ => Underline::Single,
                    };
                    let underline_color = match style.underline_color.tag {
                        GhosttyStyleColorTag::GHOSTTY_STYLE_COLOR_RGB => Some(style.underline_color.value.rgb.into()),
                        GhosttyStyleColorTag::GHOSTTY_STYLE_COLOR_PALETTE => Some(palette(style.underline_color.value.palette)),
                        _ => None,
                    };

                    let same = row.runs.last().is_some_and(|r: &Run| {
                        r.col + r.cells == this_col
                            && r.fg == fg
                            && r.bg == bg
                            && r.bold == style.bold
                            && r.italic == style.italic
                            && r.faint == style.faint
                            && r.underline == underline
                            && r.underline_color == underline_color
                            && r.strikethrough == style.strikethrough
                            && r.overline == style.overline
                            && r.selected == selected
                    });
                    if same {
                        let r = row.runs.last_mut().unwrap();
                        r.text.push_str(&text);
                        r.widths.push(width);
                        r.cells += width as u16;
                    } else {
                        row.runs.push(Run {
                            col: this_col,
                            cells: width as u16,
                            text,
                            widths: vec![width],
                            fg,
                            bg,
                            bold: style.bold,
                            italic: style.italic,
                            faint: style.faint,
                            underline,
                            underline_color,
                            strikethrough: style.strikethrough,
                            overline: style.overline,
                            selected,
                        });
                    }
                }
                lines.push(row);
            }
            ghostty_render_state_clean(self.render);

            let cursor = (cur.visible && cur.viewport_has_value).then_some(Cursor {
                col: cur.viewport_x,
                row: cur.viewport_y,
                style: match cur.visual_style {
                    GhosttyRenderStateCursorVisualStyle::GHOSTTY_RENDER_STATE_CURSOR_VISUAL_STYLE_BAR => CursorStyle::Bar,
                    GhosttyRenderStateCursorVisualStyle::GHOSTTY_RENDER_STATE_CURSOR_VISUAL_STYLE_UNDERLINE => CursorStyle::Underline,
                    GhosttyRenderStateCursorVisualStyle::GHOSTTY_RENDER_STATE_CURSOR_VISUAL_STYLE_BLOCK_HOLLOW => CursorStyle::HollowBlock,
                    _ => CursorStyle::Block,
                },
                blinking: cur.blinking,
                password: cur.password_input,
                wide: cur.wide_tail,
            });

            Frame {
                cols,
                rows,
                background: bg_default,
                foreground: fg_default,
                cursor_color: colors.cursor_has_value.then(|| colors.cursor.into()),
                cursor,
                lines,
                scrollbar: self.scrollbar(),
                alternate_screen: self.alternate_screen(),
            }
        }
    }

    /// The visible screen as plain text lines (for tests and previews).
    pub fn screen_lines(&mut self) -> Vec<String> {
        self.frame().lines.iter().map(|r| r.runs.iter().map(|run| run.text.as_str()).collect::<String>().trim_end().to_owned()).collect()
    }

    /// Replace the 16 ANSI colors (themes), keeping the 256-color cube; `None` restores the built-in palette.
    pub fn set_ansi_palette(&mut self, ansi: Option<&[Rgb; 16]>) {
        unsafe {
            ghostty_terminal_set(self.raw, GhosttyTerminalOption::GHOSTTY_TERMINAL_OPT_COLOR_PALETTE, ptr::null());
            let Some(ansi) = ansi else { return };
            let Some(mut pal) = self.get::<[GhosttyColorRgb; 256]>(GhosttyTerminalData::GHOSTTY_TERMINAL_DATA_COLOR_PALETTE_DEFAULT) else { return };
            for (i, c) in ansi.iter().enumerate() {
                pal[i] = GhosttyColorRgb { r: c.r, g: c.g, b: c.b };
            }
            ghostty_terminal_set(self.raw, GhosttyTerminalOption::GHOSTTY_TERMINAL_OPT_COLOR_PALETTE, &pal as *const _ as *const _);
        }
    }

    /// Set default colors (from the stream's `theme` message).
    pub fn set_default_colors(&mut self, fg: Option<Rgb>, bg: Option<Rgb>, cursor: Option<Rgb>) {
        unsafe {
            let set = |opt: GhosttyTerminalOption, c: Option<Rgb>| match c {
                Some(c) => {
                    let v = GhosttyColorRgb { r: c.r, g: c.g, b: c.b };
                    ghostty_terminal_set(self.raw, opt, &v as *const _ as *const _);
                }
                None => {
                    ghostty_terminal_set(self.raw, opt, ptr::null());
                }
            };
            set(GhosttyTerminalOption::GHOSTTY_TERMINAL_OPT_COLOR_FOREGROUND, fg);
            set(GhosttyTerminalOption::GHOSTTY_TERMINAL_OPT_COLOR_BACKGROUND, bg);
            set(GhosttyTerminalOption::GHOSTTY_TERMINAL_OPT_COLOR_CURSOR, cursor);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_and_frames() {
        let mut t = Terminal::new(20, 4);
        t.write_vt(b"hi \x1b[1;31mred\x1b[0m\r\n\xe4\xbd\xa0ok");
        let f = t.frame();
        assert_eq!((f.cols, f.rows), (20, 4));
        let r0 = &f.lines[0].runs;
        assert_eq!(r0[0].text, "hi ");
        let red = r0.iter().find(|r| r.text == "red").unwrap();
        assert!(red.bold);
        assert_ne!(red.fg, f.foreground);
        let r1 = &f.lines[1].runs[0];
        assert_eq!(r1.widths[0], 2, "CJK is wide");
        assert!(r1.text.starts_with("你ok"));
        let c = f.cursor.unwrap();
        assert_eq!((c.col, c.row), (4, 1));
    }

    #[test]
    fn snapshot_roundtrip_through_stream() {
        // Encode a snapshot with libghostty-vt itself, then feed it as a stream.
        let mut src = Terminal::new(30, 5);
        src.write_vt(b"$ echo restored\r\nrestored\r\n$ ");
        let snap = unsafe {
            let mut p = ptr::null_mut();
            let mut n = 0usize;
            check(ghostty_snapshot_encode_alloc(src.raw, ptr::null(), &mut p, &mut n)).unwrap();
            let v = std::slice::from_raw_parts(p, n).to_vec();
            ghostty_free(ptr::null(), p, n);
            v
        };
        let mut stream = snap.clone();
        stream.extend_from_slice(b"live!");
        let mut dst = Terminal::new(80, 24);
        dst.begin_stream();
        // Feed in awkward chunks to exercise the framer.
        for chunk in stream.chunks(7) {
            dst.feed_stream(chunk).unwrap();
        }
        assert_eq!(dst.stream_state(), StreamState::Live);
        let lines = dst.screen_lines();
        assert_eq!(lines[0], "$ echo restored");
        assert_eq!(lines[1], "restored");
        assert_eq!(lines[2], "$ live!");
        assert_eq!(dst.size(), (30, 5));
    }

    #[test]
    fn ansi_palette_override() {
        let mut t = Terminal::new(10, 2);
        let mut ansi = [Rgb::new(1, 2, 3); 16];
        ansi[1] = Rgb::new(0xff, 0x00, 0x7f);
        t.set_ansi_palette(Some(&ansi));
        t.write_vt(b"\x1b[31mX");
        assert_eq!(t.frame().lines[0].runs[0].fg, Rgb::new(0xff, 0x00, 0x7f));
        t.set_ansi_palette(None);
        assert_ne!(t.frame().lines[0].runs[0].fg, Rgb::new(0xff, 0x00, 0x7f));
    }

    #[test]
    fn selection_and_scrollback() {
        let mut t = Terminal::new(10, 3);
        for i in 0..20 {
            t.write_vt(format!("line{i}\r\n").as_bytes());
        }
        let sb = t.scrollbar();
        assert!(sb.total > sb.len && sb.at_bottom());
        t.scroll(-5);
        assert!(!t.scrollbar().at_bottom());
        t.scroll_to_bottom();
        assert!(t.select_word(1, 0));
        assert_eq!(t.selection_text().as_deref(), Some("line18"));
        assert!(t.all_text().contains("line0\nline1\n"));
    }
}
