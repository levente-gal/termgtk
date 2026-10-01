//! Neutral cell grid. No Herdr JSON and no Ghostty types.
//!
//! A snapshot is a row-major rectangle of exactly `cols * rows` cells. Consumers
//! must not invent missing cells. A [`CellOccupancy::WideContinuation`] is the
//! trailing half of a wide glyph and is not painted on its own.
//!
//! Cursor: `x`/`y` are cell coordinates. When `Cursor::wide_tail` is set, the
//! cursor sits on the continuation cell; painters highlight the leading cell at
//! `x - 1` in the same row. Combining marks stay inside `Cell::grapheme` as one
//! cluster. Indexed colours stay indexed; this model does not apply a palette.
//! [`Style::reverse`] is reverse video and is not pre-swapped into the colours.
//!
//! Deliberately unsupported, and never synthesized: faint, blink, invisible,
//! strikethrough, overline, underline colour, and cursor shape. Ghostty
//! soft-wrap spacer heads are stored as [`CellOccupancy::WideContinuation`]
//! with an empty grapheme.

use thiserror::Error;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TerminalSnapshot {
    cols: u16,
    rows: u16,
    cursor: Cursor,
    cells: Vec<Cell>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cursor {
    pub x: u16,
    pub y: u16,
    pub visible: bool,
    pub wide_tail: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Cell {
    pub x: u16,
    pub y: u16,
    pub grapheme: String,
    pub fg: Color,
    pub bg: Color,
    pub style: Style,
    pub occupancy: CellOccupancy,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Style {
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub reverse: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Color {
    Default,
    Indexed(u8),
    Rgb(u8, u8, u8),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CellOccupancy {
    Single,
    WideLeading,
    WideContinuation,
}

/// One decoded terminal stream record.
///
/// `full` means the bytes are a complete viewport: the engine must be reset
/// before they are fed. `seq` restarts at 1 for each Herdr process.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TerminalEvent {
    Data(FrameBytes),
    Closed(TerminalCloseReason),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FrameBytes {
    pub bytes: Vec<u8>,
    pub full: bool,
    pub seq: u64,
    pub cols: u16,
    pub rows: u16,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TerminalCloseReason {
    pub reason: String,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ModelError {
    #[error("terminal grid must be at least 1x1")]
    EmptyGrid,
    #[error("expected {expected} cells, found {actual}")]
    CellCount { expected: usize, actual: usize },
    #[error("cell {index} is not at row-major position ({x}, {y})")]
    CellOrder { index: usize, x: u16, y: u16 },
    #[error("cursor ({x}, {y}) is outside {cols}x{rows}")]
    CursorOutOfRange {
        x: u16,
        y: u16,
        cols: u16,
        rows: u16,
    },
    #[error("wide cell pair is incomplete at ({x}, {y})")]
    BrokenWide { x: u16, y: u16 },
}

impl TerminalSnapshot {
    pub fn try_new(
        cols: u16,
        rows: u16,
        cursor: Cursor,
        cells: Vec<Cell>,
    ) -> Result<Self, ModelError> {
        if cols == 0 || rows == 0 {
            return Err(ModelError::EmptyGrid);
        }
        let expected = usize::from(cols) * usize::from(rows);
        if cells.len() != expected {
            return Err(ModelError::CellCount {
                expected,
                actual: cells.len(),
            });
        }
        for (index, cell) in cells.iter().enumerate() {
            let x = (index % usize::from(cols)) as u16;
            let y = (index / usize::from(cols)) as u16;
            if cell.x != x || cell.y != y {
                return Err(ModelError::CellOrder { index, x, y });
            }
            match cell.occupancy {
                CellOccupancy::WideLeading => {
                    let next = cells.get(index + 1);
                    let paired = next.is_some_and(|n| {
                        n.y == y && n.occupancy == CellOccupancy::WideContinuation
                    });
                    if !paired {
                        return Err(ModelError::BrokenWide { x, y });
                    }
                }
                CellOccupancy::WideContinuation => {
                    let prev_ok = index > 0
                        && cells[index - 1].y == y
                        && cells[index - 1].occupancy == CellOccupancy::WideLeading;
                    if !prev_ok {
                        return Err(ModelError::BrokenWide { x, y });
                    }
                }
                CellOccupancy::Single => {}
            }
        }
        if cursor.x >= cols || cursor.y >= rows {
            return Err(ModelError::CursorOutOfRange {
                x: cursor.x,
                y: cursor.y,
                cols,
                rows,
            });
        }
        Ok(Self {
            cols,
            rows,
            cursor,
            cells,
        })
    }

    pub fn blank(cols: u16, rows: u16) -> Result<Self, ModelError> {
        let mut cells = Vec::with_capacity(usize::from(cols) * usize::from(rows));
        for y in 0..rows {
            for x in 0..cols {
                cells.push(Cell::blank(x, y));
            }
        }
        Self::try_new(cols, rows, Cursor::hidden(), cells)
    }

    pub fn cols(&self) -> u16 {
        self.cols
    }

    pub fn rows(&self) -> u16 {
        self.rows
    }

    pub fn cursor(&self) -> Cursor {
        self.cursor
    }

    pub fn cells(&self) -> &[Cell] {
        &self.cells
    }

    pub fn cell(&self, x: u16, y: u16) -> Option<&Cell> {
        if x >= self.cols || y >= self.rows {
            return None;
        }
        let index = usize::from(y) * usize::from(self.cols) + usize::from(x);
        self.cells.get(index)
    }
}

impl Cursor {
    pub fn hidden() -> Self {
        Self {
            x: 0,
            y: 0,
            visible: false,
            wide_tail: false,
        }
    }

    pub fn at(x: u16, y: u16) -> Self {
        Self {
            x,
            y,
            visible: true,
            wide_tail: false,
        }
    }
}

impl Cell {
    pub fn blank(x: u16, y: u16) -> Self {
        Self {
            x,
            y,
            grapheme: String::new(),
            fg: Color::Default,
            bg: Color::Default,
            style: Style::default(),
            occupancy: CellOccupancy::Single,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grid(cols: u16, rows: u16, paint: impl Fn(&mut Cell)) -> TerminalSnapshot {
        let mut cells = Vec::new();
        for y in 0..rows {
            for x in 0..cols {
                let mut cell = Cell::blank(x, y);
                paint(&mut cell);
                cells.push(cell);
            }
        }
        TerminalSnapshot::try_new(cols, rows, Cursor::at(0, 0), cells).unwrap()
    }

    #[test]
    fn ascii_grid_is_rectangular() {
        let snap = grid(4, 2, |cell| {
            cell.grapheme = char::from(b'A' + cell.x as u8 + cell.y as u8 * 4).to_string();
        });
        assert_eq!(snap.cells().len(), 8);
        assert_eq!(snap.cell(3, 1).unwrap().grapheme, "H");
    }

    #[test]
    fn colors_keep_default_indexed_and_rgb() {
        let snap = grid(3, 1, |cell| match cell.x {
            0 => cell.fg = Color::Default,
            1 => {
                cell.fg = Color::Indexed(1);
                cell.bg = Color::Indexed(196);
            }
            _ => {
                cell.fg = Color::Rgb(1, 2, 3);
                cell.bg = Color::Rgb(4, 5, 6);
            }
        });
        assert_eq!(snap.cell(0, 0).unwrap().fg, Color::Default);
        assert_eq!(snap.cell(1, 0).unwrap().fg, Color::Indexed(1));
        assert_eq!(snap.cell(2, 0).unwrap().bg, Color::Rgb(4, 5, 6));
    }

    #[test]
    fn combining_mark_stays_one_cluster() {
        let snap = grid(1, 1, |cell| cell.grapheme = "e\u{0301}".to_string());
        assert_eq!(snap.cell(0, 0).unwrap().grapheme, "e\u{0301}");
        assert_eq!(snap.cell(0, 0).unwrap().occupancy, CellOccupancy::Single);
    }

    #[test]
    fn wide_pair_and_cursor_on_tail() {
        let cells = vec![
            Cell {
                grapheme: "你".into(),
                occupancy: CellOccupancy::WideLeading,
                ..Cell::blank(0, 0)
            },
            Cell {
                occupancy: CellOccupancy::WideContinuation,
                ..Cell::blank(1, 0)
            },
        ];
        let mut cursor = Cursor::at(1, 0);
        cursor.wide_tail = true;
        let snap = TerminalSnapshot::try_new(2, 1, cursor, cells).unwrap();
        assert!(snap.cursor().wide_tail);
        assert_eq!(
            snap.cell(1, 0).unwrap().occupancy,
            CellOccupancy::WideContinuation
        );
    }

    #[test]
    fn reverse_video_is_a_flag() {
        let snap = grid(1, 1, |cell| {
            cell.style.reverse = true;
            cell.fg = Color::Indexed(2);
            cell.bg = Color::Default;
        });
        let cell = snap.cell(0, 0).unwrap();
        assert!(cell.style.reverse);
        assert_eq!(cell.fg, Color::Indexed(2));
    }

    #[test]
    fn box_drawing_grid_preserves_glyphs() {
        let glyphs = ["┌", "─", "┐", "│", "", "│", "└", "─", "┘"];
        let mut cells = Vec::new();
        for (index, glyph) in glyphs.iter().enumerate() {
            let mut cell = Cell::blank((index % 3) as u16, (index / 3) as u16);
            cell.grapheme = (*glyph).to_string();
            cells.push(cell);
        }
        let snap = TerminalSnapshot::try_new(3, 3, Cursor::hidden(), cells).unwrap();
        assert_eq!(snap.cell(0, 0).unwrap().grapheme, "┌");
        assert_eq!(snap.cell(2, 2).unwrap().grapheme, "┘");
    }

    #[test]
    fn rejects_broken_wide_and_short_grid() {
        let err = TerminalSnapshot::try_new(1, 1, Cursor::hidden(), vec![]).unwrap_err();
        assert!(matches!(err, ModelError::CellCount { .. }));
        let broken = TerminalSnapshot::try_new(
            1,
            1,
            Cursor::hidden(),
            vec![Cell {
                occupancy: CellOccupancy::WideLeading,
                ..Cell::blank(0, 0)
            }],
        );
        assert!(matches!(broken, Err(ModelError::BrokenWide { .. })));
    }
}
