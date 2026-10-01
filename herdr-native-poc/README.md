# Herdr Native Renderer POC

Linux proof of concept: one focused Herdr pane, drawn by this process as a GTK4 window. Herdr keeps the PTY and the session. This app does not modify Herdr or the program inside the pane.

The question under test:

> Can an unmodified TUI be progressively rendered as something that looks like a native application?

**Result so far:** yes for the narrow slice this POC implements. A live Herdr pane is selected, parsed with pinned `libghostty-vt`, and painted as cells inside a graphical card (radius, border, shadow, padding). Box-drawing runs that meet the detector rules can be replaced with lines and panel chrome. Takeover, a small key set, debounced resize, and release-on-close are implemented. This is not a terminal emulator and not a Herdr replacement. See [Limitations](#limitations) and [`docs/validation.md`](docs/validation.md).

## Architecture

```text
Herdr server
    │  persistent pane (unchanged)
    ▼
herdr terminal session observe | control
    │  NDJSON: terminal.frame / terminal.closed
    ▼
herdr/**          decode, redact logs, input, resize, release
    │  TerminalEvent
    ▼
terminal/**       libghostty-vt → TerminalSnapshot
    │
    ▼
render/**         optional semantic overlay, then cell paint
    │
    ▼
GTK4 / libadwaita Wayland window
```

| Layer | Owns |
| --- | --- |
| `herdr/**` | Herdr command lines and NDJSON only |
| `terminal/ghostty.rs` | Ghostty C API only |
| `terminal/model.rs` | Renderer-neutral cell snapshot |
| `render/**` | `TerminalSnapshot` plus an optional semantic pass |
| `app.rs` | Window lifecycle, takeover, resize, release |

## Fedora / Wayland

Target: Fedora, Wayland (Niri-compatible), Rust, GTK4, libadwaita, Herdr already installed, Zig 0.16.0 to build `libghostty-vt`.

Runtime and devel packages used to compile the GTK shell:

```bash
sudo dnf install \
  gtk4-devel libadwaita-devel pango-devel glib2-devel \
  cairo-devel cairo-gobject-devel graphene-devel \
  gobject-introspection-devel pkgconf-pkg-config \
  zig gcc
```

`zig` must be **0.16.0**. The Ghostty pin will not build on 0.15.x.

Closeout host (2026-09-28):

| Component | Version |
| --- | --- |
| Herdr | 0.8.0, protocol 19 |
| Herdr schema SHA-256 | `88ff414aa996e390c2db05a37b95d28dbe4e81b98329f6ed7f7a2cc5c6ebf51a` |
| Ghostty | commit `b40acce58dcf77df52231c3798ea58e924647c89` (MIT), checked out as the `vendor/ghostty` git submodule. Stable v1.3.1 has no terminal C API |
| Zig | 0.16.0 (`/usr/bin/zig`) |
| Rust / Cargo | 1.98.1 |
| GTK / libadwaita (pkg-config) | 4.22.5 / 1.9.4 |

Discovery captured GTK 4.22.1 and libadwaita 1.9.0 before the `-devel` packages were installed. The schema hash is unchanged. Contract files live in [`docs/contracts/`](docs/contracts/).

## Clone

`vendor/ghostty` is a git submodule pinned to Ghostty commit `b40acce58dcf77df52231c3798ea58e924647c89`.

```bash
git clone --recurse-submodules https://github.com/levente-gal/termgtk.git
```

If the repo was cloned without submodules:

```bash
git submodule update --init --recursive
```

## Build and run

From the repository root, with `herdr server` already running and a pane focused:

```bash
cargo run --manifest-path herdr-native-poc/Cargo.toml --features gtk,ghostty
```

Offline tests (no display, no live pane):

```bash
cargo test --manifest-path herdr-native-poc/Cargo.toml --features gtk,ghostty
```

Logs: `RUST_LOG=info` (default filter is `info`). Frame payloads are not logged.

Debug overlay (pane id, grid, cell size, scale, frame count, detection count — no ANSI bytes):

```bash
HERDR_NATIVE_DEBUG=1 cargo run --manifest-path herdr-native-poc/Cargo.toml --features gtk,ghostty
```

## Focused pane

On startup the app runs `herdr api snapshot`, checks Herdr **0.8.0** and the schema hash above, and follows:

- `result.snapshot.focused_workspace_id`
- `result.snapshot.focused_tab_id`
- `result.snapshot.focused_pane_id`

The header shows the workspace label and the pane title. It then starts a **read-only** mirror:

```text
herdr terminal session observe <pane> --cols N --rows N
```

The first `terminal.frame` is `full: true` and replaces the grid. Later frames with a higher `seq` are incremental. A backwards `seq` drops that record and resyncs from a new full frame. The GTK side keeps one pending frame, so a burst cannot grow an unbounded queue. Reconnect starts a new observe process; `seq` restarts at 1 and the previous frame is not kept.

## Take over and release

The window starts read-only. **Take over** in the header is the only path into control:

```text
herdr terminal session control <pane> --takeover --cols N --rows N
```

Keys go to that process only after takeover. Closing the window writes `{"type":"terminal.release"}`, flushes it, and waits up to 300ms. If the child is still alive, the process is killed and a warning is logged. `SIGKILL` before the release line is flushed does not detach; Herdr detaches on `terminal.release` or stdin EOF.

There is no resize or input acknowledgement record. A resize counts as accepted only when a later `terminal.frame` reports the same `width` and `height`. A rejected line stays on Herdr stderr (`terminal session control input ignored`); the last confirmed size is kept. At most one resize is in flight; a newer size waits until that one settles. Allocation changes are debounced by 80ms before a resize is sent.

## Enhancement

**Enhance** in the header, or `Ctrl+Shift+E`, toggles semantic overlay without reconnecting. Off: every glyph is drawn as text. On, the grid stays put and recognized chrome is redrawn:

- A horizontal or vertical run of `─` / `│` of length ≥ 3 becomes a 1px line.
- A closed light or rounded rectangle (`┌┐└┘` or `╭╮╰╯`) becomes a card with radius 8. The top edge may hold one title run, and the bottom edge one footer run. The title stays as text. A footer that matches `N of M` is hidden when it parses. A right-edge thumb (`┃`, `█`, `▐`, `▌`, `▓`, or a `│` whose color differs from the corner) that does not cover the whole side becomes a scrollbar capsule. `0 of 0` and `1 of 1` do not draw a thumb.
- A full-width interior row whose background is visible, is not the most common interior background, and whose first glyph is not `+` or `-`, becomes a rounded selection pill.
- The last row, when it sits outside every panel and splits into `label: key` segments on `|`, draws those key tokens as keycaps.
- Neighbors that share a width (stacked) or a height (side by side), including a one-cell gap, share a hairline seam instead of two borders and a shadow.

Crossings (`├┤┬┴┼`), mixed corner families, two title runs on one edge, a space that breaks an edge, and heavy or double box glyphs stay raw. A cell is hidden only when the primitive that replaces it is actually drawn.

`Ctrl+Shift+E` is an application shortcut. It is not forwarded to the terminal.

## Fidelity

Painted:

| Feature | Behavior |
| --- | --- |
| Colour | Default, indexed (0–255), RGB |
| Style | Bold, italic, underline, reverse video |
| Unicode | Grapheme clusters, including combining marks, as one cell |
| Wide glyphs | Leading cell painted; continuation cell not painted again |
| Cursor | Cell highlight; wide-tail cursor highlights the leading cell |

Present in Ghostty and **not** painted (never invented): faint, blink, invisible, strikethrough, overline, underline colour, cursor shape and blink, Kitty graphics, selection, scrollback outside the viewport, synchronized-output holds.

### Keys after takeover

| Input | Wire |
| --- | --- |
| Printable characters | `{"type":"terminal.input","text":"…"}` |
| Enter | `\r` |
| Backspace | `0x7f` |
| Tab | `\t` |
| Escape | `0x1b` |
| Arrows | `\x1b[A` `\x1b[B` `\x1b[C` `\x1b[D` |
| Ctrl+C / D / L / R | `0x03` `0x04` `0x0c` `0x12` |

Not sent: Alt, Ctrl+Shift (including the enhance shortcut), Shift+Tab, mouse, IME, and every other Ctrl letter.

## Stream, schema, redaction

Protocol facts: [`docs/contracts/stream-contract.md`](docs/contracts/stream-contract.md). Schema: [`docs/contracts/herdr-api.schema.json`](docs/contracts/herdr-api.schema.json).

Fixtures under `docs/contracts/fixtures/` and `observe.ndjson` / `snapshot.json` replace home paths, usernames, hostnames, titles, and real ANSI payloads. Do not paste `terminal.frame` `bytes` into logs or docs.

## Limitations

Out of scope, on purpose:

- Full Herdr workspace UI, tabs, and pane management
- A complete terminal emulator (mouse, selection, scrollback, Kitty keyboard/graphics, images, OSC replies)
- Semantic detection beyond simple separators and closed rectangles
- Modifying Herdr or the TUI under test
- Tracking Ghostty `tip` by name; the pin is the commit above

What this run does **not** claim: that every TUI looks native, or that rejected resizes and forced kills were captured on a live pane during closeout. Fixture coverage and the live checks that were run are in [`docs/validation.md`](docs/validation.md). A raw-versus-enhanced screenshot was not saved; the live pane can contain session content, and the detector difference is covered by fixture tests instead.
