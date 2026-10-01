# 03: Ghostty Terminal Engine

## Owner Paths

`herdr-native-poc/src/terminal/ghostty.rs`, Ghostty-specific files under `vendor/ghostty/`, and FFI-specific parts of `build.rs` by agreement with the foundation owner.

## Dependencies

The pinned revision and C API contract from discovery; neutral model from foundation.

## Work

1. Vendor the exact discovered Ghostty commit or use a committed, locked
   submodule. Never track `main` implicitly; record source provenance and
   licensing.
2. Build/link `libghostty-vt` through a reproducible `build.rs`; pin the
   required Zig version, avoid build-time network fetches, and fail with an
   actionable Zig/Ghostty/system-package diagnostic.
3. Encapsulate all unsafe/raw APIs in this module or a tiny C shim owned by this task.
4. Implement the Rust facade:

```rust
pub struct TerminalEngine;
impl TerminalEngine {
    pub fn new(cols: u16, rows: u16) -> Result<Self>;
    pub fn feed(&mut self, bytes: &[u8]);
    pub fn resize(&mut self, cols: u16, rows: u16);
    pub fn snapshot(&self) -> TerminalSnapshot;
}
```

5. Convert actual grid state into the neutral model: grapheme text, attributes,
   default/indexed/RGB colours, occupancy, and cursor. Copy text before
   returning a snapshot.
6. Document unsupported terminal features rather than silently faking them,
   including palette behavior, reverse video, combining characters, and input
   encoding.

## Tests

- ANSI fixture produces text, colours, bold, and cursor expected by model tests.
- Resize maintains reported dimensions.
- Unicode box characters and a common wide character do not corrupt adjacent cells.
- No unsafe call appears outside this module/shim; model-only tests do not
  require a graphical display.

## Handoff

The renderer gets snapshots only. The integrator owns mutation/thread scheduling and feeds bytes serially.
