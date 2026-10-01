//! Ghostty VT adapter (`libghostty-vt`).
//!
//! Unsupported / not mapped into the neutral model: faint, blink, invisible,
//! strikethrough, overline, underline colour, cursor shape/blink, Kitty
//! graphics, selection, PTY effect callbacks, synchronized-output render
//! holds, keyboard encoding, and scrollback outside the viewport. Soft-wrap
//! spacer heads are stored as [`CellOccupancy::WideContinuation`] with an
//! empty grapheme (same as spacer tails).

#[cfg(feature = "ghostty")]
use crate::terminal::{Cell, CellOccupancy, Color, Cursor, Style};
use crate::terminal::{ModelError, TerminalSnapshot};

#[derive(Debug, thiserror::Error)]
pub enum GhosttyError {
    #[error("libghostty-vt is not linked ({0})")]
    Unavailable(&'static str),
    #[error("ghostty terminal create failed ({0})")]
    CreateFailed(i32),
    #[error(transparent)]
    Model(#[from] ModelError),
}

#[cfg(not(feature = "ghostty"))]
pub struct TerminalEngine;

#[cfg(not(feature = "ghostty"))]
impl TerminalEngine {
    pub fn new(_cols: u16, _rows: u16) -> Result<Self, GhosttyError> {
        Err(GhosttyError::Unavailable(
            "enable the ghostty Cargo feature to link libghostty-vt",
        ))
    }

    pub fn feed(&mut self, _bytes: &[u8]) {}

    pub fn resize(&mut self, _cols: u16, _rows: u16) {}

    pub fn set_cell_px(&mut self, _width: u32, _height: u32) {}

    pub fn snapshot(&self) -> TerminalSnapshot {
        TerminalSnapshot::blank(1, 1).expect("1x1 blank")
    }
}

#[cfg(feature = "ghostty")]
type GhosttyResult = std::os::raw::c_int;
#[cfg(feature = "ghostty")]
const GHOSTTY_SUCCESS: GhosttyResult = 0;
#[cfg(feature = "ghostty")]
const GHOSTTY_OUT_OF_SPACE: GhosttyResult = -3;

#[cfg(feature = "ghostty")]
type GhosttyTerminal = *mut std::os::raw::c_void;
#[cfg(feature = "ghostty")]
type GhosttyRenderState = *mut std::os::raw::c_void;
#[cfg(feature = "ghostty")]
type GhosttyRenderStateRowIterator = *mut std::os::raw::c_void;
#[cfg(feature = "ghostty")]
type GhosttyRenderStateRowCells = *mut std::os::raw::c_void;
#[cfg(feature = "ghostty")]
type GhosttyCell = u64;

#[cfg(feature = "ghostty")]
const GHOSTTY_RENDER_STATE_DATA_ROW_ITERATOR: std::os::raw::c_int = 4;
#[cfg(feature = "ghostty")]
const GHOSTTY_RENDER_STATE_DATA_CURSOR: std::os::raw::c_int = 18;
#[cfg(feature = "ghostty")]
const GHOSTTY_RENDER_STATE_ROW_DATA_CELLS: std::os::raw::c_int = 3;
#[cfg(feature = "ghostty")]
const GHOSTTY_RENDER_STATE_ROW_CELLS_DATA_RAW: std::os::raw::c_int = 1;
#[cfg(feature = "ghostty")]
const GHOSTTY_RENDER_STATE_ROW_CELLS_DATA_STYLE: std::os::raw::c_int = 2;
#[cfg(feature = "ghostty")]
const GHOSTTY_RENDER_STATE_ROW_CELLS_DATA_GRAPHEMES_UTF8: std::os::raw::c_int = 9;
#[cfg(feature = "ghostty")]
const GHOSTTY_CELL_DATA_WIDE: std::os::raw::c_int = 3;
#[cfg(feature = "ghostty")]
const GHOSTTY_CELL_WIDE_NARROW: std::os::raw::c_int = 0;
#[cfg(feature = "ghostty")]
const GHOSTTY_CELL_WIDE_WIDE: std::os::raw::c_int = 1;
#[cfg(feature = "ghostty")]
const GHOSTTY_CELL_WIDE_SPACER_TAIL: std::os::raw::c_int = 2;
#[cfg(feature = "ghostty")]
const GHOSTTY_CELL_WIDE_SPACER_HEAD: std::os::raw::c_int = 3;
#[cfg(feature = "ghostty")]
const GHOSTTY_STYLE_COLOR_NONE: std::os::raw::c_int = 0;
#[cfg(feature = "ghostty")]
const GHOSTTY_STYLE_COLOR_PALETTE: std::os::raw::c_int = 1;
#[cfg(feature = "ghostty")]
const GHOSTTY_STYLE_COLOR_RGB: std::os::raw::c_int = 2;
#[cfg(feature = "ghostty")]
const GHOSTTY_SGR_UNDERLINE_NONE: std::os::raw::c_int = 0;

#[cfg(feature = "ghostty")]
#[repr(C)]
#[derive(Clone, Copy)]
struct GhosttyColorRgb {
    r: u8,
    g: u8,
    b: u8,
}

#[cfg(feature = "ghostty")]
#[repr(C)]
#[derive(Clone, Copy)]
union GhosttyStyleColorValue {
    palette: u8,
    rgb: GhosttyColorRgb,
    _padding: u64,
}

#[cfg(feature = "ghostty")]
#[repr(C)]
#[derive(Clone, Copy)]
struct GhosttyStyleColor {
    tag: std::os::raw::c_int,
    _pad: [u8; 4],
    value: GhosttyStyleColorValue,
}

#[cfg(feature = "ghostty")]
#[repr(C)]
#[derive(Clone, Copy)]
struct GhosttyStyle {
    size: usize,
    fg_color: GhosttyStyleColor,
    bg_color: GhosttyStyleColor,
    underline_color: GhosttyStyleColor,
    bold: bool,
    italic: bool,
    faint: bool,
    blink: bool,
    inverse: bool,
    invisible: bool,
    strikethrough: bool,
    overline: bool,
    underline: std::os::raw::c_int,
}

#[cfg(feature = "ghostty")]
#[repr(C)]
#[derive(Clone, Copy)]
struct GhosttyRenderStateCursor {
    size: usize,
    viewport_has_value: bool,
    viewport_x: u16,
    viewport_y: u16,
    wide_tail: bool,
    visible: bool,
    blinking: bool,
    password_input: bool,
    visual_style: std::os::raw::c_int,
}

#[cfg(feature = "ghostty")]
#[repr(C)]
struct GhosttyBuffer {
    ptr: *mut u8,
    cap: usize,
    len: usize,
}

#[cfg(feature = "ghostty")]
#[link(name = "ghostty-vt", kind = "static")]
unsafe extern "C" {
    fn ghostty_terminal_new(
        allocator: *const std::os::raw::c_void,
        terminal: *mut GhosttyTerminal,
        cols: u16,
        rows: u16,
    ) -> GhosttyResult;

    fn ghostty_terminal_free(terminal: GhosttyTerminal);

    fn ghostty_terminal_resize(
        terminal: GhosttyTerminal,
        cols: u16,
        rows: u16,
        cell_width_px: u32,
        cell_height_px: u32,
    ) -> GhosttyResult;

    fn ghostty_terminal_vt_write(terminal: GhosttyTerminal, data: *const u8, len: usize);

    fn ghostty_render_state_new(
        allocator: *const std::os::raw::c_void,
        state: *mut GhosttyRenderState,
    ) -> GhosttyResult;

    fn ghostty_render_state_free(state: GhosttyRenderState);

    fn ghostty_render_state_update(
        state: GhosttyRenderState,
        terminal: GhosttyTerminal,
    ) -> GhosttyResult;

    fn ghostty_render_state_get(
        state: GhosttyRenderState,
        data: std::os::raw::c_int,
        out: *mut std::os::raw::c_void,
    ) -> GhosttyResult;

    fn ghostty_render_state_row_iterator_new(
        allocator: *const std::os::raw::c_void,
        out_iterator: *mut GhosttyRenderStateRowIterator,
    ) -> GhosttyResult;

    fn ghostty_render_state_row_iterator_free(iterator: GhosttyRenderStateRowIterator);

    fn ghostty_render_state_row_iterator_next(iterator: GhosttyRenderStateRowIterator) -> bool;

    fn ghostty_render_state_row_get(
        iterator: GhosttyRenderStateRowIterator,
        data: std::os::raw::c_int,
        out: *mut std::os::raw::c_void,
    ) -> GhosttyResult;

    fn ghostty_render_state_row_cells_new(
        allocator: *const std::os::raw::c_void,
        out_cells: *mut GhosttyRenderStateRowCells,
    ) -> GhosttyResult;

    fn ghostty_render_state_row_cells_free(cells: GhosttyRenderStateRowCells);

    fn ghostty_render_state_row_cells_next(cells: GhosttyRenderStateRowCells) -> bool;

    fn ghostty_render_state_row_cells_get(
        cells: GhosttyRenderStateRowCells,
        data: std::os::raw::c_int,
        out: *mut std::os::raw::c_void,
    ) -> GhosttyResult;

    fn ghostty_cell_get(
        cell: GhosttyCell,
        data: std::os::raw::c_int,
        out: *mut std::os::raw::c_void,
    ) -> GhosttyResult;
}

#[cfg(feature = "ghostty")]
pub struct TerminalEngine {
    terminal: GhosttyTerminal,
    render: GhosttyRenderState,
    cols: u16,
    rows: u16,
    cell_w: u32,
    cell_h: u32,
}

#[cfg(feature = "ghostty")]
impl TerminalEngine {
    pub fn new(cols: u16, rows: u16) -> Result<Self, GhosttyError> {
        if cols == 0 || rows == 0 {
            return Err(ModelError::EmptyGrid.into());
        }

        debug_assert_eq!(std::mem::size_of::<GhosttyStyle>(), 72);
        debug_assert_eq!(std::mem::size_of::<GhosttyRenderStateCursor>(), 24);
        debug_assert_eq!(std::mem::size_of::<GhosttyStyleColor>(), 16);

        let mut terminal: GhosttyTerminal = std::ptr::null_mut();
        let rc = unsafe { ghostty_terminal_new(std::ptr::null(), &mut terminal, cols, rows) };
        if rc != GHOSTTY_SUCCESS || terminal.is_null() {
            return Err(GhosttyError::CreateFailed(rc));
        }

        let mut render: GhosttyRenderState = std::ptr::null_mut();
        let rc = unsafe { ghostty_render_state_new(std::ptr::null(), &mut render) };
        if rc != GHOSTTY_SUCCESS || render.is_null() {
            unsafe { ghostty_terminal_free(terminal) };
            return Err(GhosttyError::CreateFailed(rc));
        }

        let cell_w = 1;
        let cell_h = 1;
        let _ = unsafe { ghostty_terminal_resize(terminal, cols, rows, cell_w, cell_h) };

        Ok(Self {
            terminal,
            render,
            cols,
            rows,
            cell_w,
            cell_h,
        })
    }

    pub fn feed(&mut self, bytes: &[u8]) {
        if bytes.is_empty() {
            return;
        }
        unsafe {
            ghostty_terminal_vt_write(self.terminal, bytes.as_ptr(), bytes.len());
        }
    }

    pub fn resize(&mut self, cols: u16, rows: u16) {
        if cols == 0 || rows == 0 {
            return;
        }
        let rc =
            unsafe { ghostty_terminal_resize(self.terminal, cols, rows, self.cell_w, self.cell_h) };
        if rc == GHOSTTY_SUCCESS {
            self.cols = cols;
            self.rows = rows;
        }
    }

    pub fn set_cell_px(&mut self, width: u32, height: u32) {
        self.cell_w = width.max(1);
        self.cell_h = height.max(1);
        let _ = unsafe {
            ghostty_terminal_resize(
                self.terminal,
                self.cols,
                self.rows,
                self.cell_w,
                self.cell_h,
            )
        };
    }

    pub fn snapshot(&self) -> TerminalSnapshot {
        let blank = || {
            TerminalSnapshot::blank(self.cols, self.rows)
                .unwrap_or_else(|_| TerminalSnapshot::blank(1, 1).expect("1x1 blank"))
        };

        let rc = unsafe { ghostty_render_state_update(self.render, self.terminal) };
        if rc != GHOSTTY_SUCCESS {
            return blank();
        }

        match self.build_snapshot() {
            Ok(snap) => snap,
            Err(_) => blank(),
        }
    }

    fn build_snapshot(&self) -> Result<TerminalSnapshot, GhosttyError> {
        let mut cursor_c = {
            let mut s = unsafe { std::mem::zeroed::<GhosttyRenderStateCursor>() };
            s.size = std::mem::size_of::<GhosttyRenderStateCursor>();
            s
        };
        let rc = unsafe {
            ghostty_render_state_get(
                self.render,
                GHOSTTY_RENDER_STATE_DATA_CURSOR,
                &mut cursor_c as *mut _ as *mut _,
            )
        };
        let cursor = if rc == GHOSTTY_SUCCESS && cursor_c.viewport_has_value {
            Cursor {
                x: cursor_c.viewport_x.min(self.cols.saturating_sub(1)),
                y: cursor_c.viewport_y.min(self.rows.saturating_sub(1)),
                visible: cursor_c.visible,
                wide_tail: cursor_c.wide_tail,
            }
        } else if rc == GHOSTTY_SUCCESS {
            Cursor {
                x: 0,
                y: 0,
                visible: cursor_c.visible,
                wide_tail: false,
            }
        } else {
            Cursor::hidden()
        };

        let mut row_iter: GhosttyRenderStateRowIterator = std::ptr::null_mut();
        let rc = unsafe { ghostty_render_state_row_iterator_new(std::ptr::null(), &mut row_iter) };
        if rc != GHOSTTY_SUCCESS || row_iter.is_null() {
            return Err(GhosttyError::CreateFailed(rc));
        }

        let mut cells_h: GhosttyRenderStateRowCells = std::ptr::null_mut();
        let rc = unsafe { ghostty_render_state_row_cells_new(std::ptr::null(), &mut cells_h) };
        if rc != GHOSTTY_SUCCESS || cells_h.is_null() {
            unsafe { ghostty_render_state_row_iterator_free(row_iter) };
            return Err(GhosttyError::CreateFailed(rc));
        }

        let rc = unsafe {
            ghostty_render_state_get(
                self.render,
                GHOSTTY_RENDER_STATE_DATA_ROW_ITERATOR,
                &mut row_iter as *mut _ as *mut _,
            )
        };
        if rc != GHOSTTY_SUCCESS {
            unsafe {
                ghostty_render_state_row_cells_free(cells_h);
                ghostty_render_state_row_iterator_free(row_iter);
            }
            return Err(GhosttyError::CreateFailed(rc));
        }

        let expected = usize::from(self.cols) * usize::from(self.rows);
        let mut cells = Vec::with_capacity(expected);
        let mut y: u16 = 0;

        while unsafe { ghostty_render_state_row_iterator_next(row_iter) } {
            if y >= self.rows {
                break;
            }
            let rc = unsafe {
                ghostty_render_state_row_get(
                    row_iter,
                    GHOSTTY_RENDER_STATE_ROW_DATA_CELLS,
                    &mut cells_h as *mut _ as *mut _,
                )
            };
            if rc != GHOSTTY_SUCCESS {
                unsafe {
                    ghostty_render_state_row_cells_free(cells_h);
                    ghostty_render_state_row_iterator_free(row_iter);
                }
                return Err(GhosttyError::CreateFailed(rc));
            }

            let mut x: u16 = 0;
            while unsafe { ghostty_render_state_row_cells_next(cells_h) } {
                if x >= self.cols {
                    break;
                }
                cells.push(Self::read_cell(cells_h, x, y));
                x = x.saturating_add(1);
            }
            while x < self.cols {
                cells.push(Cell::blank(x, y));
                x = x.saturating_add(1);
            }
            y = y.saturating_add(1);
        }

        while y < self.rows {
            for x in 0..self.cols {
                cells.push(Cell::blank(x, y));
            }
            y = y.saturating_add(1);
        }

        unsafe {
            ghostty_render_state_row_cells_free(cells_h);
            ghostty_render_state_row_iterator_free(row_iter);
        }

        cells.truncate(expected);
        while cells.len() < expected {
            let i = cells.len();
            let x = (i % usize::from(self.cols)) as u16;
            let y = (i / usize::from(self.cols)) as u16;
            cells.push(Cell::blank(x, y));
        }

        Ok(TerminalSnapshot::try_new(
            self.cols, self.rows, cursor, cells,
        )?)
    }

    fn read_cell(cells_h: GhosttyRenderStateRowCells, x: u16, y: u16) -> Cell {
        let mut style = {
            let mut s = unsafe { std::mem::zeroed::<GhosttyStyle>() };
            s.size = std::mem::size_of::<GhosttyStyle>();
            s
        };
        let _ = unsafe {
            ghostty_render_state_row_cells_get(
                cells_h,
                GHOSTTY_RENDER_STATE_ROW_CELLS_DATA_STYLE,
                &mut style as *mut _ as *mut _,
            )
        };

        let mut raw: GhosttyCell = 0;
        let _ = unsafe {
            ghostty_render_state_row_cells_get(
                cells_h,
                GHOSTTY_RENDER_STATE_ROW_CELLS_DATA_RAW,
                &mut raw as *mut _ as *mut _,
            )
        };

        let mut wide: std::os::raw::c_int = GHOSTTY_CELL_WIDE_NARROW;
        let _ =
            unsafe { ghostty_cell_get(raw, GHOSTTY_CELL_DATA_WIDE, &mut wide as *mut _ as *mut _) };

        let occupancy = match wide {
            GHOSTTY_CELL_WIDE_WIDE => CellOccupancy::WideLeading,
            GHOSTTY_CELL_WIDE_SPACER_TAIL | GHOSTTY_CELL_WIDE_SPACER_HEAD => {
                CellOccupancy::WideContinuation
            }
            _ => CellOccupancy::Single,
        };

        let grapheme = if occupancy == CellOccupancy::WideContinuation {
            String::new()
        } else {
            Self::read_grapheme_utf8(cells_h)
        };

        Cell {
            x,
            y,
            grapheme,
            fg: map_color(style.fg_color),
            bg: map_color(style.bg_color),
            style: Style {
                bold: style.bold,
                italic: style.italic,
                underline: style.underline != GHOSTTY_SGR_UNDERLINE_NONE,
                reverse: style.inverse,
            },
            occupancy,
        }
    }

    fn read_grapheme_utf8(cells_h: GhosttyRenderStateRowCells) -> String {
        let mut buf = GhosttyBuffer {
            ptr: std::ptr::null_mut(),
            cap: 0,
            len: 0,
        };
        let rc = unsafe {
            ghostty_render_state_row_cells_get(
                cells_h,
                GHOSTTY_RENDER_STATE_ROW_CELLS_DATA_GRAPHEMES_UTF8,
                &mut buf as *mut _ as *mut _,
            )
        };
        if rc == GHOSTTY_SUCCESS {
            return String::new();
        }
        if rc != GHOSTTY_OUT_OF_SPACE || buf.len == 0 {
            return String::new();
        }
        let need = buf.len;
        let mut storage = vec![0u8; need];
        let mut buf = GhosttyBuffer {
            ptr: storage.as_mut_ptr(),
            cap: storage.len(),
            len: 0,
        };
        let rc = unsafe {
            ghostty_render_state_row_cells_get(
                cells_h,
                GHOSTTY_RENDER_STATE_ROW_CELLS_DATA_GRAPHEMES_UTF8,
                &mut buf as *mut _ as *mut _,
            )
        };
        if rc != GHOSTTY_SUCCESS {
            return String::new();
        }
        String::from_utf8_lossy(&storage[..buf.len]).into_owned()
    }
}

#[cfg(feature = "ghostty")]
fn map_color(c: GhosttyStyleColor) -> Color {
    match c.tag {
        GHOSTTY_STYLE_COLOR_PALETTE => Color::Indexed(unsafe { c.value.palette }),
        GHOSTTY_STYLE_COLOR_RGB => {
            let rgb = unsafe { c.value.rgb };
            Color::Rgb(rgb.r, rgb.g, rgb.b)
        }
        GHOSTTY_STYLE_COLOR_NONE | _ => Color::Default,
    }
}

#[cfg(feature = "ghostty")]
impl Drop for TerminalEngine {
    fn drop(&mut self) {
        unsafe {
            if !self.render.is_null() {
                ghostty_render_state_free(self.render);
                self.render = std::ptr::null_mut();
            }
            if !self.terminal.is_null() {
                ghostty_terminal_free(self.terminal);
                self.terminal = std::ptr::null_mut();
            }
        }
    }
}

#[cfg(all(test, feature = "ghostty"))]
mod tests {
    use super::*;

    #[test]
    fn ansi_styles_cursor_and_text() {
        let mut eng = TerminalEngine::new(20, 5).expect("new");
        eng.feed(b"\x1b[31;1mHi\x1b[0m\x1b[3;6H");
        let snap = eng.snapshot();
        assert_eq!(snap.cols(), 20);
        assert_eq!(snap.rows(), 5);
        let c0 = snap.cell(0, 0).unwrap();
        assert_eq!(c0.grapheme, "H");
        assert_eq!(c0.fg, Color::Indexed(1));
        assert!(c0.style.bold);
        let c1 = snap.cell(1, 0).unwrap();
        assert_eq!(c1.grapheme, "i");
        assert_eq!(c1.fg, Color::Indexed(1));
        let cur = snap.cursor();
        assert_eq!((cur.x, cur.y), (5, 2));
        assert!(cur.visible);
    }

    #[test]
    fn resize_updates_dimensions() {
        let mut eng = TerminalEngine::new(10, 4).expect("new");
        eng.feed(b"abc");
        eng.resize(16, 8);
        let snap = eng.snapshot();
        assert_eq!(snap.cols(), 16);
        assert_eq!(snap.rows(), 8);
        assert_eq!(snap.cells().len(), 16 * 8);
    }

    #[test]
    fn wide_and_box_drawing_neighbors() {
        let mut eng = TerminalEngine::new(8, 3).expect("new");
        eng.feed("あA┌┐".as_bytes());
        let snap = eng.snapshot();
        let lead = snap.cell(0, 0).unwrap();
        let cont = snap.cell(1, 0).unwrap();
        let a = snap.cell(2, 0).unwrap();
        assert_eq!(lead.occupancy, CellOccupancy::WideLeading);
        assert_eq!(cont.occupancy, CellOccupancy::WideContinuation);
        assert!(cont.grapheme.is_empty());
        assert_eq!(a.grapheme, "A");
        assert_eq!(a.occupancy, CellOccupancy::Single);
        let box_l = snap.cell(3, 0).unwrap();
        let box_r = snap.cell(4, 0).unwrap();
        assert_eq!(box_l.grapheme, "┌");
        assert_eq!(box_r.grapheme, "┐");
        assert_eq!(box_l.occupancy, CellOccupancy::Single);
        assert_eq!(box_r.occupancy, CellOccupancy::Single);
    }
}
