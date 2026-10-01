# 01: Foundation and Neutral Model

## Owner Paths

`herdr-native-poc/Cargo.toml`, `Cargo.lock`, `build.rs`, `src/main.rs`,
`src/terminal/mod.rs`, `src/terminal/model.rs`, module declarations,
`tests/fixtures/`, and initial repository metadata.

## Work

1. Create the project tree stated in the POC, including placeholders for module boundaries.
2. Add only justified dependencies: GTK4/libadwaita/glib/gio, serde/json/base64, anyhow/thiserror, tracing/subscriber, plus test dependencies if needed.
3. Configure logging with `RUST_LOG`; ensure takeover warnings will be visible later.
4. Add `TerminalSnapshot`, `Cell`, `Cursor`, and neutral `Color` types. Define default/indexed/RGB colours, style flags, cell coordinates, grapheme-cluster text, explicit cell occupancy, and snapshot invariants.
5. Add fixture constructors and tests for ASCII, default/indexed/RGB ANSI colours, Unicode graphemes, wide leading/continuation cells, cursor positions, reverse video, and box-drawing grids under `tests/fixtures/<herdr-version>/`.
6. Define shared errors and the renderer-facing interface. Keep Ghostty and Herdr details out of this model.

## Published Contract

```rust
pub struct TerminalSnapshot { /* cols, rows, cursor, cells */ }
pub struct Cell { /* x, y, grapheme, colors, styles, occupancy */ }
pub enum CellOccupancy { Single, WideLeading, WideContinuation }
pub enum TerminalEvent { Data(Vec<u8>), Closed(TerminalCloseReason) }
```

Cells represent a row-major rectangular grid containing exactly `cols * rows`
cells. Consumers must not infer missing cells. A wide continuation cell is not
painted independently. The contract must state cursor behavior over wide cells,
combining marks, defaults, palette-indexed colours, reverse video, and every
style deliberately unsupported by the POC.

## Verification

- `cargo fmt --check`
- `cargo test`
- `cargo clippy --all-targets -- -D warnings` where GTK dependency versions permit it.
- `cargo run` opens a minimal GTK application or reports a useful setup error.

## Handoff

Publish exact types and constructors to core agents. Subsequent agents may add methods only through an agreed interface change.
