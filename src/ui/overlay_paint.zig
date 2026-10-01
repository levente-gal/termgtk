//! Enhanced overlay paint: box-cell strokes and card border strokes.

const std = @import("std");
const contract = @import("contract");
const gtk = @import("gtk");
const paint = @import("paint");
const cards = @import("cards");

const GdkRGBA = @typeInfo(@typeInfo(@TypeOf(gtk.Snapshot.appendColor)).@"fn".params[1].type.?).pointer.child;
const GrapheneRect = @typeInfo(@typeInfo(@TypeOf(gtk.Snapshot.appendColor)).@"fn".params[2].type.?).pointer.child;

pub const Bounds = struct { x: f64, y: f64, width: f64, height: f64 };

pub fn toCardsRgba(color: contract.Color) cards.Rgba {
    const c = paint.resolveFg(color);
    return .{ .r = c.r, .g = c.g, .b = c.b, .a = c.a };
}

pub fn paintScrim(snapshot: *gtk.Snapshot, width: f32, height: f32) void {
    var color: GdkRGBA = .{ .f_red = 0, .f_green = 0, .f_blue = 0, .f_alpha = cards.SCRIM_ALPHA };
    var bounds: GrapheneRect = undefined;
    _ = bounds.init(0, 0, width, height);
    snapshot.appendColor(&color, &bounds);
}

pub fn paintEnhancedBackdrop(snapshot: *gtk.Snapshot, width: f32, height: f32) void {
    const canvas, _ = cards.enhanced_chrome();
    var color: GdkRGBA = .{
        .f_red = @as(f32, @floatFromInt(canvas.r)) / 255.0,
        .f_green = @as(f32, @floatFromInt(canvas.g)) / 255.0,
        .f_blue = @as(f32, @floatFromInt(canvas.b)) / 255.0,
        .f_alpha = @as(f32, @floatFromInt(canvas.a)) / 255.0,
    };
    var bounds: GrapheneRect = undefined;
    _ = bounds.init(0, 0, width, height);
    snapshot.appendColor(&color, &bounds);
}

/// Stroke box-drawing graphemes. Returns true when stroked (caller skips glyph paint).
pub fn paintBoxCell(
    snapshot: *gtk.Snapshot,
    grapheme: []const u8,
    origin_x: f64,
    origin_y: f64,
    cell_w: f64,
    cell_h: f64,
    scale: f64,
    fg: contract.Color,
    bg: contract.Color,
    style: contract.Style,
) bool {
    var strokes: [8]paint.BoxStroke = undefined;
    const n = paint.boxCellStrokes(grapheme, cell_w, cell_h, scale, &strokes) orelse return false;
    const c = paint.paintColors(fg, bg, style)[0];
    var color: GdkRGBA = .{
        .f_red = @as(f32, @floatFromInt(c.r)) / 255.0,
        .f_green = @as(f32, @floatFromInt(c.g)) / 255.0,
        .f_blue = @as(f32, @floatFromInt(c.b)) / 255.0,
        .f_alpha = @as(f32, @floatFromInt(c.a)) / 255.0,
    };
    const s = if (std.math.isFinite(scale) and scale > 0.0) scale else 1.0;
    for (strokes[0..n]) |stroke| {
        const snapped = paint.snapRect(origin_x + stroke.x, origin_y + stroke.y, stroke.width, stroke.height, s);
        if (snapped[2] <= 0.0 or snapped[3] <= 0.0) continue;
        var bounds: GrapheneRect = undefined;
        _ = bounds.init(@floatCast(snapped[0]), @floatCast(snapped[1]), @floatCast(snapped[2]), @floatCast(snapped[3]));
        snapshot.appendColor(&color, &bounds);
    }
    return true;
}

pub fn overlayHole(x: u16, y: u16, cols: u16, rows: u16, cell_w: f64, cell_h: f64) Bounds {
    return .{
        .x = @as(f64, @floatFromInt(x)) * cell_w,
        .y = @as(f64, @floatFromInt(y)) * cell_h,
        .width = @as(f64, @floatFromInt(cols)) * cell_w,
        .height = @as(f64, @floatFromInt(rows)) * cell_h,
    };
}

/// Paint one closed-box frame (POC overlay/paint.rs stroke path).
pub fn paintFrame(
    snapshot: *gtk.Snapshot,
    x: u16,
    y: u16,
    cols: u16,
    rows: u16,
    border: contract.Color,
    scroll: ?struct { u32, u32 },
    holes: []const Bounds,
    cell_w: f64,
    cell_h: f64,
    scale: f64,
) void {
    const s = if (std.math.isFinite(scale) and scale > 0.0) scale else 1.0;
    const card = Bounds{
        .x = @as(f64, @floatFromInt(x)) * cell_w,
        .y = @as(f64, @floatFromInt(y)) * cell_h,
        .width = @as(f64, @floatFromInt(cols)) * cell_w,
        .height = @as(f64, @floatFromInt(rows)) * cell_h,
    };
    const c = paint.resolveFg(border);
    var color: GdkRGBA = .{
        .f_red = @as(f32, @floatFromInt(c.r)) / 255.0,
        .f_green = @as(f32, @floatFromInt(c.g)) / 255.0,
        .f_blue = @as(f32, @floatFromInt(c.b)) / 255.0,
        .f_alpha = @as(f32, @floatFromInt(c.a)) / 255.0,
    };
    var regions_buf: [16]Bounds = undefined;
    var regions_n: usize = 1;
    regions_buf[0] = card;
    for (holes) |hole| {
        var next: [16]Bounds = undefined;
        var next_n: usize = 0;
        for (regions_buf[0..regions_n]) |rect| {
            const x1 = rect.x + rect.width;
            const y1 = rect.y + rect.height;
            const hx1 = hole.x + hole.width;
            const hy1 = hole.y + hole.height;
            const ix0 = @max(rect.x, hole.x);
            const iy0 = @max(rect.y, hole.y);
            const ix1 = @min(x1, hx1);
            const iy1 = @min(y1, hy1);
            if (ix1 <= ix0 or iy1 <= iy0 or rect.width <= 0.0 or rect.height <= 0.0) {
                if (next_n < next.len) {
                    next[next_n] = rect;
                    next_n += 1;
                }
                continue;
            }
            if (iy0 > rect.y and next_n < next.len) {
                next[next_n] = .{ .x = rect.x, .y = rect.y, .width = rect.width, .height = iy0 - rect.y };
                next_n += 1;
            }
            if (y1 > iy1 and next_n < next.len) {
                next[next_n] = .{ .x = rect.x, .y = iy1, .width = rect.width, .height = y1 - iy1 };
                next_n += 1;
            }
            if (ix0 > rect.x and next_n < next.len) {
                next[next_n] = .{ .x = rect.x, .y = iy0, .width = ix0 - rect.x, .height = iy1 - iy0 };
                next_n += 1;
            }
            if (x1 > ix1 and next_n < next.len) {
                next[next_n] = .{ .x = ix1, .y = iy0, .width = x1 - ix1, .height = iy1 - iy0 };
                next_n += 1;
            }
        }
        @memcpy(regions_buf[0..next_n], next[0..next_n]);
        regions_n = next_n;
    }
    for (regions_buf[0..regions_n]) |region| {
        if (region.width <= 0.0 or region.height <= 0.0) continue;
        var clip: GrapheneRect = undefined;
        _ = clip.init(@floatCast(region.x), @floatCast(region.y), @floatCast(region.width), @floatCast(region.height));
        snapshot.pushClip(&clip);
        const hair = 1.0 / s;
        fill(snapshot, card.x, card.y, card.width, hair, &color, s);
        fill(snapshot, card.x, card.y + card.height - hair, card.width, hair, &color, s);
        fill(snapshot, card.x, card.y, hair, card.height, &color, s);
        fill(snapshot, card.x + card.width - hair, card.y, hair, card.height, &color, s);
        if (scroll) |pair| {
            const current, const total = pair;
            if (total > 1) {
                const inner_top = card.y + cell_h;
                const inner_h = card.height - cell_h * 2.0;
                if (inner_h > 0.0) {
                    const thumb_h = @min(@max(cell_h * 2.0, cell_h), inner_h);
                    const span = @as(f64, @floatFromInt(total - 1));
                    const t = @min(@max(@as(f64, @floatFromInt(current -| 1)) / span, 0.0), 1.0);
                    const ty = inner_top + t * (inner_h - thumb_h);
                    const thickness = @max(cell_w * 0.45, 2.0 / s);
                    fill(snapshot, card.x + card.width - thickness, ty, thickness, thumb_h, &color, s);
                }
            }
        }
        snapshot.pop();
    }
}

fn fill(snapshot: *gtk.Snapshot, x: f64, y: f64, w: f64, h: f64, color: *const GdkRGBA, scale: f64) void {
    const snapped = paint.snapRect(x, y, w, h, scale);
    if (snapped[2] <= 0.0 or snapped[3] <= 0.0) return;
    var bounds: GrapheneRect = undefined;
    _ = bounds.init(@floatCast(snapped[0]), @floatCast(snapped[1]), @floatCast(snapped[2]), @floatCast(snapped[3]));
    snapshot.appendColor(color, &bounds);
}

comptime {
    _ = toCardsRgba;
    _ = paintScrim;
    _ = paintEnhancedBackdrop;
    _ = paintBoxCell;
    _ = overlayHole;
    _ = paintFrame;
}
