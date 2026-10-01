# 04: Raw GTK Terminal Renderer

## Owner Paths

`herdr-native-poc/src/render/terminal_view.rs`,
`herdr-native-poc/src/render/cell_renderer.rs`, and
`herdr-native-poc/src/render/mod.rs` except the semantic detector.

## Dependencies

Neutral terminal model and GTK feasibility results. Use fixture snapshots until live integration exists.

## Work

1. Implement one focusable custom GTK4 `TerminalView`, not one widget per cell.
2. Accept and replace a `TerminalSnapshot`; queue redraws on update.
3. Determine JetBrains Mono metrics through Pango. Fall back to the default
   monospace family, retain measured advance/height, and reject a font whose
   reported advances are not stable for the supported grid glyphs.
4. Render cell backgrounds, compatible runs of text, the documented supported
   foreground/background colour forms and styles, Unicode, and a visible cursor
   through GTK snapshot/GSK APIs. Unsupported styles must have a documented,
   non-corrupting fallback.
5. Preserve all box-drawing glyphs in raw mode. Do not involve semantic detection in this initial implementation.
6. Expose measured logical cell size, widget scale factor, and calculated grid
   capacity for the resize owner. Snap graphical one-physical-pixel rules to the
   device-pixel grid and clip terminal content to the card viewport.

## Tests and Manual Check

- Unit-test grouping and logical-pixel-to-grid calculation, including minimum
  `1x1`, and device-pixel snapping at 100%, 125%, and 200% scale.
- Fixture demo shows ANSI colours, cursor, box glyphs, and Unicode.
- Resize the fixture window and verify no per-cell GTK children are created,
  glyph geometry remains stable, and content cannot draw outside the card.

## Handoff

Expose snapshot replacement, enhancement mode toggle plumbing, cell metrics, and focus support. Avoid dependency on Herdr or Ghostty.
