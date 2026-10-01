# 12: Tab Layout and Smooth Scrolling

## Owner Paths

Layout: `herdr-native-poc/src/herdr/snapshot.rs` for the layout model and
redacted layout fixtures,
`herdr-native-poc/src/ui/**` for the split of the card, and
`herdr-native-poc/src/app.rs` for one observer per visible pane.

Scrolling: `herdr-native-poc/docs/contracts/ghostty.md` first, then
`herdr-native-poc/src/terminal/**` and `render/terminal_view.rs` only if the
pinned Ghostty commit exposes a pixel offset.

## Dependencies

Phases 10 and 11. Layout facts are already in the redacted snapshot and in
schema `PaneLayoutSnapshot` (required: `workspace_id`, `tab_id`, `zoomed`,
`area`, `focused_pane_id`, `panes`, `splits`).

Each pane entry has `pane_id`, `focused`, and `rect` (`x`, `y`, `width`,
`height` in cells). Each split has `id`, `direction` (`right` or `down` only),
`ratio`, and `rect`.

Fixture examples: `w4:t1` is a `right` split `split_0_root` at ratio
`0.48795182` between `w4:p1` and `w4:p2`. `w1:t1` has one pane and `splits: []`.

## Part A — Focused-Tab Layout

1. Select the `layouts[]` object whose `workspace_id` and `tab_id` match the
   snapshot's focused workspace and focused tab. Ignore other workspaces' layouts.
2. Reconstruct split hierarchy from `splits[].rect` containment. A split whose
   rect equals `area` is the root; nested split rects belong to the smallest
   containing split. Use each split's `ratio` to allocate its two child regions.
   Use pane and child rects only to assign sides (`right` → left/right by `x`,
   `down` → top/bottom by `y`). Reject a layout with no unique root, ambiguous
   containment, or children that cannot occupy both sides; do not invent a
   partial layout. A single pane with empty `splits` keeps today's one-card path.
3. Run one `ObserveSession` per visible pane. Each pane's cols/rows come from
   phase 10's capacity math applied to that pane's share of the card. Stop and
   reap an observer when its pane leaves the layout or becomes hidden by zoom.
   Do not observe panes outside the focused tab.
4. When `zoomed` is true, observe and paint only `focused_pane_id` at the full
   card capacity. Confirm a live zoomed snapshot before treating `zoomed` as
   more than that flag.
5. Keyboard and takeover attach only to the pane Herdr marks focused. Other
   panes stay read-only. When `focused_pane_id` changes while controlling,
   disable input, release the old controller, then reconnect every visible pane
   read-only. Takeover requires another explicit action. Semantic enhancement
   runs per snapshot, unchanged.
6. Click-to-focus is blocked on the installed CLI. `herdr pane focus` only
   accepts `--direction left|right|up|down` (optional `--pane`). Socket method
   `pane.focus` takes a pane id, and this process still must not open a socket.
   Before adding a click handler, recheck `herdr pane --help` for a by-id
   focus command. If none exists, leave every card visible and keep input on
   the snapshot's focused pane.
7. Add redacted nested-split and zoomed fixtures. Tests: `w4:t1` maps to two
   panes split by `split_0_root`; nested fixture reconstructs parent/child
   splits; `w1:t1` maps to one pane; zoomed fixture maps to focused pane only;
   zoom transition reaps hidden observers; focused-pane change releases control.

## Part B — Smooth Scrolling

Do this only after Part A paints a real split.

1. Read the pinned Ghostty commit (`vendor/ghostty/PIN.txt`,
   `b40acce58dcf77df52231c3798ea58e924647c89`) for a render-state pixel offset
   or overscan-row API. `GhosttyRenderStateCursor.viewport_x` / `viewport_y` in
   `ghostty.md` are the cursor cell, not a fractional scroll.
2. If that API is absent, write the negative result into `ghostty.md` and stop.
   Do not blend screenshots. Do not send `terminal.scroll`. Cell-by-cell
   scrolling inside a TUI stays ordinary frame replacement.
3. If the API exists, extend the paint path with row grid + pixel offset +
   overscan rows, and animate only when that offset changes between frames
   from Ghostty. Phase 10's fit rule still holds: the settled grid matches the
   card.

## Invariant

```text
visible panes == zoom-filtered layouts[] entry for the focused tab
each painted grid fits its own card region
input and takeover target only the Herdr-focused pane
```

## Gate Demo

- A focused tab with two panes shows two cards whose split follows the snapshot
  ratio, each updating from its own observe stream.
- A one-pane tab still looks like phase 10.
- Either a short Ghostty note that smooth scrolling is unavailable on the pin,
  or a pixel offset that moves with the real API and settles on the fitted grid.

## Out of Scope

Drag-to-resize splits, `layout.apply`, `pane.split`, tabs as chrome, scrollback,
mouse, Kitty graphics, and a Herdr socket client.
