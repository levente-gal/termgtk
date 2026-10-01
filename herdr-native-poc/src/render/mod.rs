//! Snapshot painting. The layout tree is optional and must not drop raw cells.

mod cell_renderer;
mod overlay;
mod terminal_view;

pub use cell_renderer::{device_cols_rows, group_runs, Run};
pub use overlay::*;
pub(crate) use terminal_view::cell_under_pointer;
pub use terminal_view::TerminalView;
