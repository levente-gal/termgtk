//! Renderer-neutral terminal state. Ghostty FFI stays in `ghostty`.

mod ghostty;
mod model;

pub use ghostty::TerminalEngine;
pub use model::{
    Cell, CellOccupancy, Color, Cursor, FrameBytes, ModelError, Style, TerminalCloseReason,
    TerminalEvent, TerminalSnapshot,
};
