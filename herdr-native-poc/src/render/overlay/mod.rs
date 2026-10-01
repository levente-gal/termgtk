//! Layout tree for a terminal frame.
//!
//! `parse` reads closed boxes out of the cell grid. `reconcile` merges that
//! tree into the previous one and prints it when the structure changes.
//! Enhanced mode draws the tree with GTK widgets instead of the cell grid.

mod cards;
mod tree;

#[cfg(feature = "gtk")]
mod gui;
#[cfg(feature = "gtk")]
mod paint;

#[cfg(feature = "gtk")]
pub(crate) use cards::{enhanced_chrome, SCRIM_ALPHA};

#[cfg(test)]
pub(crate) mod test_grid;

pub use tree::{parse, reconcile, LayoutTree};

#[cfg(feature = "gtk")]
pub(crate) use gui::PaneGui;
