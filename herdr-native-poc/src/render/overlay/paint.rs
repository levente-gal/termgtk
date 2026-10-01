//! Draw the layout tree. Each node is a rectangle. Dialogs paint above the panes.
#![allow(dead_code)]

use gtk::prelude::*;
use gtk::{gdk, graphene};

use crate::terminal::TerminalSnapshot;

use super::super::cell_renderer::{resolve_fg, snap_rect, Rgba};
use super::tree::{node_children, node_rect, node_scroll, LayoutTree};

pub(crate) fn paint_tree(
    snapshot: &gtk::Snapshot,
    _terminal: &TerminalSnapshot,
    tree: &LayoutTree,
    cell_w: f64,
    cell_h: f64,
    scale: f64,
) {
    let scale = if scale.is_finite() && scale > 0.0 {
        scale
    } else {
        1.0
    };
    let holes = overlay_bounds(tree, cell_w, cell_h);
    paint_nodes(snapshot, tree.layout(), &holes, cell_w, cell_h, scale);
    paint_nodes(snapshot, tree.overlays(), &[], cell_w, cell_h, scale);
}

fn paint_nodes(
    snapshot: &gtk::Snapshot,
    nodes: &[super::tree::Node],
    holes: &[Bounds],
    cell_w: f64,
    cell_h: f64,
    scale: f64,
) {
    for node in nodes {
        let (x, y, cols, rows, border) = node_rect(node);
        let card = Bounds {
            x: f64::from(x) * cell_w,
            y: f64::from(y) * cell_h,
            width: f64::from(cols) * cell_w,
            height: f64::from(rows) * cell_h,
        };
        let color = rgba_to_gdk(resolve_fg(border));
        for region in subtract_bounds(card, holes) {
            if region.width <= 0.0 || region.height <= 0.0 {
                continue;
            }
            snapshot.push_clip(&graphene::Rect::new(
                region.x as f32,
                region.y as f32,
                region.width as f32,
                region.height as f32,
            ));
            stroke(snapshot, card, &color, scale);
            if let Some((current, total)) = node_scroll(node) {
                paint_thumb(
                    snapshot, card, current, total, cell_w, cell_h, &color, scale,
                );
            }
            snapshot.pop();
        }
        paint_nodes(snapshot, node_children(node), holes, cell_w, cell_h, scale);
    }
}

fn overlay_bounds(tree: &LayoutTree, cell_w: f64, cell_h: f64) -> Vec<Bounds> {
    let mut bounds = Vec::new();
    collect_bounds(tree.overlays(), cell_w, cell_h, &mut bounds);
    bounds
}

fn collect_bounds(nodes: &[super::tree::Node], cell_w: f64, cell_h: f64, out: &mut Vec<Bounds>) {
    for node in nodes {
        let (x, y, cols, rows, _) = node_rect(node);
        out.push(Bounds {
            x: f64::from(x) * cell_w,
            y: f64::from(y) * cell_h,
            width: f64::from(cols) * cell_w,
            height: f64::from(rows) * cell_h,
        });
        collect_bounds(node_children(node), cell_w, cell_h, out);
    }
}

#[derive(Clone, Copy)]
struct Bounds {
    x: f64,
    y: f64,
    width: f64,
    height: f64,
}

fn stroke(snapshot: &gtk::Snapshot, card: Bounds, color: &gdk::RGBA, scale: f64) {
    let hair = 1.0 / scale;
    fill(snapshot, card.x, card.y, card.width, hair, color, scale);
    fill(
        snapshot,
        card.x,
        card.y + card.height - hair,
        card.width,
        hair,
        color,
        scale,
    );
    fill(snapshot, card.x, card.y, hair, card.height, color, scale);
    fill(
        snapshot,
        card.x + card.width - hair,
        card.y,
        hair,
        card.height,
        color,
        scale,
    );
}

fn paint_thumb(
    snapshot: &gtk::Snapshot,
    card: Bounds,
    current: u32,
    total: u32,
    cell_w: f64,
    cell_h: f64,
    color: &gdk::RGBA,
    scale: f64,
) {
    if total <= 1 {
        return;
    }
    let inner_top = card.y + cell_h;
    let inner_h = card.height - cell_h * 2.0;
    if inner_h <= 0.0 {
        return;
    }
    let thumb_h = (cell_h * 2.0).clamp(cell_h, inner_h);
    let span = (total - 1) as f64;
    let t = (current.saturating_sub(1) as f64 / span).clamp(0.0, 1.0);
    let y = inner_top + t * (inner_h - thumb_h);
    let thickness = (cell_w * 0.45).max(2.0 / scale);
    fill(
        snapshot,
        card.x + card.width - thickness,
        y,
        thickness,
        thumb_h,
        color,
        scale,
    );
}

fn fill(snapshot: &gtk::Snapshot, x: f64, y: f64, w: f64, h: f64, color: &gdk::RGBA, scale: f64) {
    let (x, y, w, h) = snap_rect(x, y, w, h, scale);
    if w <= 0.0 || h <= 0.0 {
        return;
    }
    snapshot.append_color(
        color,
        &graphene::Rect::new(x as f32, y as f32, w as f32, h as f32),
    );
}

fn subtract_bounds(card: Bounds, holes: &[Bounds]) -> Vec<Bounds> {
    let mut rects = vec![card];
    for hole in holes {
        rects = rects
            .into_iter()
            .flat_map(|rect| subtract_one(rect, *hole))
            .collect();
    }
    rects
}

fn subtract_one(card: Bounds, hole: Bounds) -> Vec<Bounds> {
    let x1 = card.x + card.width;
    let y1 = card.y + card.height;
    let hx1 = hole.x + hole.width;
    let hy1 = hole.y + hole.height;
    let ix0 = card.x.max(hole.x);
    let iy0 = card.y.max(hole.y);
    let ix1 = x1.min(hx1);
    let iy1 = y1.min(hy1);
    if ix1 <= ix0 || iy1 <= iy0 || card.width <= 0.0 || card.height <= 0.0 {
        return vec![card];
    }
    let mut out = Vec::new();
    if iy0 > card.y {
        out.push(Bounds {
            x: card.x,
            y: card.y,
            width: card.width,
            height: iy0 - card.y,
        });
    }
    if y1 > iy1 {
        out.push(Bounds {
            x: card.x,
            y: iy1,
            width: card.width,
            height: y1 - iy1,
        });
    }
    if ix0 > card.x {
        out.push(Bounds {
            x: card.x,
            y: iy0,
            width: ix0 - card.x,
            height: iy1 - iy0,
        });
    }
    if x1 > ix1 {
        out.push(Bounds {
            x: ix1,
            y: iy0,
            width: x1 - ix1,
            height: iy1 - iy0,
        });
    }
    out
}

fn rgba_to_gdk(color: Rgba) -> gdk::RGBA {
    gdk::RGBA::new(
        f32::from(color.r) / 255.0,
        f32::from(color.g) / 255.0,
        f32::from(color.b) / 255.0,
        f32::from(color.a) / 255.0,
    )
}
