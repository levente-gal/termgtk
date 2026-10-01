# Validation — 2026-09-28 closeout

Evidence for [`dev/09-validation-docs.md`](../../dev/09-validation-docs.md). This pass did not change the renderer. Live checks below print only booleans and counts.

## Host

| Check | Result |
| --- | --- |
| `herdr --version` | `herdr 0.8.0` |
| `herdr api schema --json` SHA-256 | matches `88ff414aa996e390c2db05a37b95d28dbe4e81b98329f6ed7f7a2cc5c6ebf51a` |
| `herdr api snapshot` | `session_snapshot`; focused workspace, tab, and pane ids present; focused pane id found in `panes` (11 panes) |
| `rustc` / `cargo` | 1.98.1 |
| `zig version` | 0.16.0 at `/usr/bin/zig` |
| `pkg-config --modversion gtk4` / `libadwaita-1` | 4.22.5 / 1.9.4 |
| Ghostty pin file | `vendor/ghostty/PIN.txt` = `b40acce58dcf77df52231c3798ea58e924647c89` |
| `Cargo.lock` | present |
| Wayland | `WAYLAND_DISPLAY=wayland-1` |

Discovery recorded GTK 4.22.1 and libadwaita 1.9.0 before devel packages were installed. Schema hash is the same.

## Automated

```text
cargo test --manifest-path herdr-native-poc/Cargo.toml --features gtk,ghostty
```

57 tests passed, 0 failed (lib), 0 in the binary. That run covers redacted fixtures, protocol-gate mismatch, malformed NDJSON without payload text in the error, observe argv without `--takeover`, control argv with `--takeover` only when requested, sequence resync, resize state machine, key encoding, ANSI colour/style/cursor/wide cells, separator and panel detection, and the cases that must stay raw (short runs, intersections, mixed corners, overlaps, text that merely contains a box character).

## Acceptance matrix

| Area | Status | Evidence |
| --- | --- | --- |
| Backend | Pass | Live snapshot resolves a focused pane. Schema hash matches the committed file. No Herdr tree in this repo. |
| Stream | Partial | Operator confirmed a live pane paints and updates. Reconnect/resync and the single-slot frame queue are unit-tested. This closeout did not kill the observe process and watch a second connect. |
| Terminal | Partial | Ghostty tests cover ANSI colour forms used by the model, a combining cluster, wide cells, and cursor. Shell typing and arrow navigation are encoded in unit tests and wired after takeover. This closeout did not type into the live pane. |
| Resize | Partial | `ResizeSync` tests keep one in-flight size and ignore a non-matching frame. A live accepted/rejected resize pair was not recorded. |
| Native UI | Pass (code + prior run) | Card chrome is GSK outset shadow, 8px radius, border, and 12px padding (`ui/theme.rs`). Operator confirmed the window shows a Herdr pane. |
| Semantics | Pass (fixtures) | Detector tests: a qualifying `─`/`│` run becomes a separator; closed rectangles become panels; ambiguous, intersecting, overlapping, and unrecognized glyphs stay raw. No raw/enhanced screenshot was stored. |
| Failure paths | Pass (fixtures + messages) | Server-down, protocol mismatch, malformed record, missing/invalid frame bytes, and closed-target fixtures. UI banners exist for those states. Detector failure is “draw raw”, covered by the negative detector tests. Rejected resize is specified and unit-tested; not captured from a live Herdr stderr line in this pass. |
| Ownership | Partial | Takeover is a header button, not implicit. Release-on-close sends `terminal.release` and waits 300ms, then kills. Forced kill cannot guarantee detach (see stream contract). Live takeover and close were not scripted here. |
| Reproducibility | Pass | Pin file, `Cargo.lock`, Zig 0.16.0, schema hash, and redacted fixtures under `docs/contracts/`. |

## Demo procedure (what was and was not done)

1. Versions and schema hash recorded above. Ghostty commit is the pin file (the vendor tree has no `.git`).
2. Herdr server was already running with a focused pane. The application inside that pane was not changed and was not identified in this log.
3. Live window: the operator previously ran the app and saw a portion of the focused pane. This closeout did not start another GUI session.
4. Enhance toggle: not re-clicked in this pass. Fixture snapshot in `ShellState::fixture_snapshot` includes a separator and a box; detector tests cover on/off behavior at the model layer.
5. Text input, resize, reconnect, and orderly close: implemented and unit-tested; not exercised against the live control socket in this pass.
6. Fixture replay: the `cargo test` command above, with no display required.

## Reading

The hypothesis is supported for a single focused pane: Herdr stays the backend, the NDJSON stream is parsed by pinned libghostty-vt, and GTK paints cells inside native card chrome, with a conservative semantic overlay. It is not evidence for full terminal fidelity or for every TUI looking native.

## Fit the pane — 2026-09-29

Evidence for [`dev/10-fit-pane.md`](../../dev/10-fit-pane.md). Dimensions only; no frame payloads.

One launch of `herdr-native-poc` logged, in order: placeholder `80×24` while waiting; card capacity `240×49`; observe at `240×49`; first full frame `width=240` `height=49`. The process was then stopped. No controller was started on that launch.

A separate `control --takeover --cols 240 --rows 49` on the focused pane produced first full frame `240×49` (`seq: 1`). `terminal.resize` to `220×45` with `cell_width_px: 8` and `cell_height_px: 16` was followed by `terminal.frame` `seq: 2`, `full: true`, `width: 220`, `height: 45`. A resize back to `240×49` was followed by `seq: 3` at `240×49`. `terminal.release` exited 0. Stderr was empty on both positive resizes, so Herdr 0.8.0 did not reject them. Rejection of a non-positive size stays the unit test `resize_sync_keeps_one_inflight_and_confirmed_size`. The pane was left at `240×49`.
