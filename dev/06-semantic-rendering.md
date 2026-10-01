# 06: Semantic Enhancement

## Owner Paths

`herdr-native-poc/src/render/overlay/` (detector, chrome, scrollbar, selection, options bar, and GTK paint). The public entry is `render::detect`.

## Dependencies

Neutral model plus renderer geometry contract. This work must be pure/fixture-driven first.

## Rules

- Enhancement is an overlay. It must preserve grid geometry and text.
- A failed or uncertain recognition renders raw cells.
- Detection returns primitives plus cells to suppress only when their graphical equivalent will be drawn.
- Intersections and ambiguous structures fall back to raw rendering.
- Suppression is a set-based mask. A cell is suppressed only after one selected
  primitive covers it; overlapping candidates must never independently remove it.

## Work

1. Define `Separator` and `PanelRect` primitives in terminal-cell coordinates.
2. Recognize horizontal and vertical box-drawing runs using the required Unicode
   characters from the POC. Specify the minimum run length, compatible style and
   background requirements, and exclusions for text/scrollbar-like content.
3. Detect one closed rectangle per frame. Corners are one family (`┌┐└┘` or
   `╭╮╰╯`). The top edge may contain one title run (spaces beside the title
   belong to it; a space alone, or two runs, rejects), or one or more
   `┤label├` bubble/keycap segments amid `─` (superfile-style). The bottom edge
   may contain one footer run or the same `┤label├` keycap segments. The left
   edge is `│`. The right edge is `│`, or one strict-subset thumb run
   (`┃█▐▌▓`, or `│` whose foreground differs from the top-left corner). A footer
   matching `N of M` and that thumb become a scrollbar. The title is never
   suppressed.
4. Rank panels ahead of separators. Suppress only `frame_suppress_cells` for a
   selected panel (perimeter box glyphs, a parsed footer, and a recognized
   thumb). Do not also keep a `─`/`│` separator on that perimeter.
5. Render separators as aligned physical-pixel lines. Render panels as cards
   with radius 8, a 1px border (2px when one panel's border color has strictly
   higher chroma), a header rule under a title, and a scrollbar capsule when
   `total > 1` or a partial thumb exists.
6. When adjacent panels share an edge, or have one row or column between them,
   zero the facing borders and corner radii, drop the shadow, and draw one
   hairline on the shared boundary.
7. A rare full-width interior background, whose first glyph is not `+` or `-`,
   is a selection pill inset by a quarter cell. The last row outside every
   panel, split on `|` into `label: key` segments, draws keycaps. Both stay
   empty when the pattern does not hold.

Still raw: crossing junctions used as structure (`┬┴┼`, a lone `├`/`┤` that is
not part of a `┤label├` keycap), mixed corner families, two title runs, a broken
edge, heavy and double box glyphs, and any unrecognized cell.
8. In debug mode, expose translucent primitive bounds/highlights and counters.

## Tests

- Positive tests for separators, plain boxes, titled frames, bubble `┤label├`
  titles, keycap footers, `N of M` footers, and thumbs.
- Negative tests for broken edges, intersections, incompatible corners,
  two title runs, short runs, overlaps, nested boxes, and text that merely
  contains a box character.
- A lazygit-like frame proves the title stays visible, the footer and thumb are
  suppressed, the card radius is 8, a scrollbar is laid out, and the top dashes
  are not also a separator.
- A suppression test proves all non-recognized cells remain raw.
- Geometry tests map grid coordinates to renderer pixel bounds exactly.

## Handoff

Renderer integration must be a toggleable pass executed after the snapshot is obtained and before raw glyph painting.
