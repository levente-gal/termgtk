//! Card geometry and focus. GTK applies the stylesheet; these helpers do not.

#![cfg_attr(not(any(test, feature = "gtk")), allow(dead_code))]

use crate::render::cell_renderer::{resolve_fg, Rgba};
use crate::terminal::Color;
use crate::ui::Theme;

/// Pull each edge of a pane in so neighbouring cards leave an 8px gutter.
pub(crate) const CARD_INSET: f64 = 4.0;
/// Mix the terminal background this far toward black for the canvas.
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) const CANVAS_DARKEN: f32 = 0.16;
/// Black scrim drawn over layout cards while a dialog is open.
pub(crate) const SCRIM_ALPHA: f32 = 0.45;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct PaneFace {
    pub id: u64,
    pub border: Color,
}

/// Origin and size of a card inset from a pane rectangle in pixels.
pub(crate) fn inset_card(x: f64, y: f64, w: f64, h: f64) -> (f64, f64, i32, i32) {
    let width = (w - CARD_INSET * 2.0).round().max(1.0);
    let height = (h - CARD_INSET * 2.0).round().max(1.0);
    (x + CARD_INSET, y + CARD_INSET, width as i32, height as i32)
}

/// A frame in cell coordinates, inclusive of its border.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct FrameRect {
    pub x: u16,
    pub y: u16,
    pub cols: u16,
    pub rows: u16,
}

/// Exact pixel rectangle a card is allocated into.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct CardSlot {
    pub x: f64,
    pub y: f64,
    pub width: i32,
    pub height: i32,
}

#[derive(Clone, Copy)]
struct Edges {
    left: f64,
    top: f64,
    right: f64,
    bottom: f64,
}

/// Place layout panes so shared border cells become an 8px gutter.
///
/// Each frame includes the box-drawing border. Neighbours therefore overlap
/// by one cell. The shared strip is split at its midpoint and each side pulls
/// back by [`CARD_INSET`]. Edges with no neighbour pull in by the same amount.
/// A pane that contains another is left inset, so a dialog can still cover it.
pub(crate) fn resolve_slots(frames: &[FrameRect], cell_w: f64, cell_h: f64) -> Vec<CardSlot> {
    let px: Vec<Edges> = frames
        .iter()
        .map(|frame| frame_edges(frame, cell_w, cell_h))
        .collect();
    px.iter()
        .enumerate()
        .map(|(index, edges)| {
            let resolved = inset_shared(&px, index, *edges);
            slot_from_edges(resolved)
        })
        .collect()
}

fn frame_edges(frame: &FrameRect, cell_w: f64, cell_h: f64) -> Edges {
    Edges {
        left: f64::from(frame.x) * cell_w,
        top: f64::from(frame.y) * cell_h,
        right: f64::from(frame.x.saturating_add(frame.cols)) * cell_w,
        bottom: f64::from(frame.y.saturating_add(frame.rows)) * cell_h,
    }
}

fn inset_shared(all: &[Edges], index: usize, edges: Edges) -> Edges {
    let mut left = edges.left + CARD_INSET;
    let mut top = edges.top + CARD_INSET;
    let mut right = edges.right - CARD_INSET;
    let mut bottom = edges.bottom - CARD_INSET;
    for (other_index, other) in all.iter().enumerate() {
        if other_index == index {
            continue;
        }
        let Some(hit) = intersection(edges, *other) else {
            continue;
        };
        if covers(edges, hit) || covers(*other, hit) {
            continue;
        }
        let overlap_w = hit.right - hit.left;
        let overlap_h = hit.bottom - hit.top;
        if overlap_w < overlap_h {
            let mid = (hit.left + hit.right) * 0.5;
            if center_x(edges) <= center_x(*other) {
                right = right.min(mid - CARD_INSET);
            } else {
                left = left.max(mid + CARD_INSET);
            }
        } else if overlap_h < overlap_w {
            let mid = (hit.top + hit.bottom) * 0.5;
            if center_y(edges) <= center_y(*other) {
                bottom = bottom.min(mid - CARD_INSET);
            } else {
                top = top.max(mid + CARD_INSET);
            }
        }
    }
    Edges {
        left,
        top,
        right,
        bottom,
    }
}

fn intersection(a: Edges, b: Edges) -> Option<Edges> {
    let left = a.left.max(b.left);
    let top = a.top.max(b.top);
    let right = a.right.min(b.right);
    let bottom = a.bottom.min(b.bottom);
    if right - left <= 0.5 || bottom - top <= 0.5 {
        return None;
    }
    Some(Edges {
        left,
        top,
        right,
        bottom,
    })
}

fn covers(pane: Edges, hit: Edges) -> bool {
    let width = pane.right - pane.left;
    let height = pane.bottom - pane.top;
    hit.right - hit.left >= width - 0.5 && hit.bottom - hit.top >= height - 0.5
}

fn center_x(edges: Edges) -> f64 {
    (edges.left + edges.right) * 0.5
}

fn center_y(edges: Edges) -> f64 {
    (edges.top + edges.bottom) * 0.5
}

fn slot_from_edges(edges: Edges) -> CardSlot {
    let width = (edges.right - edges.left).round().max(1.0);
    let height = (edges.bottom - edges.top).round().max(1.0);
    CardSlot {
        x: edges.left,
        y: edges.top,
        width: width as i32,
        height: height as i32,
    }
}

/// Mix `color` toward black. `amount` of 0 keeps it; 1 is black.
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn darken(color: Rgba, amount: f32) -> Rgba {
    let keep = 1.0 - amount.clamp(0.0, 1.0);
    Rgba {
        r: scale_channel(color.r, keep),
        g: scale_channel(color.g, keep),
        b: scale_channel(color.b, keep),
        a: color.a,
    }
}

fn scale_channel(channel: u8, keep: f32) -> u8 {
    (f32::from(channel) * keep).round().clamp(0.0, 255.0) as u8
}

/// Layout pane that should wear the accent ring.
///
/// A border colour used by only one pane wins. When that is ambiguous, the
/// most saturated border wins. Identical borders mean no pane is focused.
pub(crate) fn pick_focused(panes: &[PaneFace]) -> Option<u64> {
    if panes.is_empty() {
        return None;
    }
    if panes.len() == 1 {
        return Some(panes[0].id);
    }
    let mut counts: Vec<(Color, usize)> = Vec::new();
    for pane in panes {
        if let Some(slot) = counts.iter_mut().find(|slot| slot.0 == pane.border) {
            slot.1 += 1;
        } else {
            counts.push((pane.border, 1));
        }
    }
    if counts.len() == 1 {
        return None;
    }
    let unique: Vec<u64> = panes
        .iter()
        .filter(|pane| {
            counts
                .iter()
                .any(|slot| slot.0 == pane.border && slot.1 == 1)
        })
        .map(|pane| pane.id)
        .collect();
    if unique.len() == 1 {
        return Some(unique[0]);
    }
    panes
        .iter()
        .max_by_key(|pane| saturation(pane.border))
        .map(|pane| pane.id)
}

fn saturation(color: Color) -> u16 {
    let rgba = resolve_fg(color);
    let max = u16::from(rgba.r.max(rgba.g).max(rgba.b));
    let min = u16::from(rgba.r.min(rgba.g).min(rgba.b));
    max - min
}

pub(crate) fn ring_class(color: Rgba) -> String {
    format!("herdr-ring-{:02x}{:02x}{:02x}", color.r, color.g, color.b)
}

/// Canvas gutter and card fill for enhanced overlay.
///
/// These stay on [`Theme`] so a focused pane's selection or reverse-video
/// majority cannot retint every card.
pub(crate) fn enhanced_chrome() -> (Rgba, Rgba) {
    (Rgba::opaque(Theme::BACKGROUND), Rgba::opaque(Theme::CARD))
}

/// Overlay-only rules. Shared card, row, and scrollbar CSS lives on [`Theme`].
pub(crate) fn card_stylesheet(ring: Option<Rgba>) -> String {
    let (cr, cg, cb) = Theme::CARD;
    let mut css = format!(
        "\
.herdr-dialog {{
  box-shadow: 0 16px 48px rgba(0, 0, 0, 0.45), 0 2px 8px rgba(0, 0, 0, 0.35);
}}
.herdr-card-count {{
  padding: 8px 12px 4px 0;
}}
.herdr-fade {{
  min-height: 20px;
  background-image: linear-gradient(to bottom, rgba({cr}, {cg}, {cb}, 0), #{cr:02x}{cg:02x}{cb:02x});
}}
"
    );
    if let Some(color) = ring {
        let class = ring_class(color);
        let hex = format!("#{:02x}{:02x}{:02x}", color.r, color.g, color.b);
        css.push_str(&format!(
            "\
.herdr-card.focused.{class} {{
  box-shadow: 0 0 0 2px {hex}, 0 8px 24px rgba(0, 0, 0, 0.28), 0 1px 2px rgba(0, 0, 0, 0.18);
}}
.herdr-dialog.focused.{class} {{
  box-shadow: 0 0 0 2px {hex}, 0 16px 48px rgba(0, 0, 0, 0.45), 0 2px 8px rgba(0, 0, 0, 0.35);
}}
"
        ));
    }
    css
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inset_pulls_each_edge_in_by_four() {
        let (x, y, w, h) = inset_card(10.0, 20.0, 100.0, 80.0);
        assert_eq!((x, y, w, h), (14.0, 24.0, 92, 72));
    }

    #[test]
    fn inset_never_collapses() {
        let (_, _, w, h) = inset_card(0.0, 0.0, 4.0, 2.0);
        assert!(w >= 1 && h >= 1);
    }

    #[test]
    fn a_lone_pane_is_inset_on_every_edge() {
        let slots = resolve_slots(
            &[FrameRect {
                x: 0,
                y: 0,
                cols: 10,
                rows: 5,
            }],
            10.0,
            20.0,
        );
        let (x, y, width, height) = inset_card(0.0, 0.0, 100.0, 100.0);
        assert_eq!(
            slots,
            vec![CardSlot {
                x,
                y,
                width,
                height
            }]
        );
    }

    #[test]
    fn vertical_neighbors_split_the_shared_border() {
        let slots = resolve_slots(
            &[
                FrameRect {
                    x: 0,
                    y: 0,
                    cols: 10,
                    rows: 5,
                },
                FrameRect {
                    x: 0,
                    y: 4,
                    cols: 10,
                    rows: 10,
                },
            ],
            10.0,
            20.0,
        );
        let top = slots[0];
        let bottom = slots[1];
        let top_bottom = top.y + f64::from(top.height);
        assert!(
            (bottom.y - top_bottom - 8.0).abs() < 0.5,
            "gutter {top_bottom} -> {}",
            bottom.y
        );
        assert!((top.y - 4.0).abs() < 0.1, "outer top {}", top.y);
        assert!(bottom.y >= top_bottom);
    }

    #[test]
    fn horizontal_neighbors_split_the_shared_border() {
        let slots = resolve_slots(
            &[
                FrameRect {
                    x: 0,
                    y: 0,
                    cols: 10,
                    rows: 8,
                },
                FrameRect {
                    x: 9,
                    y: 0,
                    cols: 11,
                    rows: 8,
                },
            ],
            10.0,
            16.0,
        );
        let left = slots[0];
        let right = slots[1];
        let left_right = left.x + f64::from(left.width);
        assert!(
            (right.x - left_right - 8.0).abs() < 0.5,
            "gutter {left_right} -> {}",
            right.x
        );
        assert!(right.x >= left_right);
        assert!((left.y - 4.0).abs() < 0.1, "outer top {}", left.y);
    }

    #[test]
    fn darken_mixes_sixteen_percent_toward_black() {
        let color = Rgba::opaque((100, 50, 0));
        let darkened = darken(color, CANVAS_DARKEN);
        assert_eq!(darkened.r, 84);
        assert_eq!(darkened.g, 42);
        assert_eq!(darkened.b, 0);
        assert_eq!(darkened.a, 255);
    }

    #[test]
    fn unique_border_is_focused() {
        let panes = [
            PaneFace {
                id: 1,
                border: Color::Indexed(8),
            },
            PaneFace {
                id: 2,
                border: Color::Indexed(7),
            },
            PaneFace {
                id: 3,
                border: Color::Indexed(8),
            },
        ];
        assert_eq!(pick_focused(&panes), Some(2));
    }

    #[test]
    fn matching_borders_are_not_focused() {
        let panes = [
            PaneFace {
                id: 1,
                border: Color::Indexed(8),
            },
            PaneFace {
                id: 2,
                border: Color::Indexed(8),
            },
        ];
        assert_eq!(pick_focused(&panes), None);
    }

    #[test]
    fn saturated_border_wins_when_every_colour_is_unique() {
        let panes = [
            PaneFace {
                id: 1,
                border: Color::Rgb(128, 128, 128),
            },
            PaneFace {
                id: 2,
                border: Color::Rgb(40, 200, 80),
            },
        ];
        assert_eq!(pick_focused(&panes), Some(2));
    }

    #[test]
    fn a_single_pane_is_focused() {
        let panes = [PaneFace {
            id: 4,
            border: Color::Default,
        }];
        assert_eq!(pick_focused(&panes), Some(4));
    }

    #[test]
    fn stylesheet_describes_the_card_and_the_ring() {
        let ring = Rgba::opaque((0x3d, 0xdc, 0x97));
        let css = card_stylesheet(Some(ring));
        assert!(!css.contains("overflow"));
        assert!(!css.contains("margin-bottom: -"));
        assert!(!css.contains("opacity:"));
        assert!(css.contains(".herdr-fade"));
        assert!(css.contains(".herdr-card-count"));
        assert!(css.contains("herdr-ring-3ddc97"));
        assert!(css.contains("0 0 0 2px #3ddc97"));
        assert!(!css.contains('┌'));
        let shared = Theme::stylesheet();
        assert!(shared.contains("border-radius: 12px"));
        assert!(shared.contains("background-color: #171a20"));
        assert!(shared.contains("alpha(@accent_bg_color, 0.28)"));
        assert!(shared.contains("scrollbar.herdr-scroll slider"));
    }

    #[test]
    fn overlay_chrome_does_not_follow_a_reverse_video_majority() {
        use crate::render::cell_renderer::dominant_opaque_bg;
        use crate::terminal::{Cell, Cursor, Style, TerminalSnapshot};

        let mut cells = Vec::new();
        for y in 0..4u16 {
            for x in 0..8u16 {
                let mut cell = Cell::blank(x, y);
                cell.style = Style {
                    reverse: y < 3,
                    ..Style::default()
                };
                cells.push(cell);
            }
        }
        let snap = TerminalSnapshot::try_new(8, 4, Cursor::hidden(), cells).unwrap();
        let sampled = dominant_opaque_bg(&snap).expect("reverse cells paint an opaque bg");
        let (canvas, card) = enhanced_chrome();
        assert_eq!(card, Rgba::opaque(Theme::CARD));
        assert_eq!(canvas, Rgba::opaque(Theme::BACKGROUND));
        assert_ne!(sampled, card);
        assert_ne!(sampled, canvas);
        let css = Theme::stylesheet();
        assert!(css.contains("background-color: #171a20"));
        assert!(!css.contains(&format!(
            "background-color: #{:02x}{:02x}{:02x}",
            sampled.r, sampled.g, sampled.b
        )));
    }
}
