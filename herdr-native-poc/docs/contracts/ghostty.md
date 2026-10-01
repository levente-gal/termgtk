# ghostty.md — libghostty-vt FFI contract

Research-only discovery for Agent B. No Ghostty sources are vendored into this repo yet.

## Verdict

| Question | Answer |
| --- | --- |
| Newest stable release with usable terminal object C API? | **None.** `v1.3.1` (`332b2aefc6e72d363aa93ab6ecfc86eeeeb5ed28`) ships `libghostty-vt` but only key/OSC/SGR/paste parsers — **no** `ghostty_terminal_*`, render-state, or grid iteration. |
| Pin for this POC | **`b40acce58dcf77df52231c3798ea58e924647c89`** (floating tag `tip` as of 2026-09-28) |
| C API sufficient for planned `TerminalEngine`? | **Yes**, at tip: create / feed / resize / destroy / grid iterate / styles / cursor are all present. API is explicitly marked **unstable / WIP**. |
| C shim required? | **No.** Map C → Rust in `terminal/ghostty.rs` (bindgen + thin safe wrapper). Optional convenience C helpers are unnecessary for the planned facade. |
| Zig required | **`0.16.0`** (`build.zig.zon` `minimum_zig_version`) |
| Local `libghostty-vt` build | **Succeeded** in `/tmp` (see `environment-ghostty.md`) |

## Provenance

| Field | Value |
| --- | --- |
| Source | https://github.com/ghostty-org/ghostty |
| Pinned commit | `b40acce58dcf77df52231c3798ea58e924647c89` |
| Tag at pin | `tip` (not a semver release; re-pin by hash only) |
| License | **MIT** (`LICENSE`: “Copyright (c) 2024 Mitchell Hashimoto, Ghostty contributors”) |
| Rejected stable | `v1.3.1` / `332b2aef…` — Zig `0.15.2`, insufficient C surface for `TerminalEngine` |

Do not track `tip`/`main` by name in `build.rs`; pin the hash above (or a later hash after deliberate re-discovery).

## Stability

Upstream `include/ghostty/vt.h` states the C API is incomplete, work-in-progress, and will change. README likewise: functionality is production-proven inside Ghostty GUI, but **C signatures are in flux** and libghostty is not yet version-tagged. Acceptable for a pinned POC; expect breakage on tip moves.

---

## Planned Rust facade (adapter-internal C)

```rust
pub struct TerminalEngine;
impl TerminalEngine {
    pub fn new(cols: u16, rows: u16) -> Result<Self>;
    pub fn feed(&mut self, bytes: &[u8]);
    pub fn resize(&mut self, cols: u16, rows: u16);
    pub fn snapshot(&self) -> TerminalSnapshot;
}
```

Raw C types (`GhosttyTerminal`, `GhosttyRenderState`, `GhosttyCell`, `GhosttyStyle`, …) must stay inside the Ghostty adapter module. Public model types come from foundation (`TerminalSnapshot`, `Cell`, `Color`, `Cursor`, `CellOccupancy`).

---

## Exact C signatures (quoted from tip headers)

Opaque handle (`include/ghostty/vt/types.h`):

```c
typedef struct GhosttyTerminalImpl* GhosttyTerminal;
typedef struct GhosttyRenderStateImpl* GhosttyRenderState;
typedef struct GhosttyRenderStateRowIteratorImpl* GhosttyRenderStateRowIterator;
typedef struct GhosttyRenderStateRowCellsImpl* GhosttyRenderStateRowCells;
```

### Create / destroy / feed / resize

From `include/ghostty/vt/terminal.h`:

```c
GHOSTTY_API GhosttyResult ghostty_terminal_new(
    const GhosttyAllocator* allocator,
    GhosttyTerminal* terminal,
    uint16_t cols,
    uint16_t rows);

GHOSTTY_API void ghostty_terminal_free(GhosttyTerminal terminal);

GHOSTTY_API void ghostty_terminal_reset(GhosttyTerminal terminal);

GHOSTTY_API GhosttyResult ghostty_terminal_resize(
    GhosttyTerminal terminal,
    uint16_t cols,
    uint16_t rows,
    uint32_t cell_width_px,
    uint32_t cell_height_px);

GHOSTTY_API void ghostty_terminal_vt_write(
    GhosttyTerminal terminal,
    const uint8_t* data,
    size_t len);
```

Notes from upstream docs on these calls:

- `ghostty_terminal_vt_write` **never fails**; malformed input is absorbed.
- `ghostty_terminal_resize` requires pixel cell size (for image protocols / size reports). The Rust facade only takes cols/rows — adapter must pass measured font metrics later, or **`1, 1`** until the GTK renderer supplies real metrics.
- Default allocator: pass `NULL` for `allocator` (as in `example/c-vt-stream`).

### Snapshot / grid iteration (render state)

There is **no** single `ghostty_terminal_snapshot_grid()` for cell painting. Use the render-state API (`include/ghostty/vt/render.h`). Upstream also has a separate encode/restore “snapshot” API in `snapshot.h` — that is **persistence**, not the POC paint snapshot.

```c
GHOSTTY_API GhosttyResult ghostty_render_state_new(
    const GhosttyAllocator* allocator,
    GhosttyRenderState* state);

GHOSTTY_API void ghostty_render_state_free(GhosttyRenderState state);

GHOSTTY_API GhosttyResult ghostty_render_state_update(
    GhosttyRenderState state,
    GhosttyTerminal terminal);

GHOSTTY_API GhosttyResult ghostty_render_state_get(
    GhosttyRenderState state,
    GhosttyRenderStateData data,
    void* out);

GHOSTTY_API GhosttyResult ghostty_render_state_row_iterator_new(
    const GhosttyAllocator* allocator,
    GhosttyRenderStateRowIterator* out_iterator);

GHOSTTY_API bool ghostty_render_state_row_iterator_next(
    GhosttyRenderStateRowIterator iterator);

GHOSTTY_API GhosttyResult ghostty_render_state_row_get(
    GhosttyRenderStateRowIterator iterator,
    GhosttyRenderStateRowData data,
    void* out);

GHOSTTY_API GhosttyResult ghostty_render_state_row_cells_new(
    const GhosttyAllocator* allocator,
    GhosttyRenderStateRowCells* out_cells);

GHOSTTY_API bool ghostty_render_state_row_cells_next(
    GhosttyRenderStateRowCells cells);

GHOSTTY_API GhosttyResult ghostty_render_state_row_cells_get(
    GhosttyRenderStateRowCells cells,
    GhosttyRenderStateRowCellsData data,
    void* out);

GHOSTTY_API void ghostty_render_state_row_cells_free(
    GhosttyRenderStateRowCells cells);

GHOSTTY_API void ghostty_render_state_row_iterator_free(
    GhosttyRenderStateRowIterator iterator);
```

Canonical iteration pattern (from `example/c-vt-render/src/main.c`):

1. `ghostty_render_state_update(render_state, terminal)`
2. `ghostty_render_state_get(..., GHOSTTY_RENDER_STATE_DATA_COLORS, &colors)`
3. `ghostty_render_state_get(..., GHOSTTY_RENDER_STATE_DATA_CURSOR, &cursor)`
4. Bind row iterator via `GHOSTTY_RENDER_STATE_DATA_ROW_ITERATOR`
5. Per row: `ghostty_render_state_row_get(..., GHOSTTY_RENDER_STATE_ROW_DATA_CELLS, &cells)` then `ghostty_render_state_row_cells_next` / `_get`

For a full rectangular `TerminalSnapshot`, use `ghostty_render_state_row_iterator_next` (all rows), not only `…_next_dirty`.

Low-level packed cells also exist (`ghostty_cell_get` / `GhosttyCell` in `screen.h`) but render-state row cells are the preferred path for graphemes + resolved styles.

### Cell / style / colour / wide / cursor representation

**Style** (`include/ghostty/vt/style.h`):

```c
typedef struct {
  size_t size;
  GhosttyStyleColor fg_color;   /* tag: NONE | PALETTE | RGB */
  GhosttyStyleColor bg_color;
  GhosttyStyleColor underline_color;
  bool bold;
  bool italic;
  bool faint;
  bool blink;
  bool inverse;                 /* reverse video */
  bool invisible;
  bool strikethrough;
  bool overline;
  int underline;                /* GHOSTTY_SGR_UNDERLINE_* */
} GhosttyStyle;
```

**RGB** (`include/ghostty/vt/color.h`):

```c
typedef struct {
  uint8_t r;
  uint8_t g;
  uint8_t b;
} GhosttyColorRgb;
```

**Wide cells** (`GhosttyCellWide` via `GHOSTTY_CELL_DATA_WIDE` / render-state raw cell):

| Enum | Meaning | Map to `CellOccupancy` |
| --- | --- | --- |
| `GHOSTTY_CELL_WIDE_NARROW` | width 1 | `Single` |
| `GHOSTTY_CELL_WIDE_WIDE` | wide leading | `WideLeading` |
| `GHOSTTY_CELL_WIDE_SPACER_TAIL` | do not paint | `WideContinuation` |
| `GHOSTTY_CELL_WIDE_SPACER_HEAD` | soft-wrap spacer | treat as non-paint / document as unsupported edge |

**Graphemes** (render-state cell data):

- `GHOSTTY_RENDER_STATE_ROW_CELLS_DATA_GRAPHEMES_LEN` → `uint32_t`
- `GHOSTTY_RENDER_STATE_ROW_CELLS_DATA_GRAPHEMES_BUF` → `uint32_t*` codepoints (base first)
- `GHOSTTY_RENDER_STATE_ROW_CELLS_DATA_GRAPHEMES_UTF8` → UTF-8 into `GhosttyBuffer`
- Content tags include `GHOSTTY_CELL_CONTENT_CODEPOINT_GRAPHEME` for combining / multi-codepoint clusters

**Cursor** (`GhosttyRenderStateCursor` via `GHOSTTY_RENDER_STATE_DATA_CURSOR`):

```c
typedef struct {
  size_t size;
  bool viewport_has_value;
  uint16_t viewport_x;
  uint16_t viewport_y;
  bool wide_tail;
  bool visible;          /* mode-based visibility */
  bool blinking;
  bool password_input;
  GhosttyRenderStateCursorVisualStyle visual_style;
} GhosttyRenderStateCursor;
```

Also available on the live terminal via `ghostty_terminal_get` keys such as `GHOSTTY_TERMINAL_DATA_CURSOR_X/Y`, `GHOSTTY_TERMINAL_DATA_CURSOR_VISIBLE`, `GHOSTTY_TERMINAL_DATA_CURSOR_STYLE`. Prefer render-state cursor when building a paint snapshot so it matches the same frame as cells.

### Smooth scrolling

Smooth scrolling is **unavailable** on pin `b40acce58dcf77df52231c3798ea58e924647c89`.

`GhosttyRenderStateCursor.viewport_x` / `viewport_y` are the cursor cell, not a fractional scroll. `GhosttyTerminalScrollbar` (`GHOSTTY_TERMINAL_DATA_SCROLLBAR`) is whole rows: `total`, `offset`, and `len` are `uint64_t`. `GHOSTTY_TERMINAL_DATA_VIEWPORT_ACTIVE` is a bool.

`GHOSTTY_RENDER_STATE_OPTION_OVERSCAN` can capture extra rows above and below the viewport (`include/ghostty/vt/render.h`). The same header says the render state does not store the scroll position. The sample `offset_px` "comes from the renderer's own scroll animation" and is not a value Ghostty reports between frames. No `GHOSTTY_RENDER_STATE_DATA_*` key is a pixel offset.

Paint stays cell-aligned frame replacement. Do not blend screenshots. A TUI that scrolls by cells is ordinary frame replacement. The settled grid still has to fit the card. Wheel and Page Up/Down send `terminal.scroll` so Herdr moves the viewport; that is scrollback, not a pixel offset.

**Palette / defaults**: `GhosttyRenderStateColors` carries `background`, `foreground`, `cursor` (+ `cursor_has_value`), and `palette[256]`. Style colours with tag `PALETTE` resolve through that palette; `NONE` falls back to default fg/bg.

### Keyboard encoding (available)

Full Kitty-capable encoder is in tip (`include/ghostty/vt/key.h`, `key/encoder.h`, `key/event.h`):

```c
GHOSTTY_API GhosttyResult ghostty_key_encoder_new(
    const GhosttyAllocator *allocator, GhosttyKeyEncoder *encoder);
GHOSTTY_API void ghostty_key_encoder_free(GhosttyKeyEncoder encoder);
GHOSTTY_API void ghostty_key_encoder_setopt(
    GhosttyKeyEncoder encoder, GhosttyKeyEncoderOption option, const void *value);
GHOSTTY_API void ghostty_key_encoder_setopt_from_terminal(
    GhosttyKeyEncoder encoder, GhosttyTerminal terminal);
GHOSTTY_API GhosttyResult ghostty_key_encoder_encode(
    GhosttyKeyEncoder encoder, GhosttyKeyEvent event,
    char *out_buf, size_t out_buf_size, size_t *out_len);
GHOSTTY_API GhosttyResult ghostty_key_event_new(
    const GhosttyAllocator *allocator, GhosttyKeyEvent *event);
```

Not required for the read-only POC vertical slice; required later for interactive input. Sync encoder options from the terminal with `ghostty_key_encoder_setopt_from_terminal` after VT writes that change modes.

---

## Rust-facing mapping

| Rust method | C calls | Notes |
| --- | --- | --- |
| `TerminalEngine::new(cols, rows)` | `ghostty_terminal_new(NULL, &t, cols, rows)` + `ghostty_render_state_new` | Keep both handles in the Rust struct. Fail on non-`GHOSTTY_SUCCESS`. |
| `feed(bytes)` | `ghostty_terminal_vt_write(t, bytes.as_ptr(), bytes.len())` | Infallible at C layer. |
| `resize(cols, rows)` | `ghostty_terminal_resize(t, cols, rows, cell_w_px, cell_h_px)` | Supply pixel metrics from renderer or `1,1` until then. |
| `snapshot()` | `ghostty_render_state_update` → iterate rows/cells → copy grapheme UTF-8 / styles / occupancy / cursor into owned `TerminalSnapshot` | Copy all text before return; never retain C pointers. |
| (drop) | `ghostty_render_state_free` then `ghostty_terminal_free` | |

Colour mapping into foundation `Color`:

- `GHOSTTY_STYLE_COLOR_NONE` → default colour
- `GHOSTTY_STYLE_COLOR_PALETTE` → indexed
- `GHOSTTY_STYLE_COLOR_RGB` → RGB
- Apply `inverse` as reverse-video in the model (do not silently drop)

---

## Shim assessment

**C shim not required.** The C API already exposes create/feed/resize/destroy and a complete render-state iteration path. A Rust adapter with `bindgen` (or manually declared `extern "C"`) is enough.

A shim would only be justified if later work wants a single C function that fills a flat cell buffer; that is an optimization, not a blocker.

---

## Known limitations (document; do not fake)

| Area | Status at pinned tip |
| --- | --- |
| Colours | Default / palette / RGB + reverse (`inverse`) available. Underline colour, faint, blink, invisible, overline, strikethrough exposed in `GhosttyStyle`; POC may omit painting some flags but must not invent values. |
| Wide cells | Explicit wide / spacer-tail / spacer-head. Map leading + continuation; do not paint spacer tails independently. Soft-wrap `SPACER_HEAD` needs an explicit model policy. |
| Combining / graphemes | Multi-codepoint clusters via grapheme len/buf/UTF-8 APIs. Copy full cluster into cell text. |
| Cursor visibility | `visible`, `blinking`, viewport coords, `wide_tail`, visual style. Password-input flag present. |
| Keyboard encoding | Available (Kitty + legacy modes) via key encoder; **out of scope** until interaction wave. |
| Effects / PTY replies | Queries/bell/title/clipboard etc. need `ghostty_terminal_set` callbacks; ignored by default. Read-only observe may not need them. |
| Synchronized output | `GHOSTTY_TERMINAL_OPT_RENDER_HOLD` — optional for POC. |
| ABI stability | Unstable; pin hash; rebuild headers with the library. |
| Formatter API | Plain/VT/HTML dump only — **not** a high-performance paint path (upstream warning). |

---

## Build / link from Cargo `build.rs` (Zig)

### Install Zig

- Fedora package available: `zig-0.16.0-1.fc44.x86_64` (`dnf install zig`) — matches tip’s `minimum_zig_version`.
- Machine under discovery had **no** system `zig` on `PATH`; build was verified with the official `zig-x86_64-linux-0.16.0` tarball.

### Build only `libghostty-vt` (no GUI)

From a checkout of the pinned commit:

```bash
zig build -Demit-lib-vt -Doptimize=ReleaseFast --prefix "$PREFIX"
```

Verified command (ephemeral `/tmp` tree, not vendored):

```bash
export ZIG=/tmp/zig-x86_64-linux-0.16.0/zig   # or system zig 0.16.0
export ZIG_GLOBAL_CACHE_DIR=/tmp/zig-global-cache
cd /tmp/ghostty-tip   # git checkout b40acce58dcf77df52231c3798ea58e924647c89
$ZIG build -Demit-lib-vt -Doptimize=ReleaseFast --prefix /tmp/ghostty-vt-prefix
```

`-Demit-lib-vt` (see `src/build/Config.zig`) selects libghostty-vt-only defaults (skips macOS app / xcframework / docs). First build fetches Zig deps from `deps.files.ghostty.org` via `build.zig.zon` — **`build.rs` must either vendor those deps or fail with a clear offline diagnostic** (per `dev/03-ghostty-engine.md`).

### Produced artifacts (verified)

| Path | Role |
| --- | --- |
| `$PREFIX/include/ghostty/vt.h` (+ `ghostty/vt/*.h`) | Public C headers |
| `$PREFIX/lib/libghostty-vt.a` | Static library (~18 MiB ReleaseFast) |
| `$PREFIX/lib/libghostty-vt.so` → `libghostty-vt.so.0.1.0` | Shared library (~9.9 MiB) |
| `$PREFIX/share/pkgconfig/libghostty-vt.pc` | `Cflags: -I…/include`, `Libs: -L…/lib -lghostty-vt` |
| `$PREFIX/share/pkgconfig/libghostty-vt-static.pc` | Static: links `$PREFIX/lib/libghostty-vt.a` |

Recommended Cargo link for a reproducible binary: **static** (`libghostty-vt.a`) so runtime does not need `LD_LIBRARY_PATH`.

```text
cargo:rustc-link-search=native=$PREFIX/lib
cargo:rustc-link-lib=static=ghostty-vt
# plus any libc / compiler-rt needs Zig’s static archive already folds for typical Linux glibc hosts
```

Or via pkg-config: `PKG_CONFIG_PATH=$PREFIX/share/pkgconfig` + `libghostty-vt-static`.

Headers for bindgen: `$PREFIX/include` with `#include <ghostty/vt.h>`.

### `build.rs` expectations

1. Locate Zig `0.16.x` (`zig version`); error if missing/mismatched.
2. Build pinned Ghostty tree with `-Demit-lib-vt` into `OUT_DIR` (or use prebuilt prefix in CI).
3. Prefer offline vendored Ghostty + `build.zig.zon` deps; if network is required, print an actionable message.
4. Emit `cargo:rerun-if-changed` for the vendored pin and `build.rs`.
5. Do not expose C types outside `terminal/ghostty.rs`.

### Fedora packages relevant to this FFI path

| Package | Role |
| --- | --- |
| `zig` (≥ 0.16.0) | Build `libghostty-vt` |
| `gcc` / `clang` | Link Rust + static lib (usual Fedora toolchain) |
| `gtk4` / `libadwaita` | **Not** required to build or link `libghostty-vt` (VT lib is libc-only). Needed only for the GTK app shell (Agent C). |

---

## Function name cheat sheet

| Role | C symbol |
| --- | --- |
| create | `ghostty_terminal_new` |
| feed | `ghostty_terminal_vt_write` |
| resize | `ghostty_terminal_resize` |
| destroy | `ghostty_terminal_free` |
| iterate (preferred) | `ghostty_render_state_update` + `ghostty_render_state_row_iterator_*` + `ghostty_render_state_row_cells_*` |
| cell field (packed) | `ghostty_cell_get` / `ghostty_cell_get_multi` |
| keyboard | `ghostty_key_encoder_*` / `ghostty_key_event_*` |
