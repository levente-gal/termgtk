//! Pure session rules: observe fit, resize sync, observers, scroll.

const std = @import("std");
const contract = @import("contract");

pub fn resize_sync_new(grid: contract.GridSize) contract.ResizeSync {
    return .{
        .confirmed = .{
            .cols = @max(grid.cols, 1),
            .rows = @max(grid.rows, 1),
            .cell_w = grid.cell_w,
            .cell_h = grid.cell_h,
        },
        .inflight = null,
        .pending = null,
    };
}

pub fn confirmed(self: *const contract.ResizeSync) struct { u16, u16 } {
    return .{ self.confirmed.cols, self.confirmed.rows };
}

pub fn note_desired(self: *contract.ResizeSync, grid: contract.GridSize) ?contract.GridSize {
    const g: contract.GridSize = .{
        .cols = @max(grid.cols, 1),
        .rows = @max(grid.rows, 1),
        .cell_w = grid.cell_w,
        .cell_h = grid.cell_h,
    };
    if (self.inflight) |inf| {
        if (std.meta.eql(inf, g)) {
            self.pending = null;
            return null;
        }
        self.pending = g;
        return null;
    }
    if (std.meta.eql(self.confirmed, g)) {
        self.pending = null;
        return null;
    }
    self.inflight = g;
    return g;
}

pub fn on_frame(self: *contract.ResizeSync, cols: u16, rows: u16) contract.FrameFit {
    if (self.inflight) |target| {
        if (target.cols == cols and target.rows == rows) {
            self.confirmed = self.inflight.?;
            self.inflight = null;
            return .acknowledged;
        }
    }
    if (cols == self.confirmed.cols and rows == self.confirmed.rows) {
        return .use;
    }
    if (self.inflight != null) return .ignore;
    self.confirmed.cols = cols;
    self.confirmed.rows = rows;
    return .use;
}

pub fn reject_inflight(self: *contract.ResizeSync) ?contract.GridSize {
    const rejected = self.inflight;
    self.inflight = null;
    const pending = self.pending orelse return null;
    self.pending = null;
    if (rejected) |r| {
        if (std.meta.eql(pending, r)) return null;
    }
    if (std.meta.eql(pending, self.confirmed)) return null;
    self.inflight = pending;
    return pending;
}

pub fn next_pending(self: *contract.ResizeSync) ?contract.GridSize {
    if (self.inflight != null) return null;
    const pending = self.pending orelse return null;
    self.pending = null;
    if (std.meta.eql(pending, self.confirmed)) return null;
    self.inflight = pending;
    return pending;
}

pub fn classify_observe_frame(full: bool, width: u16, height: u16, cols: u16, rows: u16) contract.ObserveFit {
    if (width == cols and height == rows) return .draw;
    if (full) return .disconnect;
    return .skip;
}

pub fn observe_spawn_size(capacity: ?contract.GridSize) ?contract.GridSize {
    const grid = capacity orelse return null;
    if (grid.cols == 0 or grid.rows == 0) return null;
    return grid;
}

pub fn control_reject_is_resize(detail: []const u8) bool {
    return std.mem.indexOf(u8, detail, "resize") != null;
}

pub const ObserverDiff = struct {
    reap: []const []const u8,
    spawn: []const []const u8,
};

pub fn diff_observers(
    allocator: std.mem.Allocator,
    running: []const []const u8,
    visible: []const []const u8,
) !ObserverDiff {
    var reap: std.ArrayListUnmanaged([]const u8) = .empty;
    errdefer reap.deinit(allocator);
    var spawn: std.ArrayListUnmanaged([]const u8) = .empty;
    errdefer spawn.deinit(allocator);

    for (running) |id| {
        var found = false;
        for (visible) |v| {
            if (std.mem.eql(u8, id, v)) {
                found = true;
                break;
            }
        }
        if (!found) try reap.append(allocator, id);
    }
    for (visible) |id| {
        var found = false;
        for (running) |r| {
            if (std.mem.eql(u8, id, r)) {
                found = true;
                break;
            }
        }
        if (!found) try spawn.append(allocator, id);
    }

    const lessThan = struct {
        fn f(_: void, a: []const u8, b: []const u8) bool {
            return std.mem.order(u8, a, b) == .lt;
        }
    }.f;
    std.mem.sort([]const u8, reap.items, {}, lessThan);
    std.mem.sort([]const u8, spawn.items, {}, lessThan);

    return .{
        .reap = try reap.toOwnedSlice(allocator),
        .spawn = try spawn.toOwnedSlice(allocator),
    };
}

pub fn observe_targets(
    allocator: std.mem.Allocator,
    visible: []const []const u8,
    controlling: bool,
    focused: []const u8,
) ![]const []const u8 {
    if (!controlling) {
        const out = try allocator.alloc([]const u8, visible.len);
        @memcpy(out, visible);
        return out;
    }
    var list: std.ArrayListUnmanaged([]const u8) = .empty;
    errdefer list.deinit(allocator);
    for (visible) |id| {
        if (!std.mem.eql(u8, id, focused)) try list.append(allocator, id);
    }
    return try list.toOwnedSlice(allocator);
}

pub fn plan_observers(
    allocator: std.mem.Allocator,
    running: []const []const u8,
    visible: []const []const u8,
    controlling: bool,
    focused: []const u8,
) !ObserverDiff {
    const targets = try observe_targets(allocator, visible, controlling, focused);
    defer allocator.free(targets);
    return diff_observers(allocator, running, targets);
}

pub const FocusControl = enum { keep, release };

pub fn focus_control(controlling: bool, previous: []const u8, next: []const u8) FocusControl {
    if (controlling and !std.mem.eql(u8, previous, next)) return .release;
    return .keep;
}

pub fn push_scroll_delta(accum: *f64, delta_lines: f64) ?struct { contract.ScrollDirection, u16 } {
    if (!std.math.isFinite(delta_lines) or delta_lines == 0.0) return null;
    accum.* += delta_lines;
    const lines: i32 = @intFromFloat(@trunc(accum.*));
    if (lines == 0) return null;
    accum.* -= @floatFromInt(lines);
    const direction: contract.ScrollDirection = if (lines > 0) .down else .up;
    const abs: u32 = @intCast(if (lines > 0) lines else -lines);
    const n: u16 = @intCast(@min(@max(abs, 1), 64));
    return .{ direction, n };
}

pub fn scroll_lines_from_delta(dy: f64) ?struct { contract.ScrollDirection, u16 } {
    var accum: f64 = 0.0;
    return push_scroll_delta(&accum, dy * 3.0);
}

test "wheel delta becomes three lines" {
    try std.testing.expect(scroll_lines_from_delta(0.0) == null);
    {
        const got = scroll_lines_from_delta(1.0).?;
        try std.testing.expectEqual(contract.ScrollDirection.down, got[0]);
        try std.testing.expectEqual(@as(u16, 3), got[1]);
    }
    {
        const got = scroll_lines_from_delta(-1.0).?;
        try std.testing.expectEqual(contract.ScrollDirection.up, got[0]);
        try std.testing.expectEqual(@as(u16, 3), got[1]);
    }
    var accum: f64 = 0.0;
    try std.testing.expect(push_scroll_delta(&accum, 0.4) == null);
    try std.testing.expect(push_scroll_delta(&accum, 0.4) == null);
    const got = push_scroll_delta(&accum, 0.4).?;
    try std.testing.expectEqual(contract.ScrollDirection.down, got[0]);
    try std.testing.expectEqual(@as(u16, 1), got[1]);
    try std.testing.expect(@abs(accum - 0.2) < 1e-9); // remainder after three 0.4 pushes
}

test "observe waits for positive card capacity" {
    try std.testing.expect(observe_spawn_size(null) == null);
    try std.testing.expect(observe_spawn_size(.{ .cols = 0, .rows = 24, .cell_w = 8, .cell_h = 16 }) == null);
    try std.testing.expect(observe_spawn_size(.{ .cols = 80, .rows = 0, .cell_w = 8, .cell_h = 16 }) == null);
    const ready = observe_spawn_size(.{ .cols = 100, .rows = 40, .cell_w = 9, .cell_h = 18 }).?;
    try std.testing.expectEqual(@as(u16, 100), ready.cols);
    try std.testing.expectEqual(@as(u16, 40), ready.rows);
    try std.testing.expectEqual(@as(u32, 9), ready.cell_w);
    try std.testing.expectEqual(@as(u32, 18), ready.cell_h);
}

test "full frame mismatch disconnects without drawing" {
    try std.testing.expectEqual(contract.ObserveFit.disconnect, classify_observe_frame(true, 80, 24, 100, 40));
    try std.testing.expectEqual(contract.ObserveFit.skip, classify_observe_frame(false, 80, 24, 100, 40));
    try std.testing.expectEqual(contract.ObserveFit.draw, classify_observe_frame(true, 100, 40, 100, 40));
    try std.testing.expectEqual(contract.ObserveFit.draw, classify_observe_frame(false, 100, 40, 100, 40));
}

test "resize sync keeps one inflight and confirmed size" {
    var sync = resize_sync_new(.{ .cols = 80, .rows = 24, .cell_w = 8, .cell_h = 16 });
    try std.testing.expect(note_desired(&sync, .{ .cols = 80, .rows = 24, .cell_w = 8, .cell_h = 16 }) == null);
    const sent = note_desired(&sync, .{ .cols = 100, .rows = 30, .cell_w = 8, .cell_h = 16 }).?;
    try std.testing.expectEqual(@as(u16, 100), sent.cols);
    try std.testing.expectEqual(@as(u16, 30), sent.rows);
    try std.testing.expect(note_desired(&sync, .{ .cols = 110, .rows = 40, .cell_w = 8, .cell_h = 16 }) == null);
    try std.testing.expectEqual(contract.FrameFit.use, on_frame(&sync, 80, 24));
    {
        const c = confirmed(&sync);
        try std.testing.expectEqual(@as(u16, 80), c[0]);
        try std.testing.expectEqual(@as(u16, 24), c[1]);
    }
    try std.testing.expectEqual(contract.FrameFit.ignore, on_frame(&sync, 90, 20));
    {
        const c = confirmed(&sync);
        try std.testing.expectEqual(@as(u16, 80), c[0]);
        try std.testing.expectEqual(@as(u16, 24), c[1]);
    }
    try std.testing.expectEqual(contract.FrameFit.acknowledged, on_frame(&sync, 100, 30));
    {
        const c = confirmed(&sync);
        try std.testing.expectEqual(@as(u16, 100), c[0]);
        try std.testing.expectEqual(@as(u16, 30), c[1]);
    }
    const next = next_pending(&sync).?;
    try std.testing.expectEqual(@as(u16, 110), next.cols);
    try std.testing.expectEqual(@as(u16, 40), next.rows);
    try std.testing.expect(reject_inflight(&sync) == null);
    {
        const c = confirmed(&sync);
        try std.testing.expectEqual(@as(u16, 100), c[0]);
        try std.testing.expectEqual(@as(u16, 30), c[1]);
    }
    try std.testing.expectEqual(contract.FrameFit.use, on_frame(&sync, 100, 30));
    const pixel_only = note_desired(&sync, .{ .cols = 100, .rows = 30, .cell_w = 16, .cell_h = 32 }).?;
    try std.testing.expectEqual(@as(u16, 100), pixel_only.cols);
    try std.testing.expectEqual(@as(u16, 30), pixel_only.rows);
    try std.testing.expectEqual(contract.FrameFit.acknowledged, on_frame(&sync, 100, 30));
    try std.testing.expect(control_reject_is_resize("terminal.resize cols and rows must be greater than 0"));
    try std.testing.expect(!control_reject_is_resize("terminal.input accepts text or bytes, not both"));
}

test "focused pane change releases control" {
    try std.testing.expectEqual(FocusControl.release, focus_control(true, "w4:p1", "w4:p2"));
    try std.testing.expectEqual(FocusControl.keep, focus_control(true, "w4:p1", "w4:p1"));
    try std.testing.expectEqual(FocusControl.keep, focus_control(false, "w4:p1", "w4:p2"));

    const alloc = std.testing.allocator;
    const visible = [_][]const u8{ "w4:p1", "w4:p2" };
    const diff = try plan_observers(alloc, &.{}, &visible, false, "w4:p2");
    defer {
        alloc.free(diff.reap);
        alloc.free(diff.spawn);
    }
    try std.testing.expectEqual(@as(usize, 2), diff.spawn.len);
    try std.testing.expectEqualSlices(u8, "w4:p1", diff.spawn[0]);
    try std.testing.expectEqualSlices(u8, "w4:p2", diff.spawn[1]);
    try std.testing.expectEqual(@as(usize, 0), diff.reap.len);
}

test "zoom transition reaps hidden observers" {
    const alloc = std.testing.allocator;
    const running = [_][]const u8{ "w4:p1", "w4:p2" };
    const visible = [_][]const u8{"w4:p1"};
    const diff = try plan_observers(alloc, &running, &visible, false, "w4:p1");
    defer {
        alloc.free(diff.reap);
        alloc.free(diff.spawn);
    }
    try std.testing.expectEqual(@as(usize, 1), diff.reap.len);
    try std.testing.expectEqualSlices(u8, "w4:p2", diff.reap[0]);
    try std.testing.expectEqual(@as(usize, 0), diff.spawn.len);
}

test "takeover reaps focused observe target only" {
    const alloc = std.testing.allocator;
    const running = [_][]const u8{ "w4:p1", "w4:p2" };
    const visible = [_][]const u8{ "w4:p1", "w4:p2" };
    const diff = try plan_observers(alloc, &running, &visible, true, "w4:p1");
    defer {
        alloc.free(diff.reap);
        alloc.free(diff.spawn);
    }
    try std.testing.expectEqual(@as(usize, 1), diff.reap.len);
    try std.testing.expectEqualSlices(u8, "w4:p1", diff.reap[0]);
    try std.testing.expectEqual(@as(usize, 0), diff.spawn.len);
}
