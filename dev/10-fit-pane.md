# 10: Fit the Pane to the Card

## Owner Paths

`herdr-native-poc/src/app.rs` session startup and `ResizeSync`,
`herdr-native-poc/src/render/terminal_view.rs` paint origin, and a short
evidence note in `herdr-native-poc/docs/validation.md`.

## Dependencies

Phase 09 is done. Stream facts in `docs/contracts/stream-contract.md`: observe
`--cols`/`--rows` sets that stream's `terminal.frame` `width`/`height` and does
not prove a PTY resize. A successful `terminal.resize` is visible only as a
later frame's `width`/`height`. A rejected line stays on stderr and the last
confirmed size stands.

## Gap

`session_worker` starts `ObserveSession` at 80×24 before `TerminalView` has
reported a capacity. `size_allocate` clips the widget and paints the snapshot
from the top-left, so a grid larger than the card shows only a corner. Capacity
changes while observing already respawn the observer; that path is not what the
first frame uses, and closeout never recorded a live accepted resize.

## Work

1. Keep 80×24 only as the pre-allocation placeholder. Do not spawn the first
   `herdr terminal session observe` until `TerminalView` has emitted a grid
   capacity from a positive allocation and ready font metrics. The 80ms debounce
   in `size_allocate` stays.
2. While read-only, a later capacity change keeps the existing respawn: drop
   the observer, bump the snapshot generation, and observe again at the new
   cols/rows. Log those cols/rows and the next full frame `width`/`height`.
   If they differ, surface a disconnect. Do not draw a frame that does not fit.
3. Paint only a snapshot whose cols/rows are ≤ the card capacity. A smaller
   snapshot (resize still in flight) is letterboxed, centered, at the measured
   cell size. Do not scale glyphs to fill leftover pixels. Do not clip a larger
   grid down to a partial corner.
4. After takeover, keep `ResizeSync`: one in-flight size, at most one newer
   target, apply on a matching frame, and on stderr rejection keep
   `confirmed`. The control process is spawned at the card capacity. Later
   changes send `terminal.resize` with `cols`, `rows`, `cell_width_px`, and
   `cell_height_px`. Do not respawn the controller to resize.
5. Record one live observe whose first full frame matches the card, one
   accepted control resize whose later frame matches, and the existing
   rejection unit test. If Herdr 0.8.0 will not reject a positive size on a
   live pane, write that down instead of inventing a rejection.

## Invariant

```text
painted snapshot cols × rows == last confirmed observe or controller size
and that size fits inside the card's cell capacity
```

## Gate Demo

- A desktop window shows the whole focused pane, including the bottom and right
  edges, with no clipped last row or column.
- Shrinking the window respawns observe (before takeover) or sends
  `terminal.resize` (after takeover), and the next used frame matches.
- Enhancement and takeover still behave as in phases 06 and 08.

## Out of Scope

Scrollback, `terminal.scroll`, mouse, and smooth pixel scrolling. Those belong
to phase 12.
