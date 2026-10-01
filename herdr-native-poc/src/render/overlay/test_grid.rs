use crate::terminal::{Cell, Cursor, TerminalSnapshot};

pub(crate) fn test_grid(cols: u16, rows: u16, paint: impl Fn(&mut Cell)) -> TerminalSnapshot {
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
