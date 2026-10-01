//! Card geometry and focus. GTK applies the stylesheet; these helpers do not.

const std = @import("std");
const contract = @import("contract");

pub const CARD_INSET: f64 = 4.0;
pub const CANVAS_DARKEN: f32 = 0.16;
pub const SCRIM_ALPHA: f32 = 0.45;

const BACKGROUND = .{ 0x11, 0x13, 0x18 };
const CARD = .{ 0x17, 0x1a, 0x20 };
const DEFAULT_FG = .{ 0xe8, 0xe8, 0xea };

pub const Rgba = struct {
    r: u8,
    g: u8,
    b: u8,
    a: u8,

    pub fn opaqueRgb(rgb: struct { u8, u8, u8 }) Rgba {
        return .{ .r = rgb[0], .g = rgb[1], .b = rgb[2], .a = 255 };
    }
};

pub const PaneFace = struct {
    id: u64,
    border: contract.Color,
};

pub const FrameRect = struct {
    x: u16,
    y: u16,
    cols: u16,
    rows: u16,
};

pub const CardSlot = struct {
    x: f64,
    y: f64,
    width: i32,
    height: i32,
};

const Edges = struct {
    left: f64,
    top: f64,
    right: f64,
    bottom: f64,
};

pub fn inset_card(x: f64, y: f64, w: f64, h: f64) struct { f64, f64, i32, i32 } {
    return .{
        x + CARD_INSET,
        y + CARD_INSET,
        @intFromFloat(@max(@round(w - CARD_INSET * 2.0), 1.0)),
        @intFromFloat(@max(@round(h - CARD_INSET * 2.0), 1.0)),
    };
}

pub fn resolve_slots(frames: []const FrameRect, cell_w: f64, cell_h: f64, allocator: std.mem.Allocator) ![]CardSlot {
    var px: std.ArrayList(Edges) = .empty;
    defer px.deinit(allocator);
    try px.ensureTotalCapacity(allocator, frames.len);
    for (frames) |frame| {
        try px.append(allocator, frame_edges(frame, cell_w, cell_h));
    }
    var out: std.ArrayList(CardSlot) = .empty;
    errdefer out.deinit(allocator);
    try out.ensureTotalCapacity(allocator, frames.len);
    for (px.items, 0..) |edges, index| {
        try out.append(allocator, slot_from_edges(inset_shared(px.items, index, edges)));
    }
    return try out.toOwnedSlice(allocator);
}

fn frame_edges(frame: FrameRect, cell_w: f64, cell_h: f64) Edges {
    return .{
        .left = @as(f64, @floatFromInt(frame.x)) * cell_w,
        .top = @as(f64, @floatFromInt(frame.y)) * cell_h,
        .right = @as(f64, @floatFromInt(frame.x +% frame.cols)) * cell_w,
        .bottom = @as(f64, @floatFromInt(frame.y +% frame.rows)) * cell_h,
    };
}

fn inset_shared(all: []const Edges, index: usize, edges: Edges) Edges {
    var left = edges.left + CARD_INSET;
    var top = edges.top + CARD_INSET;
    var right = edges.right - CARD_INSET;
    var bottom = edges.bottom - CARD_INSET;
    for (all, 0..) |other, other_index| {
        if (other_index == index) continue;
        const hit = intersection(edges, other) orelse continue;
        if (covers(edges, hit) or covers(other, hit)) continue;
        const overlap_w = hit.right - hit.left;
        const overlap_h = hit.bottom - hit.top;
        if (overlap_w < overlap_h) {
            const mid = (hit.left + hit.right) * 0.5;
            if (center_x(edges) <= center_x(other)) {
                right = @min(right, mid - CARD_INSET);
            } else {
                left = @max(left, mid + CARD_INSET);
            }
        } else if (overlap_h < overlap_w) {
            const mid = (hit.top + hit.bottom) * 0.5;
            if (center_y(edges) <= center_y(other)) {
                bottom = @min(bottom, mid - CARD_INSET);
            } else {
                top = @max(top, mid + CARD_INSET);
            }
        }
    }
    return .{ .left = left, .top = top, .right = right, .bottom = bottom };
}

fn intersection(a: Edges, b: Edges) ?Edges {
    const left = @max(a.left, b.left);
    const top = @max(a.top, b.top);
    const right = @min(a.right, b.right);
    const bottom = @min(a.bottom, b.bottom);
    if (right - left <= 0.5 or bottom - top <= 0.5) return null;
    return .{ .left = left, .top = top, .right = right, .bottom = bottom };
}

fn covers(pane: Edges, hit: Edges) bool {
    const width = pane.right - pane.left;
    const height = pane.bottom - pane.top;
    return hit.right - hit.left >= width - 0.5 and hit.bottom - hit.top >= height - 0.5;
}

fn center_x(edges: Edges) f64 {
    return (edges.left + edges.right) * 0.5;
}

fn center_y(edges: Edges) f64 {
    return (edges.top + edges.bottom) * 0.5;
}

fn slot_from_edges(edges: Edges) CardSlot {
    return .{
        .x = edges.left,
        .y = edges.top,
        .width = @intFromFloat(@max(@round(edges.right - edges.left), 1.0)),
        .height = @intFromFloat(@max(@round(edges.bottom - edges.top), 1.0)),
    };
}

pub fn darken(color: Rgba, amount: f32) Rgba {
    const keep = 1.0 - @min(@max(amount, 0.0), 1.0);
    return .{
        .r = scale_channel(color.r, keep),
        .g = scale_channel(color.g, keep),
        .b = scale_channel(color.b, keep),
        .a = color.a,
    };
}

fn scale_channel(channel: u8, keep: f32) u8 {
    return @intFromFloat(@min(@max(@round(@as(f32, @floatFromInt(channel)) * keep), 0.0), 255.0));
}

pub fn pick_focused(panes: []const PaneFace) ?u64 {
    if (panes.len == 0) return null;
    if (panes.len == 1) return panes[0].id;

    var counts_buf: [64]struct { color: contract.Color, n: usize } = undefined;
    var counts_len: usize = 0;
    for (panes) |pane| {
        var found = false;
        for (counts_buf[0..counts_len]) |*slot| {
            if (colorEql(slot.color, pane.border)) {
                slot.n += 1;
                found = true;
                break;
            }
        }
        if (!found and counts_len < counts_buf.len) {
            counts_buf[counts_len] = .{ .color = pane.border, .n = 1 };
            counts_len += 1;
        }
    }
    if (counts_len == 1) return null;

    var unique_id: ?u64 = null;
    var unique_count: usize = 0;
    for (panes) |pane| {
        var n: usize = 0;
        for (counts_buf[0..counts_len]) |slot| {
            if (colorEql(slot.color, pane.border)) {
                n = slot.n;
                break;
            }
        }
        if (n == 1) {
            unique_count += 1;
            unique_id = pane.id;
        }
    }
    if (unique_count == 1) return unique_id;

    var best = panes[0];
    for (panes[1..]) |pane| {
        if (saturation(pane.border) > saturation(best.border)) best = pane;
    }
    return best.id;
}

fn colorEql(a: contract.Color, b: contract.Color) bool {
    return std.meta.eql(a, b);
}

fn saturation(color: contract.Color) u16 {
    const rgba = resolve_fg(color);
    const max = @max(rgba.r, @max(rgba.g, rgba.b));
    const min = @min(rgba.r, @min(rgba.g, rgba.b));
    return @as(u16, max) - @as(u16, min);
}

fn resolve_fg(color: contract.Color) Rgba {
    return switch (color) {
        .default => Rgba.opaqueRgb(DEFAULT_FG),
        .indexed => |index| Rgba.opaqueRgb(indexed_rgb(index)),
        .rgb => |c| Rgba.opaqueRgb(.{ c.r, c.g, c.b }),
    };
}

fn indexed_rgb(index: u8) struct { u8, u8, u8 } {
    return switch (index) {
        0 => .{ 0x00, 0x00, 0x00 },
        1 => .{ 0xcd, 0x00, 0x00 },
        2 => .{ 0x00, 0xcd, 0x00 },
        3 => .{ 0xcd, 0xcd, 0x00 },
        4 => .{ 0x00, 0x00, 0xee },
        5 => .{ 0xcd, 0x00, 0xcd },
        6 => .{ 0x00, 0xcd, 0xcd },
        7 => .{ 0xe5, 0xe5, 0xe5 },
        8 => .{ 0x7f, 0x7f, 0x7f },
        9 => .{ 0xff, 0x00, 0x00 },
        10 => .{ 0x00, 0xff, 0x00 },
        11 => .{ 0xff, 0xff, 0x00 },
        12 => .{ 0x5c, 0x5c, 0xff },
        13 => .{ 0xff, 0x00, 0xff },
        14 => .{ 0x00, 0xff, 0xff },
        15 => .{ 0xff, 0xff, 0xff },
        16...231 => blk: {
            const n = index - 16;
            const r = n / 36;
            const g = (n % 36) / 6;
            const b = n % 6;
            break :blk .{ cube(r), cube(g), cube(b) };
        },
        else => blk: {
            const level: u16 = 8 + (@as(u16, index) - 232) * 10;
            const v: u8 = @intCast(level);
            break :blk .{ v, v, v };
        },
    };
}

fn cube(value: u8) u8 {
    if (value == 0) return 0;
    return 55 + 40 * value;
}

pub fn ring_class(color: Rgba, buf: *[32]u8) []const u8 {
    return std.fmt.bufPrint(buf, "herdr-ring-{x:0>2}{x:0>2}{x:0>2}", .{ color.r, color.g, color.b }) catch unreachable;
}

pub fn enhanced_chrome() struct { Rgba, Rgba } {
    return .{ Rgba.opaqueRgb(BACKGROUND), Rgba.opaqueRgb(CARD) };
}

pub fn card_stylesheet(ring: ?Rgba, allocator: std.mem.Allocator) ![]u8 {
    const cr, const cg, const cb = CARD;
    var css = try std.fmt.allocPrint(allocator,
        \\.herdr-dialog {{
        \\  box-shadow: 0 16px 48px rgba(0, 0, 0, 0.45), 0 2px 8px rgba(0, 0, 0, 0.35);
        \\}}
        \\.herdr-card-count {{
        \\  padding: 8px 12px 4px 0;
        \\}}
        \\.herdr-fade {{
        \\  min-height: 20px;
        \\  background-image: linear-gradient(to bottom, rgba({d}, {d}, {d}, 0), #{x:0>2}{x:0>2}{x:0>2});
        \\}}
        \\
    , .{ cr, cg, cb, cr, cg, cb });
    if (ring) |color| {
        var class_buf: [32]u8 = undefined;
        const class = ring_class(color, &class_buf);
        const extra = try std.fmt.allocPrint(allocator,
            \\.herdr-card.focused.{s} {{
            \\  box-shadow: 0 0 0 2px #{x:0>2}{x:0>2}{x:0>2}, 0 8px 24px rgba(0, 0, 0, 0.28), 0 1px 2px rgba(0, 0, 0, 0.18);
            \\}}
            \\.herdr-dialog.focused.{s} {{
            \\  box-shadow: 0 0 0 2px #{x:0>2}{x:0>2}{x:0>2}, 0 16px 48px rgba(0, 0, 0, 0.45), 0 2px 8px rgba(0, 0, 0, 0.35);
            \\}}
            \\
        , .{ class, color.r, color.g, color.b, class, color.r, color.g, color.b });
        defer allocator.free(extra);
        const joined = try std.mem.concat(allocator, u8, &.{ css, extra });
        allocator.free(css);
        css = joined;
    }
    return css;
}

test "inset pulls each edge in by four" {
    const x, const y, const w, const h = inset_card(10.0, 20.0, 100.0, 80.0);
    try std.testing.expectEqual(@as(f64, 14.0), x);
    try std.testing.expectEqual(@as(f64, 24.0), y);
    try std.testing.expectEqual(@as(i32, 92), w);
    try std.testing.expectEqual(@as(i32, 72), h);
}

test "inset never collapses" {
    const result = inset_card(0.0, 0.0, 4.0, 2.0);
    try std.testing.expect(result[2] >= 1 and result[3] >= 1);
}

test "a lone pane is inset on every edge" {
    const alloc = std.testing.allocator;
    const slots = try resolve_slots(&.{.{ .x = 0, .y = 0, .cols = 10, .rows = 5 }}, 10.0, 20.0, alloc);
    defer alloc.free(slots);
    const x, const y, const width, const height = inset_card(0.0, 0.0, 100.0, 100.0);
    try std.testing.expectEqual(@as(usize, 1), slots.len);
    try std.testing.expectEqual(x, slots[0].x);
    try std.testing.expectEqual(y, slots[0].y);
    try std.testing.expectEqual(width, slots[0].width);
    try std.testing.expectEqual(height, slots[0].height);
}

test "vertical neighbors split the shared border" {
    const alloc = std.testing.allocator;
    const slots = try resolve_slots(&.{
        .{ .x = 0, .y = 0, .cols = 10, .rows = 5 },
        .{ .x = 0, .y = 4, .cols = 10, .rows = 10 },
    }, 10.0, 20.0, alloc);
    defer alloc.free(slots);
    const top = slots[0];
    const bottom = slots[1];
    const top_bottom = top.y + @as(f64, @floatFromInt(top.height));
    try std.testing.expect(@abs(bottom.y - top_bottom - 8.0) < 0.5);
    try std.testing.expect(@abs(top.y - 4.0) < 0.1);
    try std.testing.expect(bottom.y >= top_bottom);
}

test "horizontal neighbors split the shared border" {
    const alloc = std.testing.allocator;
    const slots = try resolve_slots(&.{
        .{ .x = 0, .y = 0, .cols = 10, .rows = 8 },
        .{ .x = 9, .y = 0, .cols = 11, .rows = 8 },
    }, 10.0, 16.0, alloc);
    defer alloc.free(slots);
    const left = slots[0];
    const right = slots[1];
    const left_right = left.x + @as(f64, @floatFromInt(left.width));
    try std.testing.expect(@abs(right.x - left_right - 8.0) < 0.5);
    try std.testing.expect(right.x >= left_right);
    try std.testing.expect(@abs(left.y - 4.0) < 0.1);
}

test "darken mixes sixteen percent toward black" {
    const color = Rgba.opaqueRgb(.{ 100, 50, 0 });
    const darkened = darken(color, CANVAS_DARKEN);
    try std.testing.expectEqual(@as(u8, 84), darkened.r);
    try std.testing.expectEqual(@as(u8, 42), darkened.g);
    try std.testing.expectEqual(@as(u8, 0), darkened.b);
    try std.testing.expectEqual(@as(u8, 255), darkened.a);
}

test "unique border is focused" {
    const panes = [_]PaneFace{
        .{ .id = 1, .border = .{ .indexed = 8 } },
        .{ .id = 2, .border = .{ .indexed = 7 } },
        .{ .id = 3, .border = .{ .indexed = 8 } },
    };
    try std.testing.expectEqual(@as(?u64, 2), pick_focused(&panes));
}

test "matching borders are not focused" {
    const panes = [_]PaneFace{
        .{ .id = 1, .border = .{ .indexed = 8 } },
        .{ .id = 2, .border = .{ .indexed = 8 } },
    };
    try std.testing.expectEqual(@as(?u64, null), pick_focused(&panes));
}

test "saturated border wins when every colour is unique" {
    const panes = [_]PaneFace{
        .{ .id = 1, .border = .{ .rgb = .{ .r = 128, .g = 128, .b = 128 } } },
        .{ .id = 2, .border = .{ .rgb = .{ .r = 40, .g = 200, .b = 80 } } },
    };
    try std.testing.expectEqual(@as(?u64, 2), pick_focused(&panes));
}

test "a single pane is focused" {
    const panes = [_]PaneFace{.{ .id = 4, .border = .default }};
    try std.testing.expectEqual(@as(?u64, 4), pick_focused(&panes));
}

test "stylesheet describes the card and the ring" {
    const alloc = std.testing.allocator;
    const ring = Rgba.opaqueRgb(.{ 0x3d, 0xdc, 0x97 });
    const css = try card_stylesheet(ring, alloc);
    defer alloc.free(css);
    try std.testing.expect(std.mem.indexOf(u8, css, "overflow") == null);
    try std.testing.expect(std.mem.indexOf(u8, css, ".herdr-fade") != null);
    try std.testing.expect(std.mem.indexOf(u8, css, ".herdr-card-count") != null);
    try std.testing.expect(std.mem.indexOf(u8, css, "herdr-ring-3ddc97") != null);
    try std.testing.expect(std.mem.indexOf(u8, css, "0 0 0 2px #3ddc97") != null);
}
