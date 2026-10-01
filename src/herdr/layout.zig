//! Focused-tab layout reconstruction from Herdr snapshot JSON. No process spawn.

const std = @import("std");
const contract = @import("contract");

const Allocator = std.mem.Allocator;
const Value = std.json.Value;

pub const Error = error{
    LayoutRejected,
    OutOfMemory,
};

const CellRect = struct { x: u16, y: u16, width: u16, height: u16 };

const RawPane = struct { pane_id: []const u8, rect: CellRect };
const RawSplit = struct {
    id: []const u8,
    direction: contract.SplitDirection,
    ratio: contract.SplitRatio,
    rect: CellRect,
};

const ChildRef = union(enum) { pane: usize, split: usize };

/// Select the unique layout for `workspace_id`/`tab_id` and rebuild its tree.
pub fn tabLayout(
    allocator: Allocator,
    snapshot: Value,
    workspace_id: []const u8,
    tab_id: []const u8,
) Error!contract.TabLayout {
    const layouts_val = objGet(snapshot, "layouts") orelse return error.LayoutRejected;
    const layouts = switch (layouts_val) {
        .array => |a| a.items,
        else => return error.LayoutRejected,
    };
    var found: ?Value = null;
    for (layouts) |entry| {
        const wid = nonemptyString(objGet(entry, "workspace_id")) orelse continue;
        const tid = nonemptyString(objGet(entry, "tab_id")) orelse continue;
        if (std.mem.eql(u8, wid, workspace_id) and std.mem.eql(u8, tid, tab_id)) {
            if (found != null) return error.LayoutRejected;
            found = entry;
        }
    }
    const layout = found orelse return error.LayoutRejected;
    return reconstructLayout(allocator, layout, workspace_id, tab_id);
}

pub fn visiblePanes(allocator: Allocator, layout: contract.TabLayout) Error![]const []const u8 {
    var out: std.ArrayListUnmanaged([]const u8) = .empty;
    errdefer out.deinit(allocator);
    try walkPanes(allocator, &out, layout.root);
    return try out.toOwnedSlice(allocator);
}

pub fn directionToward(layout: contract.TabLayout, from: []const u8, to: []const u8) ?contract.PaneDirection {
    if (std.mem.eql(u8, from, to)) return null;
    const a = findPane(layout.panes, from) orelse return null;
    const b = findPane(layout.panes, to) orelse return null;
    const dx: i32 = @as(i32, b.x + b.width / 2) - @as(i32, a.x + a.width / 2);
    const dy: i32 = @as(i32, b.y + b.height / 2) - @as(i32, a.y + a.height / 2);
    if (dx == 0 and dy == 0) return null;
    if (@abs(dx) >= @abs(dy)) {
        return if (dx > 0) .right else .left;
    } else {
        return if (dy > 0) .down else .up;
    }
}

fn findPane(panes: []const contract.PlacedPane, id: []const u8) ?contract.PlacedPane {
    for (panes) |p| if (std.mem.eql(u8, p.pane_id, id)) return p;
    return null;
}

fn walkPanes(allocator: Allocator, out: *std.ArrayListUnmanaged([]const u8), node: contract.LayoutNode) Error!void {
    switch (node) {
        .pane => |p| try out.append(allocator, p.pane_id),
        .split => |s| {
            try walkPanes(allocator, out, s.first.*);
            try walkPanes(allocator, out, s.second.*);
        },
    }
}

fn reconstructLayout(
    allocator: Allocator,
    layout: Value,
    workspace_id: []const u8,
    tab_id: []const u8,
) Error!contract.TabLayout {
    const focused_pane_id = nonemptyString(objGet(layout, "focused_pane_id")) orelse return error.LayoutRejected;
    const zoomed_val = objGet(layout, "zoomed") orelse return error.LayoutRejected;
    const zoomed = switch (zoomed_val) {
        .bool => |b| b,
        else => return error.LayoutRejected,
    };
    const area = try parseCellRect(objGet(layout, "area") orelse return error.LayoutRejected);
    const panes = try parseLayoutPanes(allocator, layout);
    defer allocator.free(panes);

    var focused_ok = false;
    for (panes) |p| {
        if (std.mem.eql(u8, p.pane_id, focused_pane_id)) focused_ok = true;
    }
    if (!focused_ok) return error.LayoutRejected;

    const root: contract.LayoutNode = if (zoomed)
        .{ .pane = .{ .pane_id = focused_pane_id } }
    else blk: {
        const splits = try parseLayoutSplits(allocator, layout);
        defer allocator.free(splits);
        if (splits.len == 0) {
            if (panes.len != 1) return error.LayoutRejected;
            break :blk .{ .pane = .{ .pane_id = panes[0].pane_id } };
        }
        break :blk try buildSplitTree(allocator, panes, splits, area);
    };

    var placed: std.ArrayListUnmanaged(contract.PlacedPane) = .empty;
    errdefer placed.deinit(allocator);
    for (panes) |p| {
        try placed.append(allocator, .{
            .pane_id = p.pane_id,
            .x = p.rect.x,
            .y = p.rect.y,
            .width = p.rect.width,
            .height = p.rect.height,
        });
    }

    return .{
        .workspace_id = workspace_id,
        .tab_id = tab_id,
        .zoomed = zoomed,
        .focused_pane_id = focused_pane_id,
        .root = root,
        .panes = try placed.toOwnedSlice(allocator),
    };
}

fn parseLayoutPanes(allocator: Allocator, layout: Value) Error![]RawPane {
    const entries_val = objGet(layout, "panes") orelse return error.LayoutRejected;
    const entries = switch (entries_val) {
        .array => |a| a.items,
        else => return error.LayoutRejected,
    };
    var panes: std.ArrayListUnmanaged(RawPane) = .empty;
    errdefer panes.deinit(allocator);
    for (entries) |entry| {
        const pane_id = nonemptyString(objGet(entry, "pane_id")) orelse return error.LayoutRejected;
        for (panes.items) |p| if (std.mem.eql(u8, p.pane_id, pane_id)) return error.LayoutRejected;
        const rect = try parseCellRect(objGet(entry, "rect") orelse return error.LayoutRejected);
        try panes.append(allocator, .{ .pane_id = pane_id, .rect = rect });
    }
    return try panes.toOwnedSlice(allocator);
}

fn parseLayoutSplits(allocator: Allocator, layout: Value) Error![]RawSplit {
    const entries_val = objGet(layout, "splits") orelse return error.LayoutRejected;
    const entries = switch (entries_val) {
        .array => |a| a.items,
        else => return error.LayoutRejected,
    };
    var splits: std.ArrayListUnmanaged(RawSplit) = .empty;
    errdefer splits.deinit(allocator);
    for (entries) |entry| {
        const id = nonemptyString(objGet(entry, "id")) orelse return error.LayoutRejected;
        for (splits.items) |s| if (std.mem.eql(u8, s.id, id)) return error.LayoutRejected;
        const dir_s = nonemptyString(objGet(entry, "direction")) orelse return error.LayoutRejected;
        const direction: contract.SplitDirection = if (std.mem.eql(u8, dir_s, "right"))
            .right
        else if (std.mem.eql(u8, dir_s, "down"))
            .down
        else
            return error.LayoutRejected;
        const ratio_v = objGet(entry, "ratio") orelse return error.LayoutRejected;
        const ratio = asF64(ratio_v) orelse return error.LayoutRejected;
        if (!std.math.isFinite(ratio) or ratio <= 0.0 or ratio >= 1.0) return error.LayoutRejected;
        const rect = try parseCellRect(objGet(entry, "rect") orelse return error.LayoutRejected);
        try splits.append(allocator, .{
            .id = id,
            .direction = direction,
            .ratio = .{ .value = ratio },
            .rect = rect,
        });
    }
    return try splits.toOwnedSlice(allocator);
}

fn parseCellRect(value: Value) Error!CellRect {
    const field = struct {
        fn get(v: Value, name: []const u8) Error!u16 {
            const n = objGet(v, name) orelse return error.LayoutRejected;
            const u = asU64(n) orelse return error.LayoutRejected;
            if (u > std.math.maxInt(u16)) return error.LayoutRejected;
            return @intCast(u);
        }
    }.get;
    return .{
        .x = try field(value, "x"),
        .y = try field(value, "y"),
        .width = try field(value, "width"),
        .height = try field(value, "height"),
    };
}

fn rectContains(outer: CellRect, inner: CellRect) bool {
    const outer_right = @as(u32, outer.x) + outer.width;
    const outer_bottom = @as(u32, outer.y) + outer.height;
    const inner_right = @as(u32, inner.x) + inner.width;
    const inner_bottom = @as(u32, inner.y) + inner.height;
    return inner.x >= outer.x and inner.y >= outer.y and inner_right <= outer_right and inner_bottom <= outer_bottom;
}

fn rectEq(a: CellRect, b: CellRect) bool {
    return a.x == b.x and a.y == b.y and a.width == b.width and a.height == b.height;
}

fn rectArea(rect: CellRect) u32 {
    return @as(u32, rect.width) * @as(u32, rect.height);
}

fn smallestParent(splits: []const RawSplit, rect: CellRect) Error!usize {
    var best_area: ?u32 = null;
    var best_index: usize = 0;
    var ambiguous = false;
    for (splits, 0..) |split, index| {
        if (!rectContains(split.rect, rect) or rectEq(split.rect, rect)) continue;
        const area = rectArea(split.rect);
        if (best_area) |ba| {
            if (area < ba) {
                best_area = area;
                best_index = index;
                ambiguous = false;
            } else if (area == ba) {
                ambiguous = true;
            }
        } else {
            best_area = area;
            best_index = index;
            ambiguous = false;
        }
    }
    if (best_area == null) return error.LayoutRejected;
    if (ambiguous) return error.LayoutRejected;
    return best_index;
}

fn buildSplitTree(
    allocator: Allocator,
    panes: []const RawPane,
    splits: []const RawSplit,
    area: CellRect,
) Error!contract.LayoutNode {
    var roots: std.ArrayListUnmanaged(usize) = .empty;
    defer roots.deinit(allocator);
    for (splits, 0..) |split, index| {
        if (rectEq(split.rect, area)) try roots.append(allocator, index);
    }
    if (roots.items.len != 1) return error.LayoutRejected;
    const root = roots.items[0];

    var children = std.AutoHashMap(usize, std.ArrayListUnmanaged(ChildRef)).init(allocator);
    defer {
        var it = children.valueIterator();
        while (it.next()) |list| list.deinit(allocator);
        children.deinit();
    }

    for (panes, 0..) |pane, index| {
        const parent = try smallestParent(splits, pane.rect);
        const gop = try children.getOrPut(parent);
        if (!gop.found_existing) gop.value_ptr.* = .empty;
        try gop.value_ptr.append(allocator, .{ .pane = index });
    }
    for (splits, 0..) |split, index| {
        _ = split;
        if (index == root) continue;
        const parent = try smallestParent(splits, splits[index].rect);
        const gop = try children.getOrPut(parent);
        if (!gop.found_existing) gop.value_ptr.* = .empty;
        try gop.value_ptr.append(allocator, .{ .split = index });
    }

    const node = try buildSplit(allocator, root, splits, panes, &children);
    if (nodePaneCount(node) != panes.len) return error.LayoutRejected;
    return node;
}

fn buildSplit(
    allocator: Allocator,
    index: usize,
    splits: []const RawSplit,
    panes: []const RawPane,
    children: *std.AutoHashMap(usize, std.ArrayListUnmanaged(ChildRef)),
) Error!contract.LayoutNode {
    const split = splits[index];
    const kids_list = children.getPtr(index);
    const kids = if (kids_list) |l| l.items else &[_]ChildRef{};
    if (kids.len != 2) return error.LayoutRejected;

    const side = struct {
        fn get(child: ChildRef, s: RawSplit, p: []const RawPane, sp: []const RawSplit) u16 {
            const rect = switch (child) {
                .pane => |i| p[i].rect,
                .split => |i| sp[i].rect,
            };
            return switch (s.direction) {
                .right => rect.x,
                .down => rect.y,
            };
        }
    }.get;

    if (side(kids[0], split, panes, splits) == side(kids[1], split, panes, splits)) return error.LayoutRejected;
    const first_ref, const second_ref = if (side(kids[0], split, panes, splits) < side(kids[1], split, panes, splits))
        .{ kids[0], kids[1] }
    else
        .{ kids[1], kids[0] };

    const first = try buildChild(allocator, first_ref, splits, panes, children);
    const second = try buildChild(allocator, second_ref, splits, panes, children);
    const first_ptr = try allocator.create(contract.LayoutNode);
    errdefer allocator.destroy(first_ptr);
    const second_ptr = try allocator.create(contract.LayoutNode);
    errdefer allocator.destroy(second_ptr);
    first_ptr.* = first;
    second_ptr.* = second;
    return .{
        .split = .{
            .id = split.id,
            .direction = split.direction,
            .ratio = split.ratio,
            .first = first_ptr,
            .second = second_ptr,
        },
    };
}

fn buildChild(
    allocator: Allocator,
    child: ChildRef,
    splits: []const RawSplit,
    panes: []const RawPane,
    children: *std.AutoHashMap(usize, std.ArrayListUnmanaged(ChildRef)),
) Error!contract.LayoutNode {
    return switch (child) {
        .pane => |i| .{ .pane = .{ .pane_id = panes[i].pane_id } },
        .split => |i| try buildSplit(allocator, i, splits, panes, children),
    };
}

fn nodePaneCount(node: contract.LayoutNode) usize {
    return switch (node) {
        .pane => 1,
        .split => |s| nodePaneCount(s.first.*) + nodePaneCount(s.second.*),
    };
}

fn objGet(v: Value, key: []const u8) ?Value {
    return switch (v) {
        .object => |o| o.get(key),
        else => null,
    };
}

fn nonemptyString(v: ?Value) ?[]const u8 {
    const val = v orelse return null;
    return switch (val) {
        .string => |s| if (s.len == 0) null else s,
        else => null,
    };
}

fn asU64(v: Value) ?u64 {
    return switch (v) {
        .integer => |i| if (i >= 0) @intCast(i) else null,
        .float => |f| blk: {
            if (!std.math.isFinite(f) or f < 0 or f != @floor(f)) break :blk null;
            break :blk @intFromFloat(f);
        },
        else => null,
    };
}

fn asF64(v: Value) ?f64 {
    return switch (v) {
        .float => |f| f,
        .integer => |i| @floatFromInt(i),
        else => null,
    };
}

fn snapshotObject(root: Value) ?Value {
    const result = objGet(root, "result") orelse return null;
    return objGet(result, "snapshot");
}

fn loadJson(allocator: Allocator, path: []const u8) !std.json.Parsed(Value) {
    const text = try std.Io.Dir.cwd().readFileAlloc(std.testing.io, path, allocator, .limited(20 << 20));
    defer allocator.free(text);
    return try std.json.parseFromSlice(Value, allocator, text, .{});
}

test "w4:t1 maps to two panes split by split_0_root" {
    const alloc = std.testing.allocator;
    var parsed = try loadJson(alloc, "herdr-native-poc/docs/contracts/snapshot.json");
    defer parsed.deinit();
    const snap = snapshotObject(parsed.value) orelse return error.TestUnexpectedResult;
    var arena = std.heap.ArenaAllocator.init(alloc);
    defer arena.deinit();
    const layout = try tabLayout(arena.allocator(), snap, "w4", "w4:t1");
    try std.testing.expect(!layout.zoomed);
    try std.testing.expectEqualStrings("w4:p1", layout.focused_pane_id);
    switch (layout.root) {
        .split => |s| {
            try std.testing.expectEqualStrings("split_0_root", s.id);
            try std.testing.expect(s.direction == .right);
            try std.testing.expect(@abs(s.ratio.value - 0.48795182) < 1e-8);
            try std.testing.expectEqualStrings("w4:p1", s.first.pane.pane_id);
            try std.testing.expectEqualStrings("w4:p2", s.second.pane.pane_id);
        },
        else => return error.TestUnexpectedResult,
    }
}

test "w1:t1 maps to one pane" {
    const alloc = std.testing.allocator;
    var parsed = try loadJson(alloc, "herdr-native-poc/docs/contracts/snapshot.json");
    defer parsed.deinit();
    const snap = snapshotObject(parsed.value) orelse return error.TestUnexpectedResult;
    var arena = std.heap.ArenaAllocator.init(alloc);
    defer arena.deinit();
    const layout = try tabLayout(arena.allocator(), snap, "w1", "w1:t1");
    try std.testing.expect(!layout.zoomed);
    const panes = try visiblePanes(arena.allocator(), layout);
    try std.testing.expectEqual(@as(usize, 1), panes.len);
    try std.testing.expectEqualStrings("w1:p1", panes[0]);
    try std.testing.expectEqualStrings("w1:p1", layout.root.pane.pane_id);
}

test "nested fixture reconstructs parent and child splits" {
    const alloc = std.testing.allocator;
    var parsed = try loadJson(alloc, "herdr-native-poc/docs/contracts/fixtures/layout-nested.json");
    defer parsed.deinit();
    const snap = snapshotObject(parsed.value) orelse return error.TestUnexpectedResult;
    var arena = std.heap.ArenaAllocator.init(alloc);
    defer arena.deinit();
    const layout = try tabLayout(arena.allocator(), snap, "w4", "w4:t1");
    const panes = try visiblePanes(arena.allocator(), layout);
    try std.testing.expectEqual(@as(usize, 3), panes.len);
    try std.testing.expectEqualStrings("w4:p1", panes[0]);
    try std.testing.expectEqualStrings("w4:p2", panes[1]);
    try std.testing.expectEqualStrings("w4:p3", panes[2]);
    switch (layout.root) {
        .split => |s| {
            try std.testing.expectEqualStrings("split_0_root", s.id);
            try std.testing.expect(s.direction == .right);
            try std.testing.expectEqualStrings("w4:p1", s.first.pane.pane_id);
            switch (s.second.*) {
                .split => |inner| {
                    try std.testing.expectEqualStrings("split_1", inner.id);
                    try std.testing.expect(inner.direction == .down);
                    try std.testing.expectEqualStrings("w4:p2", inner.first.pane.pane_id);
                    try std.testing.expectEqualStrings("w4:p3", inner.second.pane.pane_id);
                },
                else => return error.TestUnexpectedResult,
            }
        },
        else => return error.TestUnexpectedResult,
    }
}

test "zoomed fixture maps to the focused pane only" {
    const alloc = std.testing.allocator;
    var parsed = try loadJson(alloc, "herdr-native-poc/docs/contracts/fixtures/layout-zoomed.json");
    defer parsed.deinit();
    const snap = snapshotObject(parsed.value) orelse return error.TestUnexpectedResult;
    var arena = std.heap.ArenaAllocator.init(alloc);
    defer arena.deinit();
    const layout = try tabLayout(arena.allocator(), snap, "w4", "w4:t1");
    try std.testing.expect(layout.zoomed);
    try std.testing.expectEqualStrings("w4:p1", layout.focused_pane_id);
    const panes = try visiblePanes(arena.allocator(), layout);
    try std.testing.expectEqual(@as(usize, 1), panes.len);
    try std.testing.expectEqualStrings("w4:p1", panes[0]);
    try std.testing.expectEqualStrings("w4:p1", layout.root.pane.pane_id);
}

test "layout without a unique root is rejected" {
    const alloc = std.testing.allocator;
    const json =
        \\{
        \\  "area": {"x": 0, "y": 0, "width": 10, "height": 10},
        \\  "focused_pane_id": "p1",
        \\  "zoomed": false,
        \\  "panes": [
        \\    {"pane_id": "p1", "focused": true, "rect": {"x": 0, "y": 0, "width": 10, "height": 10}}
        \\  ],
        \\  "splits": [
        \\    {"id": "a", "direction": "right", "ratio": 0.5, "rect": {"x": 0, "y": 0, "width": 9, "height": 10}},
        \\    {"id": "b", "direction": "right", "ratio": 0.5, "rect": {"x": 0, "y": 0, "width": 8, "height": 10}}
        \\  ],
        \\  "workspace_id": "w",
        \\  "tab_id": "t"
        \\}
    ;
    var parsed = try std.json.parseFromSlice(Value, alloc, json, .{});
    defer parsed.deinit();
    var arena = std.heap.ArenaAllocator.init(alloc);
    defer arena.deinit();
    try std.testing.expectError(error.LayoutRejected, reconstructLayout(arena.allocator(), parsed.value, "w", "t"));
}

test "click direction follows pane centers" {
    const alloc = std.testing.allocator;
    var nested = try loadJson(alloc, "herdr-native-poc/docs/contracts/fixtures/layout-nested.json");
    defer nested.deinit();
    const snap = snapshotObject(nested.value) orelse return error.TestUnexpectedResult;
    var arena = std.heap.ArenaAllocator.init(alloc);
    defer arena.deinit();
    const layout = try tabLayout(arena.allocator(), snap, "w4", "w4:t1");
    try std.testing.expect(directionToward(layout, "w4:p1", "w4:p2") == .right);
    try std.testing.expect(directionToward(layout, "w4:p2", "w4:p3") == .down);
    try std.testing.expect(directionToward(layout, "w4:p3", "w4:p2") == .up);
    try std.testing.expect(directionToward(layout, "w4:p2", "w4:p1") == .left);
    try std.testing.expect(directionToward(layout, "w4:p1", "w4:p1") == null);
}

test "focused tab layout ignores other workspaces" {
    const alloc = std.testing.allocator;
    var parsed = try loadJson(alloc, "herdr-native-poc/docs/contracts/snapshot.json");
    defer parsed.deinit();
    const snap = snapshotObject(parsed.value) orelse return error.TestUnexpectedResult;
    const wid = nonemptyString(objGet(snap, "focused_workspace_id")) orelse return error.TestUnexpectedResult;
    const tid = nonemptyString(objGet(snap, "focused_tab_id")) orelse return error.TestUnexpectedResult;
    var arena = std.heap.ArenaAllocator.init(alloc);
    defer arena.deinit();
    const layout = try tabLayout(arena.allocator(), snap, wid, tid);
    try std.testing.expectEqualStrings("w9", layout.workspace_id);
    try std.testing.expectEqualStrings("w9:t1", layout.tab_id);
    try std.testing.expect(!layout.zoomed);
    const panes = try visiblePanes(arena.allocator(), layout);
    try std.testing.expectEqual(@as(usize, 2), panes.len);
    try std.testing.expectEqualStrings("w9:p1", panes[0]);
    try std.testing.expectEqualStrings("w9:p2", panes[1]);
}
