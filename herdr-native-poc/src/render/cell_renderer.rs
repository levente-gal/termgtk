//! Cell grouping, colour resolution, and grid math. GTK drawing stays behind the `gtk` feature.

use crate::terminal::{Cell, CellOccupancy, Color, Style, TerminalSnapshot};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Run<'a> {
    pub cells: &'a [Cell],
}

/// Near-white default foreground so content remains readable on the card.
#[cfg_attr(not(any(test, feature = "gtk")), allow(dead_code))]
pub const DEFAULT_FG: (u8, u8, u8) = (0xe8, 0xe8, 0xea);
/// Solid stand-in used only when reverse video needs an opaque default background.
#[cfg_attr(not(any(test, feature = "gtk")), allow(dead_code))]
const DEFAULT_BG_SOLID: (u8, u8, u8) = (0x12, 0x12, 0x14);

/// Group consecutive same-row cells that share fg/bg/style into text runs.
///
/// [`CellOccupancy::WideContinuation`] cells are omitted: they stay in the
/// snapshot for geometry, but they are not their own text run and are not
/// painted as glyphs.
pub fn group_runs(snapshot: &TerminalSnapshot) -> Vec<Run<'_>> {
    group_compatible(snapshot, true)
}

/// Same attribute/row grouping as [`group_runs`], including wide continuations
/// so backgrounds cover the full wide-glyph span.
#[cfg_attr(not(any(test, feature = "gtk")), allow(dead_code))]
pub fn group_background_runs(snapshot: &TerminalSnapshot) -> Vec<Run<'_>> {
    group_compatible(snapshot, false)
}

fn group_compatible(snapshot: &TerminalSnapshot, skip_wide_continuation: bool) -> Vec<Run<'_>> {
    let cells = snapshot.cells();
    let mut runs = Vec::new();
    let mut start = 0usize;
    let mut i = 0usize;
    while i < cells.len() {
        if skip_wide_continuation && cells[i].occupancy == CellOccupancy::WideContinuation {
            if start < i {
                runs.push(Run {
                    cells: &cells[start..i],
                });
            }
            i += 1;
            start = i;
            continue;
        }
        if i > start {
            let prev = &cells[i - 1];
            let cur = &cells[i];
            if cur.y != prev.y || !same_paint_attrs(prev, cur) {
                runs.push(Run {
                    cells: &cells[start..i],
                });
                start = i;
            }
        }
        i += 1;
    }
    if start < cells.len() {
        runs.push(Run {
            cells: &cells[start..],
        });
    }
    runs
}

fn same_paint_attrs(a: &Cell, b: &Cell) -> bool {
    a.fg == b.fg && a.bg == b.bg && a.style == b.style
}

/// Logical pixels to a grid of at least 1x1, snapped to the device pixel.
pub fn device_cols_rows(
    width_px: f64,
    height_px: f64,
    cell_w: f64,
    cell_h: f64,
    scale: f64,
) -> (u16, u16) {
    let scale = if scale.is_finite() && scale > 0.0 {
        scale
    } else {
        1.0
    };
    let snap = |value: f64, cell: f64| -> u16 {
        if !cell.is_finite() || cell <= 0.0 || !value.is_finite() || value <= 0.0 {
            return 1;
        }
        let logical = (value * scale).round() / scale;
        (logical / cell).floor().max(1.0) as u16
    };
    (snap(width_px, cell_w), snap(height_px, cell_h))
}

fn finite_scale(scale: f64) -> f64 {
    if scale.is_finite() && scale > 0.0 {
        scale
    } else {
        1.0
    }
}

/// Snap a logical cell size onto whole device pixels.
///
/// Painting uses this size so adjacent row rects share an edge instead of
/// leaving a gap that shows the card underneath.
pub fn snap_cell_px(logical: f64, scale: f64) -> f64 {
    let scale = finite_scale(scale);
    if !logical.is_finite() || logical <= 0.0 {
        return 1.0 / scale;
    }
    (logical * scale).round().max(1.0) / scale
}

/// Device-pixel edges of an axis-aligned rect. Width and height stay at least
/// one device pixel so a hairline does not round away.
#[cfg_attr(not(any(test, feature = "gtk")), allow(dead_code))]
pub fn snap_rect(x: f64, y: f64, w: f64, h: f64, scale: f64) -> (f64, f64, f64, f64) {
    let scale = finite_scale(scale);
    let left = (x * scale).round() / scale;
    let top = (y * scale).round() / scale;
    let right = ((x + w) * scale).round() / scale;
    let bottom = ((y + h) * scale).round() / scale;
    let width = (right - left).max(1.0 / scale);
    let height = (bottom - top).max(1.0 / scale);
    (left, top, width, height)
}

/// One cell-aligned rectangle in logical pixels.
#[cfg_attr(not(any(test, feature = "gtk")), allow(dead_code))]
pub fn cell_band(x: u16, y: u16, cols: u16, cell_w: f64, cell_h: f64) -> (f64, f64, f64, f64) {
    (
        f64::from(x) * cell_w,
        f64::from(y) * cell_h,
        f64::from(cols.max(1)) * cell_w,
        cell_h,
    )
}

/// Most common opaque painted background, or `None` when every cell is transparent.
///
/// Ties keep the colour that appears first in row-major order.
#[cfg_attr(not(any(test, feature = "gtk")), allow(dead_code))]
pub fn dominant_opaque_bg(snapshot: &TerminalSnapshot) -> Option<Rgba> {
    let mut counts: Vec<(Rgba, usize, usize)> = Vec::new();
    for (index, cell) in snapshot.cells().iter().enumerate() {
        let (_, bg) = paint_colors(cell.fg, cell.bg, cell.style);
        if !bg.is_visible() {
            continue;
        }
        if let Some(slot) = counts.iter_mut().find(|slot| slot.0 == bg) {
            slot.1 += 1;
        } else {
            counts.push((bg, 1, index));
        }
    }
    counts
        .into_iter()
        .max_by(|a, b| a.1.cmp(&b.1).then(b.2.cmp(&a.2)))
        .map(|slot| slot.0)
}

/// Foreground used for an enhanced panel border: the top-left perimeter cell.
#[cfg_attr(not(any(test, feature = "gtk")), allow(dead_code))]
pub fn panel_border_rgba(snapshot: &TerminalSnapshot, x: u16, y: u16) -> Rgba {
    match snapshot.cell(x, y) {
        Some(cell) => resolve_fg(cell.fg),
        None => Rgba::opaque(DEFAULT_FG),
    }
}

#[cfg_attr(not(any(test, feature = "gtk")), allow(dead_code))]
const ARM_LEFT: u8 = 1;
#[cfg_attr(not(any(test, feature = "gtk")), allow(dead_code))]
const ARM_RIGHT: u8 = 2;
#[cfg_attr(not(any(test, feature = "gtk")), allow(dead_code))]
const ARM_UP: u8 = 4;
#[cfg_attr(not(any(test, feature = "gtk")), allow(dead_code))]
const ARM_DOWN: u8 = 8;

#[cfg_attr(not(any(test, feature = "gtk")), allow(dead_code))]
#[derive(Clone, Copy, PartialEq, Eq)]
enum BoxWeight {
    Light,
    Heavy,
    Double,
}

/// Axis-aligned stroke inside one cell, origin at the cell's top-left.
#[cfg_attr(not(any(test, feature = "gtk")), allow(dead_code))]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BoxStroke {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

#[cfg_attr(not(any(test, feature = "gtk")), allow(dead_code))]
fn box_kind(grapheme: &str) -> Option<(BoxWeight, u8)> {
    Some(match grapheme {
        "─" => (BoxWeight::Light, ARM_LEFT | ARM_RIGHT),
        "│" => (BoxWeight::Light, ARM_UP | ARM_DOWN),
        "┌" => (BoxWeight::Light, ARM_RIGHT | ARM_DOWN),
        "┐" => (BoxWeight::Light, ARM_LEFT | ARM_DOWN),
        "└" => (BoxWeight::Light, ARM_RIGHT | ARM_UP),
        "┘" => (BoxWeight::Light, ARM_LEFT | ARM_UP),
        "├" => (BoxWeight::Light, ARM_RIGHT | ARM_UP | ARM_DOWN),
        "┤" => (BoxWeight::Light, ARM_LEFT | ARM_UP | ARM_DOWN),
        "┬" => (BoxWeight::Light, ARM_LEFT | ARM_RIGHT | ARM_DOWN),
        "┴" => (BoxWeight::Light, ARM_LEFT | ARM_RIGHT | ARM_UP),
        "┼" => (BoxWeight::Light, ARM_LEFT | ARM_RIGHT | ARM_UP | ARM_DOWN),
        "╭" => (BoxWeight::Light, ARM_RIGHT | ARM_DOWN),
        "╮" => (BoxWeight::Light, ARM_LEFT | ARM_DOWN),
        "╯" => (BoxWeight::Light, ARM_LEFT | ARM_UP),
        "╰" => (BoxWeight::Light, ARM_RIGHT | ARM_UP),
        "━" => (BoxWeight::Heavy, ARM_LEFT | ARM_RIGHT),
        "┃" => (BoxWeight::Heavy, ARM_UP | ARM_DOWN),
        "┏" => (BoxWeight::Heavy, ARM_RIGHT | ARM_DOWN),
        "┓" => (BoxWeight::Heavy, ARM_LEFT | ARM_DOWN),
        "┗" => (BoxWeight::Heavy, ARM_RIGHT | ARM_UP),
        "┛" => (BoxWeight::Heavy, ARM_LEFT | ARM_UP),
        "┣" => (BoxWeight::Heavy, ARM_RIGHT | ARM_UP | ARM_DOWN),
        "┫" => (BoxWeight::Heavy, ARM_LEFT | ARM_UP | ARM_DOWN),
        "┳" => (BoxWeight::Heavy, ARM_LEFT | ARM_RIGHT | ARM_DOWN),
        "┻" => (BoxWeight::Heavy, ARM_LEFT | ARM_RIGHT | ARM_UP),
        "╋" => (BoxWeight::Heavy, ARM_LEFT | ARM_RIGHT | ARM_UP | ARM_DOWN),
        "═" => (BoxWeight::Double, ARM_LEFT | ARM_RIGHT),
        "║" => (BoxWeight::Double, ARM_UP | ARM_DOWN),
        "╔" => (BoxWeight::Double, ARM_RIGHT | ARM_DOWN),
        "╗" => (BoxWeight::Double, ARM_LEFT | ARM_DOWN),
        "╚" => (BoxWeight::Double, ARM_RIGHT | ARM_UP),
        "╝" => (BoxWeight::Double, ARM_LEFT | ARM_UP),
        "╠" => (BoxWeight::Double, ARM_RIGHT | ARM_UP | ARM_DOWN),
        "╣" => (BoxWeight::Double, ARM_LEFT | ARM_UP | ARM_DOWN),
        "╦" => (BoxWeight::Double, ARM_LEFT | ARM_RIGHT | ARM_DOWN),
        "╩" => (BoxWeight::Double, ARM_LEFT | ARM_RIGHT | ARM_UP),
        "╬" => (BoxWeight::Double, ARM_LEFT | ARM_RIGHT | ARM_UP | ARM_DOWN),
        _ => return None,
    })
}

#[cfg_attr(not(any(test, feature = "gtk")), allow(dead_code))]
fn push_clamped(
    out: &mut Vec<BoxStroke>,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    cell_w: f64,
    cell_h: f64,
) {
    let x1 = x.max(0.0);
    let y1 = y.max(0.0);
    let x2 = (x + w).min(cell_w).max(x1);
    let y2 = (y + h).min(cell_h).max(y1);
    if x2 > x1 && y2 > y1 {
        out.push(BoxStroke {
            x: x1,
            y: y1,
            width: x2 - x1,
            height: y2 - y1,
        });
    }
}

#[cfg_attr(not(any(test, feature = "gtk")), allow(dead_code))]
fn push_arms(
    out: &mut Vec<BoxStroke>,
    cell_w: f64,
    cell_h: f64,
    thickness: f64,
    x_shift: f64,
    y_shift: f64,
    arms: u8,
) {
    let cx = cell_w * 0.5 + x_shift;
    let cy = cell_h * 0.5 + y_shift;
    let x0 = cx - thickness * 0.5;
    let y0 = cy - thickness * 0.5;
    let left = arms & ARM_LEFT != 0;
    let right = arms & ARM_RIGHT != 0;
    let up = arms & ARM_UP != 0;
    let down = arms & ARM_DOWN != 0;
    if left || right {
        let (x, w) = if left && right {
            (0.0, cell_w)
        } else if right {
            (x0, cell_w - x0)
        } else {
            (0.0, x0 + thickness)
        };
        push_clamped(out, x, y0, w, thickness, cell_w, cell_h);
    }
    if up || down {
        let (y, h) = if up && down {
            (0.0, cell_h)
        } else if down {
            (y0, cell_h - y0)
        } else {
            (0.0, y0 + thickness)
        };
        push_clamped(out, x0, y, thickness, h, cell_w, cell_h);
    }
}

/// Edge-to-edge strokes for a single box-drawing grapheme.
///
/// `None` for text. A box character inside a word is still one cell, so it
/// strokes on its own and is not deleted as a separator.
#[cfg_attr(not(any(test, feature = "gtk")), allow(dead_code))]
pub fn box_cell_strokes(
    grapheme: &str,
    cell_w: f64,
    cell_h: f64,
    scale: f64,
) -> Option<Vec<BoxStroke>> {
    let (weight, arms) = box_kind(grapheme)?;
    if !cell_w.is_finite() || cell_w <= 0.0 || !cell_h.is_finite() || cell_h <= 0.0 {
        return None;
    }
    let scale = finite_scale(scale);
    let thin = 1.0 / scale;
    let mut raw = Vec::new();
    match weight {
        BoxWeight::Light => push_arms(&mut raw, cell_w, cell_h, thin, 0.0, 0.0, arms),
        BoxWeight::Heavy => push_arms(&mut raw, cell_w, cell_h, thin * 2.0, 0.0, 0.0, arms),
        BoxWeight::Double => {
            push_arms(&mut raw, cell_w, cell_h, thin, -thin, -thin, arms);
            push_arms(&mut raw, cell_w, cell_h, thin, thin, thin, arms);
        }
    }
    let strokes = raw
        .into_iter()
        .map(|stroke| {
            let (x, y, width, height) =
                snap_rect(stroke.x, stroke.y, stroke.width, stroke.height, scale);
            BoxStroke {
                x,
                y,
                width,
                height,
            }
        })
        .collect();
    Some(strokes)
}

/// Resolved sRGB colour with alpha. Default background is transparent so the
/// card chrome shows through.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(not(any(test, feature = "gtk")), allow(dead_code))]
pub struct Rgba {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

#[cfg_attr(not(any(test, feature = "gtk")), allow(dead_code))]
impl Rgba {
    pub const TRANSPARENT: Self = Self {
        r: 0,
        g: 0,
        b: 0,
        a: 0,
    };

    pub fn opaque(rgb: (u8, u8, u8)) -> Self {
        Self {
            r: rgb.0,
            g: rgb.1,
            b: rgb.2,
            a: 255,
        }
    }

    pub fn is_visible(self) -> bool {
        self.a > 0
    }
}

/// Resolve a model colour as foreground (Default → near white, opaque).
#[cfg_attr(not(any(test, feature = "gtk")), allow(dead_code))]
pub fn resolve_fg(color: Color) -> Rgba {
    match color {
        Color::Default => Rgba::opaque(DEFAULT_FG),
        Color::Indexed(index) => Rgba::opaque(indexed_rgb(index)),
        Color::Rgb(r, g, b) => Rgba::opaque((r, g, b)),
    }
}

/// Resolve a model colour as background (Default → transparent).
#[cfg_attr(not(any(test, feature = "gtk")), allow(dead_code))]
pub fn resolve_bg(color: Color) -> Rgba {
    match color {
        Color::Default => Rgba::TRANSPARENT,
        Color::Indexed(index) => Rgba::opaque(indexed_rgb(index)),
        Color::Rgb(r, g, b) => Rgba::opaque((r, g, b)),
    }
}

/// Foreground/background pair for painting, applying reverse video by swapping.
#[cfg_attr(not(any(test, feature = "gtk")), allow(dead_code))]
pub fn paint_colors(fg: Color, bg: Color, style: Style) -> (Rgba, Rgba) {
    let mut fg_rgba = resolve_fg(fg);
    let mut bg_rgba = resolve_bg(bg);
    if style.reverse {
        std::mem::swap(&mut fg_rgba, &mut bg_rgba);
        if fg_rgba.a == 0 {
            fg_rgba = Rgba::opaque(DEFAULT_BG_SOLID);
        }
        if bg_rgba.a == 0 {
            bg_rgba = Rgba::opaque(DEFAULT_FG);
        }
    }
    (fg_rgba, bg_rgba)
}

/// Concatenate graphemes in a text run, skipping wide continuations and never
/// dropping unknown glyphs from leading/single cells.
#[cfg_attr(not(any(test, feature = "gtk")), allow(dead_code))]
pub fn run_text(run: &Run<'_>) -> String {
    let mut out = String::new();
    for cell in run.cells {
        if cell.occupancy == CellOccupancy::WideContinuation {
            continue;
        }
        out.push_str(&cell.grapheme);
    }
    out
}

/// True when every paintable cell in the run is covered by one selected primitive.
#[cfg_attr(not(any(test, feature = "gtk")), allow(dead_code))]
pub fn glyphs_fully_suppressed(
    run: &Run<'_>,
    suppress: &std::collections::HashSet<(u16, u16)>,
) -> bool {
    let mut saw_glyph = false;
    for cell in run.cells {
        if cell.occupancy == CellOccupancy::WideContinuation {
            continue;
        }
        saw_glyph = true;
        if !suppress.contains(&(cell.x, cell.y)) {
            return false;
        }
    }
    saw_glyph
}

#[cfg_attr(not(any(test, feature = "gtk")), allow(dead_code))]
fn indexed_rgb(index: u8) -> (u8, u8, u8) {
    match index {
        0 => (0x00, 0x00, 0x00),
        1 => (0xcd, 0x00, 0x00),
        2 => (0x00, 0xcd, 0x00),
        3 => (0xcd, 0xcd, 0x00),
        4 => (0x00, 0x00, 0xee),
        5 => (0xcd, 0x00, 0xcd),
        6 => (0x00, 0xcd, 0xcd),
        7 => (0xe5, 0xe5, 0xe5),
        8 => (0x7f, 0x7f, 0x7f),
        9 => (0xff, 0x00, 0x00),
        10 => (0x00, 0xff, 0x00),
        11 => (0xff, 0xff, 0x00),
        12 => (0x5c, 0x5c, 0xff),
        13 => (0xff, 0x00, 0xff),
        14 => (0x00, 0xff, 0xff),
        15 => (0xff, 0xff, 0xff),
        16..=231 => {
            let n = index - 16;
            let r = n / 36;
            let g = (n % 36) / 6;
            let b = n % 6;
            (cube_component(r), cube_component(g), cube_component(b))
        }
        232..=255 => {
            let level = 8 + (u16::from(index) - 232) * 10;
            let v = level as u8;
            (v, v, v)
        }
    }
}

#[cfg_attr(not(any(test, feature = "gtk")), allow(dead_code))]
fn cube_component(value: u8) -> u8 {
    if value == 0 {
        0
    } else {
        55 + 40 * value
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::terminal::{Cell, CellOccupancy, Color, Cursor, Style, TerminalSnapshot};

    #[test]
    fn grid_capacity_clamps_and_snaps() {
        assert_eq!(device_cols_rows(0.0, 10.0, 8.0, 16.0, 1.0), (1, 1));
        assert_eq!(device_cols_rows(80.0, 32.0, 8.0, 16.0, 1.0), (10, 2));
        assert_eq!(device_cols_rows(100.0, 40.0, 10.0, 20.0, 1.25), (10, 2));
        assert_eq!(device_cols_rows(80.0, 40.0, 8.0, 20.0, 2.0), (10, 2));
    }

    #[test]
    fn blank_snapshot_groups_without_dropping_cells() {
        let snap = TerminalSnapshot::blank(2, 1).unwrap();
        let runs = group_runs(&snap);
        assert_eq!(runs.iter().map(|run| run.cells.len()).sum::<usize>(), 2);
        assert_eq!(runs.len(), 1);
    }

    #[test]
    fn group_runs_splits_on_style_and_row() {
        let snap = TerminalSnapshot::try_new(3, 2, Cursor::hidden(), {
            let mut grid = Vec::new();
            for y in 0..2u16 {
                for x in 0..3u16 {
                    let mut cell = Cell::blank(x, y);
                    cell.grapheme = format!("{x}{y}");
                    if y == 0 && x < 2 {
                        cell.fg = Color::Indexed(1);
                    } else if y == 0 {
                        cell.fg = Color::Indexed(2);
                    } else {
                        cell.fg = Color::Indexed(1);
                    }
                    grid.push(cell);
                }
            }
            grid
        })
        .unwrap();
        let runs = group_runs(&snap);
        // row0: two indexed(1) then one indexed(2); row1: three indexed(1)
        assert_eq!(runs.len(), 3);
        assert_eq!(runs[0].cells.len(), 2);
        assert_eq!(runs[1].cells.len(), 1);
        assert_eq!(runs[2].cells.len(), 3);
    }

    #[test]
    fn group_runs_skips_wide_continuation_as_text() {
        let cells = vec![
            Cell {
                grapheme: "你".into(),
                occupancy: CellOccupancy::WideLeading,
                fg: Color::Indexed(7),
                ..Cell::blank(0, 0)
            },
            Cell {
                occupancy: CellOccupancy::WideContinuation,
                fg: Color::Indexed(7),
                ..Cell::blank(1, 0)
            },
            Cell {
                grapheme: "x".into(),
                fg: Color::Indexed(7),
                ..Cell::blank(2, 0)
            },
        ];
        let snap = TerminalSnapshot::try_new(3, 1, Cursor::hidden(), cells).unwrap();
        let text_runs = group_runs(&snap);
        assert_eq!(text_runs.len(), 2);
        assert_eq!(text_runs[0].cells.len(), 1);
        assert_eq!(text_runs[0].cells[0].grapheme, "你");
        assert_eq!(run_text(&text_runs[0]), "你");
        assert_eq!(text_runs[1].cells[0].grapheme, "x");
        assert!(!text_runs.iter().any(|run| run
            .cells
            .iter()
            .any(|c| c.occupancy == CellOccupancy::WideContinuation)));

        let bg_runs = group_background_runs(&snap);
        assert_eq!(bg_runs.len(), 1);
        assert_eq!(bg_runs[0].cells.len(), 3);
        assert_eq!(
            bg_runs[0].cells[1].occupancy,
            CellOccupancy::WideContinuation
        );
    }

    #[test]
    fn indexed_and_default_colours_resolve() {
        assert_eq!(resolve_fg(Color::Default), Rgba::opaque(DEFAULT_FG));
        assert!(!resolve_bg(Color::Default).is_visible());
        assert_eq!(
            resolve_fg(Color::Indexed(1)),
            Rgba::opaque((0xcd, 0x00, 0x00))
        );
        assert_eq!(
            resolve_fg(Color::Indexed(196)),
            Rgba::opaque((0xff, 0x00, 0x00))
        );
        assert_eq!(resolve_fg(Color::Indexed(232)), Rgba::opaque((8, 8, 8)));
        assert_eq!(resolve_fg(Color::Rgb(1, 2, 3)), Rgba::opaque((1, 2, 3)));

        let (fg, bg) = paint_colors(
            Color::Indexed(2),
            Color::Default,
            Style {
                reverse: true,
                ..Style::default()
            },
        );
        assert!(bg.is_visible());
        assert_eq!(bg, Rgba::opaque((0x00, 0xcd, 0x00)));
        assert_eq!(fg, Rgba::opaque(DEFAULT_BG_SOLID));
    }

    #[test]
    fn run_text_keeps_unknown_glyphs() {
        let snap = TerminalSnapshot::try_new(
            2,
            1,
            Cursor::hidden(),
            vec![
                Cell {
                    grapheme: "┌".into(),
                    ..Cell::blank(0, 0)
                },
                Cell {
                    grapheme: "☃".into(),
                    ..Cell::blank(1, 0)
                },
            ],
        )
        .unwrap();
        let runs = group_runs(&snap);
        assert_eq!(run_text(&runs[0]), "┌☃");
    }

    #[test]
    fn suppression_hides_a_full_run_only() {
        let snap = TerminalSnapshot::blank(2, 1).unwrap();
        let runs = group_runs(&snap);
        let mut suppress = std::collections::HashSet::new();
        assert!(!glyphs_fully_suppressed(&runs[0], &suppress));
        suppress.insert((0, 0));
        assert!(!glyphs_fully_suppressed(&runs[0], &suppress));
        suppress.insert((1, 0));
        assert!(glyphs_fully_suppressed(&runs[0], &suppress));
    }

    fn device_px(logical: f64, scale: f64) -> i64 {
        (logical * scale).round() as i64
    }

    #[test]
    fn snapped_rows_share_an_edge_at_scale_1_and_2() {
        for scale in [1.0, 2.0] {
            let cell_w = snap_cell_px(7.3, scale);
            let cell_h = snap_cell_px(16.4, scale);
            let (_x0, y0, _w0, h0) = snap_rect(0.0, 0.0, cell_w * 4.0, cell_h, scale);
            let (_x1, y1, _w1, _h1) = snap_rect(0.0, cell_h, cell_w * 4.0, cell_h, scale);
            assert_eq!(
                device_px(y0 + h0, scale),
                device_px(y1, scale),
                "scale {scale}"
            );
            let (x0, _y, w0, _) = cell_band(0, 0, 2, cell_w, cell_h);
            let (x1, _, _, _) = cell_band(2, 0, 2, cell_w, cell_h);
            let (_l, _t, width, _) = snap_rect(x0, 0.0, w0, cell_h, scale);
            let (right, _, _, _) = snap_rect(x1, 0.0, cell_w, cell_h, scale);
            assert_eq!(
                device_px(x0 + width, scale),
                device_px(right, scale),
                "scale {scale}"
            );
        }
    }

    #[test]
    fn transparent_snapshot_has_no_backdrop() {
        let snap = TerminalSnapshot::blank(3, 2).unwrap();
        assert!(dominant_opaque_bg(&snap).is_none());
    }

    #[test]
    fn dominant_backdrop_prefers_the_common_colour_then_the_earlier_cell() {
        let light = Color::Rgb(250, 250, 250);
        let dark = Color::Rgb(20, 20, 24);
        let cells = vec![
            Cell {
                bg: light,
                ..Cell::blank(0, 0)
            },
            Cell {
                bg: dark,
                ..Cell::blank(1, 0)
            },
            Cell {
                bg: light,
                ..Cell::blank(0, 1)
            },
            Cell {
                bg: Color::Default,
                ..Cell::blank(1, 1)
            },
        ];
        let snap = TerminalSnapshot::try_new(2, 2, Cursor::hidden(), cells).unwrap();
        assert_eq!(
            dominant_opaque_bg(&snap),
            Some(Rgba::opaque((250, 250, 250)))
        );

        let tied = TerminalSnapshot::try_new(
            2,
            1,
            Cursor::hidden(),
            vec![
                Cell {
                    bg: Color::Rgb(1, 0, 0),
                    ..Cell::blank(0, 0)
                },
                Cell {
                    bg: Color::Rgb(0, 0, 1),
                    ..Cell::blank(1, 0)
                },
            ],
        )
        .unwrap();
        assert_eq!(dominant_opaque_bg(&tied), Some(Rgba::opaque((1, 0, 0))));
    }

    #[test]
    fn panel_border_uses_the_corner_fg() {
        let snap = TerminalSnapshot::try_new(
            1,
            1,
            Cursor::hidden(),
            vec![Cell {
                fg: Color::Rgb(255, 140, 0),
                grapheme: "╭".into(),
                ..Cell::blank(0, 0)
            }],
        )
        .unwrap();
        assert_eq!(panel_border_rgba(&snap, 0, 0), Rgba::opaque((255, 140, 0)));
        assert_ne!(panel_border_rgba(&snap, 0, 0), Rgba::opaque(DEFAULT_FG));
    }

    fn vertical_stroke(
        grapheme: &str,
        row: u16,
        cell_w: f64,
        cell_h: f64,
        scale: f64,
    ) -> BoxStroke {
        let strokes = box_cell_strokes(grapheme, cell_w, cell_h, scale).expect(grapheme);
        let mut vertical: Vec<_> = strokes
            .into_iter()
            .filter(|stroke| stroke.height > stroke.width)
            .collect();
        assert_eq!(vertical.len(), 1, "{grapheme}");
        let mut stroke = vertical.remove(0);
        stroke.y += f64::from(row) * cell_h;
        stroke
    }

    #[test]
    fn rounded_prompt_left_edge_is_continuous_in_the_cell_fg() {
        let scale = 2.0;
        let cell_w = snap_cell_px(8.4, scale);
        let cell_h = snap_cell_px(17.2, scale);
        let orange = Color::Rgb(255, 140, 0);
        let mut cells = Vec::new();
        for y in 0..3u16 {
            for x in 0..4u16 {
                let mut cell = Cell::blank(x, y);
                cell.grapheme = match (x, y) {
                    (0, 0) => "╭",
                    (3, 0) => "╮",
                    (0, 2) => "╰",
                    (3, 2) => "╯",
                    (_, 0 | 2) => "─",
                    (0 | 3, 1) => "│",
                    _ => " ",
                }
                .into();
                if x == 0 {
                    cell.fg = orange;
                }
                cells.push(cell);
            }
        }
        let snap = TerminalSnapshot::try_new(4, 3, Cursor::hidden(), cells).unwrap();
        let top = vertical_stroke("╭", 0, cell_w, cell_h, scale);
        let mid = vertical_stroke("│", 1, cell_w, cell_h, scale);
        let bot = vertical_stroke("╰", 2, cell_w, cell_h, scale);
        assert_eq!(device_px(top.x, scale), device_px(mid.x, scale));
        assert_eq!(device_px(mid.x, scale), device_px(bot.x, scale));
        assert_eq!(
            device_px(top.y + top.height, scale),
            device_px(mid.y, scale)
        );
        assert_eq!(
            device_px(mid.y + mid.height, scale),
            device_px(bot.y, scale)
        );
        assert_eq!(device_px(mid.height, scale), device_px(cell_h, scale));
        for y in 0..3u16 {
            let (fg, _) = paint_colors(
                snap.cell(0, y).unwrap().fg,
                Color::Default,
                Style::default(),
            );
            assert_eq!(fg, Rgba::opaque((255, 140, 0)));
        }
        let bar = box_cell_strokes("─", cell_w, cell_h, scale).unwrap();
        let horizontal = bar
            .iter()
            .find(|stroke| stroke.width > stroke.height)
            .unwrap();
        assert_eq!(device_px(horizontal.x, scale), 0);
        assert_eq!(device_px(horizontal.width, scale), device_px(cell_w, scale));
        assert!(box_cell_strokes("h", cell_w, cell_h, scale).is_none());
        assert!(box_cell_strokes("─", cell_w, cell_h, scale).is_some());
    }
}
