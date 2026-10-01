//! Ratio split for tab layouts. The gap is chrome between cards; each pane's
//! grid is measured from its own allocation.

const std = @import("std");
const contract = @import("contract");
const gtk = @import("gtk");

pub const GAP_PX: i32 = 8;

extern fn g_object_set_data_full(
    object: *anyopaque,
    key: [*:0]const u8,
    data: ?*anyopaque,
    destroy: ?*const fn (?*anyopaque) callconv(.c) void,
) void;
extern fn g_object_get_data(object: *anyopaque, key: [*:0]const u8) ?*anyopaque;

const split_key: [*:0]const u8 = "herdr-split";

const SplitMeta = struct {
    ratio: f64,
    orientation: gtk.Orientation,
};

fn freeMeta(data: ?*anyopaque) callconv(.c) void {
    const meta: *SplitMeta = @ptrCast(@alignCast(data.?));
    std.heap.c_allocator.destroy(meta);
}

fn metaOf(widget: *gtk.Widget) ?*SplitMeta {
    const ptr = g_object_get_data(@ptrCast(widget), split_key) orelse return null;
    return @ptrCast(@alignCast(ptr));
}

/// Pixel lengths of the two children after `gap` is removed.
/// `ratio` is the first child's share of the remaining space (left or top).
pub fn splitLengths(total: i32, gap: i32, ratio: f64) struct { i32, i32 } {
    const gap_pos = @max(gap, 0);
    const inner: i32 = if (total > gap_pos) total - gap_pos else 0;
    if (inner == 0 or !std.math.isFinite(ratio)) return .{ 0, 0 };
    const first = std.math.clamp(
        @as(i32, @intFromFloat(@round(@as(f64, @floatFromInt(inner)) * ratio))),
        0,
        inner,
    );
    return .{ first, inner - first };
}

pub const RatioSplit = struct {
    widget: *gtk.Box,

    pub fn new(
        direction: contract.SplitDirection,
        ratio: f64,
        first: *gtk.Widget,
        second: *gtk.Widget,
    ) RatioSplit {
        const orient: gtk.Orientation = switch (direction) {
            .right => .horizontal,
            .down => .vertical,
        };
        const box = gtk.Box.new(orient, 0);
        const layout = gtk.CustomLayout.new(null, measure, allocate);
        box.as(gtk.Widget).setLayoutManager(layout.as(gtk.LayoutManager));

        const meta = std.heap.c_allocator.create(SplitMeta) catch @panic("oom");
        meta.* = .{ .ratio = ratio, .orientation = orient };
        g_object_set_data_full(@ptrCast(box), split_key, meta, &freeMeta);

        first.setParent(box.as(gtk.Widget));
        second.setParent(box.as(gtk.Widget));
        box.as(gtk.Widget).setHexpand(1);
        box.as(gtk.Widget).setVexpand(1);
        box.as(gtk.Widget).setOverflow(.hidden);
        return .{ .widget = box };
    }
};

/// Build a widget tree from `layout.root` (or the focused pane when zoomed).
pub fn build(
    layout: *const contract.TabLayout,
    pane: *const fn ([]const u8) *gtk.Widget,
) *gtk.Widget {
    if (layout.zoomed) return pane(layout.focused_pane_id);
    return buildNode(&layout.root, pane);
}

/// Place panes on a Fixed using PlacedPane cell rects scaled by cell pixels.
pub fn placeFixed(
    fixed: *gtk.Fixed,
    panes: []const contract.PlacedPane,
    cell_w: i32,
    cell_h: i32,
    pane: *const fn ([]const u8) *gtk.Widget,
) void {
    for (panes) |p| {
        const child = pane(p.pane_id);
        child.setSizeRequest(@as(i32, @intCast(p.width)) * cell_w, @as(i32, @intCast(p.height)) * cell_h);
        fixed.put(
            child,
            @floatFromInt(@as(i32, @intCast(p.x)) * cell_w),
            @floatFromInt(@as(i32, @intCast(p.y)) * cell_h),
        );
    }
}

fn buildNode(node: *const contract.LayoutNode, pane: *const fn ([]const u8) *gtk.Widget) *gtk.Widget {
    switch (node.*) {
        .pane => |p| return pane(p.pane_id),
        .split => |s| {
            const first = buildNode(s.first, pane);
            const second = buildNode(s.second, pane);
            return RatioSplit.new(s.direction, s.ratio.value, first, second).widget.as(gtk.Widget);
        },
    }
}


fn measure(
    widget: *gtk.Widget,
    orientation: gtk.Orientation,
    for_size: c_int,
    minimum: *c_int,
    natural: *c_int,
    minimum_baseline: *c_int,
    natural_baseline: *c_int,
) callconv(.c) void {
    _ = for_size;
    minimum_baseline.* = -1;
    natural_baseline.* = -1;
    const first = widget.getFirstChild() orelse {
        minimum.* = 0;
        natural.* = 0;
        return;
    };
    const second = first.getNextSibling() orelse {
        minimum.* = 0;
        natural.* = 0;
        return;
    };
    var first_min: c_int = 0;
    var first_nat: c_int = 0;
    var second_min: c_int = 0;
    var second_nat: c_int = 0;
    first.measure(orientation, -1, &first_min, &first_nat, null, null);
    second.measure(orientation, -1, &second_min, &second_nat, null, null);
    const orient = if (metaOf(widget)) |meta| meta.orientation else .horizontal;
    if (orientation == orient) {
        minimum.* = first_min +| second_min +| GAP_PX;
        natural.* = first_nat +| second_nat +| GAP_PX;
    } else {
        minimum.* = @max(first_min, second_min);
        natural.* = @max(first_nat, second_nat);
    }
}

fn allocate(widget: *gtk.Widget, width: c_int, height: c_int, baseline: c_int) callconv(.c) void {
    _ = baseline;
    const first = widget.getFirstChild() orelse return;
    const second = first.getNextSibling() orelse return;
    const meta = metaOf(widget);
    const orient = if (meta) |m| m.orientation else .horizontal;
    const ratio = if (meta) |m| m.ratio else 0.5;
    const along: i32 = if (orient == .horizontal) width else height;
    const lengths = splitLengths(along, GAP_PX, ratio);
    const offset = lengths[0] +| GAP_PX;
    if (orient == .horizontal) {
        var a = gtk.Allocation{ .f_x = 0, .f_y = 0, .f_width = lengths[0], .f_height = height };
        first.sizeAllocate(&a, -1);
        var b = gtk.Allocation{ .f_x = offset, .f_y = 0, .f_width = lengths[1], .f_height = height };
        second.sizeAllocate(&b, -1);
    } else {
        var a = gtk.Allocation{ .f_x = 0, .f_y = 0, .f_width = width, .f_height = lengths[0] };
        first.sizeAllocate(&a, -1);
        var b = gtk.Allocation{ .f_x = 0, .f_y = offset, .f_width = width, .f_height = lengths[1] };
        second.sizeAllocate(&b, -1);
    }
}

test "split lengths follow ratio" {
    const a = splitLengths(100, 0, 0.25);
    try std.testing.expectEqual(@as(i32, 25), a[0]);
    try std.testing.expectEqual(@as(i32, 75), a[1]);
    const b = splitLengths(161, 8, 0.48795182);
    try std.testing.expectEqual(@as(i32, 153), b[0] + b[1]);
    try std.testing.expect(@abs(@as(f64, @floatFromInt(b[0])) / 153.0 - 0.48795182) < 0.01);
}
