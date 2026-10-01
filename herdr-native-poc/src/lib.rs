//! Herdr native renderer POC.
//!
//! `herdr` owns the NDJSON protocol. `terminal` owns the neutral grid and the
//! Ghostty adapter. `render` paints `TerminalSnapshot` values. `app` composes
//! those layers and does not parse Herdr messages.

pub mod app;
pub mod herdr;
pub mod render;
pub mod terminal;
pub mod ui;

pub use terminal::{Cell, CellOccupancy, Color, Cursor, Style, TerminalEvent, TerminalSnapshot};
