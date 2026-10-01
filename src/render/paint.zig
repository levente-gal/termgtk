//! Cell grouping, colour resolution, snap math, box strokes, letterbox. No GTK.

const std = @import("std");
const contract = @import("contract");

pub const DEFAULT_FG: [3]u8 = .{ 0xe8, 0xe8, 0xea };
const DEFAULT_BG_SOLID: [3]u8 = .{ 0x12, 0x12, 0x14 };

pub const Run = struct {
    cells: []const contract.Cell,
};

pub const Rgba = struct {
    r: u8,
    g: u8,
    b: u8,
    a: u8,

    pub const transparent: Rgba = .{ .r = 0, .g = 0, .b = 0, .a = 0 };

    pub fn opaqueRgb(rgb: [3]u8) Rgba {
        return .{ .r = rgb[0], .g = rgb[1], .b = rgb[2], .a = 255 };
    }

    pub fn isVisible(self: Rgba) bool {
        return self.a > 0;
    }
};

pub const BoxStroke = struct {
    x: f64,
    y: f64,
    width: f64,
    height: f64,
};

/// Leading cell under a visible cursor; wide-tail maps to that leading cell.
/// Caller skips paint when this position is overlay-suppressed.
pub fn cursorPaintCell(cursor: contract.Cursor) ?struct { x: u16, y: u16 } {
    if (!cursor.visible) return null;
    return .{
        .x = if (cursor.wide_tail) cursor.x -| 1 else cursor.x,
        .y = cursor.y,
    };
}

/// Group same-row cells that share fg/bg/style. Skips wide continuations
/// (including soft-wrap spacer heads stored as wide_continuation).
pub fn groupRuns(allocator: std.mem.Allocator, cells: []const contract.Cell) ![]Run {
    var runs: std.ArrayList(Run) = .empty;
    errdefer runs.deinit(allocator);
    var start: usize = 0;
    var i: usize = 0;
    while (i < cells.len) {
        if (cells[i].occupancy == .wide_continuation) {
            if (start < i) try runs.append(allocator, .{ .cells = cells[start..i] });
            i += 1;
            start = i;
            continue;
        }
        if (i > start) {
            const prev = cells[i - 1];
            const cur = cells[i];
            if (cur.y != prev.y or !std.meta.eql(prev.fg, cur.fg) or !std.meta.eql(prev.bg, cur.bg) or !std.meta.eql(prev.style, cur.style)) {
                try runs.append(allocator, .{ .cells = cells[start..i] });
                start = i;
            }
        }
        i += 1;
    }
    if (start < cells.len) try runs.append(allocator, .{ .cells = cells[start..] });
    return try runs.toOwnedSlice(allocator);
}

/// Same grouping including wide continuations so backgrounds cover wide spans.
pub fn groupBackgroundRuns(allocator: std.mem.Allocator, cells: []const contract.Cell) ![]Run {
    var runs: std.ArrayList(Run) = .empty;
    errdefer runs.deinit(allocator);
    var start: usize = 0;
    var i: usize = 0;
    while (i < cells.len) {
        if (i > start) {
            const prev = cells[i - 1];
            const cur = cells[i];
            if (cur.y != prev.y or !std.meta.eql(prev.fg, cur.fg) or !std.meta.eql(prev.bg, cur.bg) or !std.meta.eql(prev.style, cur.style)) {
                try runs.append(allocator, .{ .cells = cells[start..i] });
                start = i;
            }
        }
        i += 1;
    }
    if (start < cells.len) try runs.append(allocator, .{ .cells = cells[start..] });
    return try runs.toOwnedSlice(allocator);
}

pub fn deviceColsRows(width_px: f64, height_px: f64, cell_w: f64, cell_h: f64, scale: f64) struct { u16, u16 } {
    const s = if (std.math.isFinite(scale) and scale > 0.0) scale else 1.0;
    const snap = struct {
        fn go(value: f64, cell: f64, sc: f64) u16 {
            if (!std.math.isFinite(cell) or cell <= 0.0 or !std.math.isFinite(value) or value <= 0.0) return 1;
            const logical = @round(value * sc) / sc;
            return @intFromFloat(@max(@floor(logical / cell), 1.0));
        }
    }.go;
    return .{ snap(width_px, cell_w, s), snap(height_px, cell_h, s) };
}

pub fn snapCellPx(logical: f64, scale: f64) f64 {
    const s = if (std.math.isFinite(scale) and scale > 0.0) scale else 1.0;
    if (!std.math.isFinite(logical) or logical <= 0.0) return 1.0 / s;
    return @max(@round(logical * s), 1.0) / s;
}

pub fn snapRect(x: f64, y: f64, w: f64, h: f64, scale: f64) struct { f64, f64, f64, f64 } {
    const s = if (std.math.isFinite(scale) and scale > 0.0) scale else 1.0;
    const left = @round(x * s) / s;
    const top = @round(y * s) / s;
    const right = @round((x + w) * s) / s;
    const bottom = @round((y + h) * s) / s;
    return .{ left, top, @max(right - left, 1.0 / s), @max(bottom - top, 1.0 / s) };
}

pub fn cellBand(x: u16, y: u16, cols: u16, cell_w: f64, cell_h: f64) struct { f64, f64, f64, f64 } {
    return .{
        @as(f64, @floatFromInt(x)) * cell_w,
        @as(f64, @floatFromInt(y)) * cell_h,
        @as(f64, @floatFromInt(@max(cols, 1))) * cell_w,
        cell_h,
    };
}

pub fn letterboxOrigin(
    view_w: f64,
    view_h: f64,
    cell_w: f64,
    cell_h: f64,
    snap_cols: u16,
    snap_rows: u16,
    cap_cols: u16,
    cap_rows: u16,
) ?struct { f64, f64 } {
    if (snap_cols > cap_cols or snap_rows > cap_rows) return null;
    if (!std.math.isFinite(view_w) or view_w <= 0.0 or !std.math.isFinite(view_h) or view_h <= 0.0) return null;
    if (!std.math.isFinite(cell_w) or cell_w <= 0.0 or !std.math.isFinite(cell_h) or cell_h <= 0.0) return null;
    const grid_w = @as(f64, @floatFromInt(snap_cols)) * cell_w;
    const grid_h = @as(f64, @floatFromInt(snap_rows)) * cell_h;
    return .{ (view_w - grid_w) / 2.0, (view_h - grid_h) / 2.0 };
}

/// Cell under a pointer in widget coordinates, clamped to the painted grid.
pub fn cellUnderPointer(
    local_x: f64,
    local_y: f64,
    view_w: f64,
    view_h: f64,
    cell_w: f64,
    cell_h: f64,
    cols: u16,
    rows: u16,
    cap_cols: u16,
    cap_rows: u16,
) ?struct { u16, u16 } {
    if (cols == 0 or rows == 0 or !std.math.isFinite(local_x) or !std.math.isFinite(local_y)) return null;
    const origin = letterboxOrigin(view_w, view_h, cell_w, cell_h, cols, rows, cap_cols, cap_rows) orelse return null;
    const col = @floor((local_x - origin[0]) / cell_w);
    const row = @floor((local_y - origin[1]) / cell_h);
    if (!std.math.isFinite(col) or !std.math.isFinite(row)) return null;
    return .{
        @intFromFloat(std.math.clamp(col, 0, @as(f64, @floatFromInt(cols -| 1)))),
        @intFromFloat(std.math.clamp(row, 0, @as(f64, @floatFromInt(rows -| 1)))),
    };
}

pub fn resolveFg(color: contract.Color) Rgba {
    return switch (color) {
        .default => Rgba.opaqueRgb(DEFAULT_FG),
        .indexed => |index| Rgba.opaqueRgb(indexedRgb(index)),
        .rgb => |c| Rgba.opaqueRgb(.{ c.r, c.g, c.b }),
    };
}

pub fn resolveBg(color: contract.Color) Rgba {
    return switch (color) {
        .default => Rgba.transparent,
        .indexed => |index| Rgba.opaqueRgb(indexedRgb(index)),
        .rgb => |c| Rgba.opaqueRgb(.{ c.r, c.g, c.b }),
    };
}

/// Applies reverse video. Bold/italic/underline are style flags for the text
/// painter; faint/blink/strikethrough/overline/underline-colour are absent.
pub fn paintColors(fg: contract.Color, bg: contract.Color, style: contract.Style) struct { Rgba, Rgba } {
    var fg_rgba = resolveFg(fg);
    var bg_rgba = resolveBg(bg);
    if (style.reverse) {
        const tmp = fg_rgba;
        fg_rgba = bg_rgba;
        bg_rgba = tmp;
        if (fg_rgba.a == 0) fg_rgba = Rgba.opaqueRgb(DEFAULT_BG_SOLID);
        if (bg_rgba.a == 0) bg_rgba = Rgba.opaqueRgb(DEFAULT_FG);
    }
    return .{ fg_rgba, bg_rgba };
}

pub fn runText(allocator: std.mem.Allocator, run: Run) ![]u8 {
    var out: std.ArrayList(u8) = .empty;
    errdefer out.deinit(allocator);
    for (run.cells) |cell| {
        if (cell.occupancy == .wide_continuation) continue;
        try out.appendSlice(allocator, cell.grapheme);
    }
    return try out.toOwnedSlice(allocator);
}

pub fn glyphsFullySuppressed(run: Run, suppressed: []const struct { u16, u16 }) bool {
    var saw_glyph = false;
    for (run.cells) |cell| {
        if (cell.occupancy == .wide_continuation) continue;
        saw_glyph = true;
        var found = false;
        for (suppressed) |xy| {
            if (xy[0] == cell.x and xy[1] == cell.y) {
                found = true;
                break;
            }
        }
        if (!found) return false;
    }
    return saw_glyph;
}

pub fn dominantOpaqueBg(cells: []const contract.Cell) ?Rgba {
    var counts: [64]struct { color: Rgba, count: usize, first: usize } = undefined;
    var n: usize = 0;
    for (cells, 0..) |cell, index| {
        const bg = paintColors(cell.fg, cell.bg, cell.style)[1];
        if (!bg.isVisible()) continue;
        var i: usize = 0;
        while (i < n) : (i += 1) {
            if (std.meta.eql(counts[i].color, bg)) {
                counts[i].count += 1;
                break;
            }
        }
        if (i == n) {
            if (n >= counts.len) continue;
            counts[n] = .{ .color = bg, .count = 1, .first = index };
            n += 1;
        }
    }
    if (n == 0) return null;
    var win = counts[0];
    var i: usize = 1;
    while (i < n) : (i += 1) {
        if (counts[i].count > win.count or (counts[i].count == win.count and counts[i].first < win.first))
            win = counts[i];
    }
    return win.color;
}

pub fn panelBorderRgba(cells: []const contract.Cell, cols: u16, x: u16, y: u16) Rgba {
    const idx = @as(usize, y) * @as(usize, cols) + @as(usize, x);
    if (idx >= cells.len) return Rgba.opaqueRgb(DEFAULT_FG);
    return resolveFg(cells[idx].fg);
}

pub fn boxCellStrokes(grapheme: []const u8, cell_w: f64, cell_h: f64, scale: f64, out: *[8]BoxStroke) ?usize {
    const kind = boxKind(grapheme) orelse return null;
    if (!std.math.isFinite(cell_w) or cell_w <= 0.0 or !std.math.isFinite(cell_h) or cell_h <= 0.0) return null;
    const s = if (std.math.isFinite(scale) and scale > 0.0) scale else 1.0;
    const thin = 1.0 / s;
    var raw: [8]BoxStroke = undefined;
    var raw_n: usize = 0;
    switch (kind.weight) {
        .light => pushArms(&raw, &raw_n, cell_w, cell_h, thin, 0.0, 0.0, kind.arms),
        .heavy => pushArms(&raw, &raw_n, cell_w, cell_h, thin * 2.0, 0.0, 0.0, kind.arms),
        .double => {
            pushArms(&raw, &raw_n, cell_w, cell_h, thin, -thin, -thin, kind.arms);
            pushArms(&raw, &raw_n, cell_w, cell_h, thin, thin, thin, kind.arms);
        },
    }
    var n: usize = 0;
    for (raw[0..raw_n]) |stroke| {
        const snapped = snapRect(stroke.x, stroke.y, stroke.width, stroke.height, s);
        out[n] = .{ .x = snapped[0], .y = snapped[1], .width = snapped[2], .height = snapped[3] };
        n += 1;
    }
    return n;
}

fn indexedRgb(index: u8) [3]u8 {
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
            const cube = struct {
                fn c(v: u8) u8 {
                    return if (v == 0) 0 else 55 + 40 * v;
                }
            }.c;
            break :blk .{ cube(r), cube(g), cube(b) };
        },
        232...255 => blk: {
            const level: u8 = @intCast(8 + (@as(u16, index) - 232) * 10);
            break :blk .{ level, level, level };
        },
    };
}

const ARM_LEFT: u8 = 1;
const ARM_RIGHT: u8 = 2;
const ARM_UP: u8 = 4;
const ARM_DOWN: u8 = 8;

const BoxWeight = enum { light, heavy, double };

fn boxKind(grapheme: []const u8) ?struct { weight: BoxWeight, arms: u8 } {
    const table = [_]struct { []const u8, BoxWeight, u8 }{
        .{ "─", .light, ARM_LEFT | ARM_RIGHT },
        .{ "│", .light, ARM_UP | ARM_DOWN },
        .{ "┌", .light, ARM_RIGHT | ARM_DOWN },
        .{ "┐", .light, ARM_LEFT | ARM_DOWN },
        .{ "└", .light, ARM_RIGHT | ARM_UP },
        .{ "┘", .light, ARM_LEFT | ARM_UP },
        .{ "├", .light, ARM_RIGHT | ARM_UP | ARM_DOWN },
        .{ "┤", .light, ARM_LEFT | ARM_UP | ARM_DOWN },
        .{ "┬", .light, ARM_LEFT | ARM_RIGHT | ARM_DOWN },
        .{ "┴", .light, ARM_LEFT | ARM_RIGHT | ARM_UP },
        .{ "┼", .light, ARM_LEFT | ARM_RIGHT | ARM_UP | ARM_DOWN },
        .{ "╭", .light, ARM_RIGHT | ARM_DOWN },
        .{ "╮", .light, ARM_LEFT | ARM_DOWN },
        .{ "╯", .light, ARM_LEFT | ARM_UP },
        .{ "╰", .light, ARM_RIGHT | ARM_UP },
        .{ "━", .heavy, ARM_LEFT | ARM_RIGHT },
        .{ "┃", .heavy, ARM_UP | ARM_DOWN },
        .{ "┏", .heavy, ARM_RIGHT | ARM_DOWN },
        .{ "┓", .heavy, ARM_LEFT | ARM_DOWN },
        .{ "┗", .heavy, ARM_RIGHT | ARM_UP },
        .{ "┛", .heavy, ARM_LEFT | ARM_UP },
        .{ "┣", .heavy, ARM_RIGHT | ARM_UP | ARM_DOWN },
        .{ "┫", .heavy, ARM_LEFT | ARM_UP | ARM_DOWN },
        .{ "┳", .heavy, ARM_LEFT | ARM_RIGHT | ARM_DOWN },
        .{ "┻", .heavy, ARM_LEFT | ARM_RIGHT | ARM_UP },
        .{ "╋", .heavy, ARM_LEFT | ARM_RIGHT | ARM_UP | ARM_DOWN },
        .{ "═", .double, ARM_LEFT | ARM_RIGHT },
        .{ "║", .double, ARM_UP | ARM_DOWN },
        .{ "╔", .double, ARM_RIGHT | ARM_DOWN },
        .{ "╗", .double, ARM_LEFT | ARM_DOWN },
        .{ "╚", .double, ARM_RIGHT | ARM_UP },
        .{ "╝", .double, ARM_LEFT | ARM_UP },
        .{ "╠", .double, ARM_RIGHT | ARM_UP | ARM_DOWN },
        .{ "╣", .double, ARM_LEFT | ARM_UP | ARM_DOWN },
        .{ "╦", .double, ARM_LEFT | ARM_RIGHT | ARM_DOWN },
        .{ "╩", .double, ARM_LEFT | ARM_RIGHT | ARM_UP },
        .{ "╬", .double, ARM_LEFT | ARM_RIGHT | ARM_UP | ARM_DOWN },
    };
    for (table) |row| {
        if (std.mem.eql(u8, grapheme, row[0])) return .{ .weight = row[1], .arms = row[2] };
    }
    return null;
}

fn pushClamped(out: *[8]BoxStroke, n: *usize, x: f64, y: f64, w: f64, h: f64, cell_w: f64, cell_h: f64) void {
    const x1 = @max(x, 0.0);
    const y1 = @max(y, 0.0);
    const x2 = @max(@min(x + w, cell_w), x1);
    const y2 = @max(@min(y + h, cell_h), y1);
    if (x2 > x1 and y2 > y1 and n.* < out.len) {
        out[n.*] = .{ .x = x1, .y = y1, .width = x2 - x1, .height = y2 - y1 };
        n.* += 1;
    }
}

fn pushArms(out: *[8]BoxStroke, n: *usize, cell_w: f64, cell_h: f64, thickness: f64, x_shift: f64, y_shift: f64, arms: u8) void {
    const cx = cell_w * 0.5 + x_shift;
    const cy = cell_h * 0.5 + y_shift;
    const x0 = cx - thickness * 0.5;
    const y0 = cy - thickness * 0.5;
    const left = arms & ARM_LEFT != 0;
    const right = arms & ARM_RIGHT != 0;
    const up = arms & ARM_UP != 0;
    const down = arms & ARM_DOWN != 0;
    if (left or right) {
        const pair: struct { f64, f64 } = if (left and right)
            .{ 0.0, cell_w }
        else if (right)
            .{ x0, cell_w - x0 }
        else
            .{ 0.0, x0 + thickness };
        pushClamped(out, n, pair[0], y0, pair[1], thickness, cell_w, cell_h);
    }
    if (up or down) {
        const pair: struct { f64, f64 } = if (up and down)
            .{ 0.0, cell_h }
        else if (down)
            .{ y0, cell_h - y0 }
        else
            .{ 0.0, y0 + thickness };
        pushClamped(out, n, x0, pair[0], thickness, pair[1], cell_w, cell_h);
    }
}

fn blankCell(x: u16, y: u16) contract.Cell {
    return .{
        .x = x,
        .y = y,
        .grapheme = " ",
        .fg = .default,
        .bg = .default,
        .style = .{},
        .occupancy = .single,
    };
}

fn devicePx(logical: f64, scale: f64) i64 {
    return @intFromFloat(@round(logical * scale));
}

test "grid capacity clamps and snaps" {
    try std.testing.expectEqual(@as(u16, 1), deviceColsRows(0.0, 10.0, 8.0, 16.0, 1.0)[0]);
    try std.testing.expectEqual(@as(u16, 1), deviceColsRows(0.0, 10.0, 8.0, 16.0, 1.0)[1]);
    try std.testing.expectEqual(@as(u16, 10), deviceColsRows(80.0, 32.0, 8.0, 16.0, 1.0)[0]);
    try std.testing.expectEqual(@as(u16, 2), deviceColsRows(80.0, 32.0, 8.0, 16.0, 1.0)[1]);
    try std.testing.expectEqual(@as(u16, 10), deviceColsRows(100.0, 40.0, 10.0, 20.0, 1.25)[0]);
    try std.testing.expectEqual(@as(u16, 2), deviceColsRows(100.0, 40.0, 10.0, 20.0, 1.25)[1]);
    try std.testing.expectEqual(@as(u16, 10), deviceColsRows(80.0, 40.0, 8.0, 20.0, 2.0)[0]);
    try std.testing.expectEqual(@as(u16, 2), deviceColsRows(80.0, 40.0, 8.0, 20.0, 2.0)[1]);
}

test "blank cells group without dropping" {
    const cells = [_]contract.Cell{ blankCell(0, 0), blankCell(1, 0) };
    const runs = try groupRuns(std.testing.allocator, &cells);
    defer std.testing.allocator.free(runs);
    try std.testing.expectEqual(@as(usize, 1), runs.len);
    try std.testing.expectEqual(@as(usize, 2), runs[0].cells.len);
}

test "group runs splits on style and row" {
    var grid: [6]contract.Cell = undefined;
    var y: u16 = 0;
    while (y < 2) : (y += 1) {
        var x: u16 = 0;
        while (x < 3) : (x += 1) {
            var cell = blankCell(x, y);
            if (y == 0 and x < 2) cell.fg = .{ .indexed = 1 } else if (y == 0) cell.fg = .{ .indexed = 2 } else cell.fg = .{ .indexed = 1 };
            grid[@as(usize, y) * 3 + @as(usize, x)] = cell;
        }
    }
    const runs = try groupRuns(std.testing.allocator, &grid);
    defer std.testing.allocator.free(runs);
    try std.testing.expectEqual(@as(usize, 3), runs.len);
    try std.testing.expectEqual(@as(usize, 2), runs[0].cells.len);
    try std.testing.expectEqual(@as(usize, 1), runs[1].cells.len);
    try std.testing.expectEqual(@as(usize, 3), runs[2].cells.len);
}

test "group runs skips wide continuation" {
    const cells = [_]contract.Cell{
        .{ .x = 0, .y = 0, .grapheme = "你", .fg = .{ .indexed = 7 }, .bg = .default, .style = .{}, .occupancy = .wide_leading },
        .{ .x = 1, .y = 0, .grapheme = "", .fg = .{ .indexed = 7 }, .bg = .default, .style = .{}, .occupancy = .wide_continuation },
        .{ .x = 2, .y = 0, .grapheme = "x", .fg = .{ .indexed = 7 }, .bg = .default, .style = .{}, .occupancy = .single },
    };
    const text_runs = try groupRuns(std.testing.allocator, &cells);
    defer std.testing.allocator.free(text_runs);
    try std.testing.expectEqual(@as(usize, 2), text_runs.len);
    try std.testing.expectEqual(@as(usize, 1), text_runs[0].cells.len);
    const text = try runText(std.testing.allocator, text_runs[0]);
    defer std.testing.allocator.free(text);
    try std.testing.expectEqualStrings("你", text);

    const bg_runs = try groupBackgroundRuns(std.testing.allocator, &cells);
    defer std.testing.allocator.free(bg_runs);
    try std.testing.expectEqual(@as(usize, 1), bg_runs.len);
    try std.testing.expectEqual(@as(usize, 3), bg_runs[0].cells.len);
}

test "indexed and default colours resolve" {
    try std.testing.expect(std.meta.eql(resolveFg(.default), Rgba.opaqueRgb(DEFAULT_FG)));
    try std.testing.expect(!resolveBg(.default).isVisible());
    try std.testing.expect(std.meta.eql(resolveFg(.{ .indexed = 1 }), Rgba.opaqueRgb(.{ 0xcd, 0x00, 0x00 })));
    try std.testing.expect(std.meta.eql(resolveFg(.{ .indexed = 196 }), Rgba.opaqueRgb(.{ 0xff, 0x00, 0x00 })));
    try std.testing.expect(std.meta.eql(resolveFg(.{ .indexed = 232 }), Rgba.opaqueRgb(.{ 8, 8, 8 })));
    try std.testing.expect(std.meta.eql(resolveFg(.{ .rgb = .{ .r = 1, .g = 2, .b = 3 } }), Rgba.opaqueRgb(.{ 1, 2, 3 })));
    const pair = paintColors(.{ .indexed = 2 }, .default, .{ .reverse = true });
    try std.testing.expect(pair[1].isVisible());
    try std.testing.expect(std.meta.eql(pair[1], Rgba.opaqueRgb(.{ 0x00, 0xcd, 0x00 })));
    try std.testing.expect(std.meta.eql(pair[0], Rgba.opaqueRgb(DEFAULT_BG_SOLID)));
}

test "cursor visible wide-tail maps to leading" {
    try std.testing.expect(cursorPaintCell(.{ .x = 2, .y = 1, .visible = false, .wide_tail = false }) == null);
    const c = cursorPaintCell(.{ .x = 2, .y = 1, .visible = true, .wide_tail = true }).?;
    try std.testing.expectEqual(@as(u16, 1), c.x);
    try std.testing.expectEqual(@as(u16, 1), c.y);
}

test "snapped rows share an edge at scale 1 1.25 2" {
    for ([_]f64{ 1.0, 1.25, 2.0 }) |scale| {
        const cell_w = snapCellPx(7.3, scale);
        const cell_h = snapCellPx(16.4, scale);
        const r0 = snapRect(0.0, 0.0, cell_w * 4.0, cell_h, scale);
        const r1 = snapRect(0.0, cell_h, cell_w * 4.0, cell_h, scale);
        try std.testing.expectEqual(devicePx(r0[1] + r0[3], scale), devicePx(r1[1], scale));
        const band0 = cellBand(0, 0, 2, cell_w, cell_h);
        const band1 = cellBand(2, 0, 2, cell_w, cell_h);
        const snapped = snapRect(band0[0], 0.0, band0[2], cell_h, scale);
        const right = snapRect(band1[0], 0.0, cell_w, cell_h, scale);
        try std.testing.expectEqual(devicePx(band0[0] + snapped[2], scale), devicePx(right[0], scale));
    }
}

test "letterbox centers or refuses" {
    const o = letterboxOrigin(100, 80, 8, 16, 10, 4, 12, 5).?;
    try std.testing.expectEqual(@as(f64, 10.0), o[0]);
    try std.testing.expectEqual(@as(f64, 8.0), o[1]);
    try std.testing.expect(letterboxOrigin(100, 80, 8, 16, 20, 4, 12, 5) == null);
}

test "cell under pointer uses letterbox origin" {
    const cell = cellUnderPointer(10.0 + 8.0, 8.0 + 16.0, 100, 80, 8, 16, 10, 4, 12, 5).?;
    try std.testing.expectEqual(@as(u16, 1), cell[0]);
    try std.testing.expectEqual(@as(u16, 1), cell[1]);
    try std.testing.expect(cellUnderPointer(0, 0, 100, 80, 8, 16, 20, 4, 12, 5) == null);
}

test "box strokes continuous left edge" {
    const scale: f64 = 2.0;
    const cell_w = snapCellPx(8.4, scale);
    const cell_h = snapCellPx(17.2, scale);
    var buf: [8]BoxStroke = undefined;
    const n_tl = boxCellStrokes("╭", cell_w, cell_h, scale, &buf).?;
    var top = buf[0];
    for (buf[0..n_tl]) |s| {
        if (s.height > s.width) top = s;
    }
    const n_mid = boxCellStrokes("│", cell_w, cell_h, scale, &buf).?;
    var mid = buf[0];
    for (buf[0..n_mid]) |s| {
        if (s.height > s.width) mid = s;
    }
    mid.y += cell_h;
    try std.testing.expectEqual(devicePx(top.x, scale), devicePx(mid.x, scale));
    try std.testing.expectEqual(devicePx(top.y + top.height, scale), devicePx(mid.y, scale));
    try std.testing.expect(boxCellStrokes("h", cell_w, cell_h, scale, &buf) == null);
}

test "dominant opaque bg prefers common then earlier" {
    const cells = [_]contract.Cell{
        .{ .x = 0, .y = 0, .grapheme = " ", .fg = .default, .bg = .{ .rgb = .{ .r = 250, .g = 250, .b = 250 } }, .style = .{}, .occupancy = .single },
        .{ .x = 1, .y = 0, .grapheme = " ", .fg = .default, .bg = .{ .rgb = .{ .r = 20, .g = 20, .b = 24 } }, .style = .{}, .occupancy = .single },
        .{ .x = 0, .y = 1, .grapheme = " ", .fg = .default, .bg = .{ .rgb = .{ .r = 250, .g = 250, .b = 250 } }, .style = .{}, .occupancy = .single },
        .{ .x = 1, .y = 1, .grapheme = " ", .fg = .default, .bg = .default, .style = .{}, .occupancy = .single },
    };
    try std.testing.expect(std.meta.eql(dominantOpaqueBg(&cells).?, Rgba.opaqueRgb(.{ 250, 250, 250 })));
    const tied = [_]contract.Cell{
        .{ .x = 0, .y = 0, .grapheme = " ", .fg = .default, .bg = .{ .rgb = .{ .r = 1, .g = 0, .b = 0 } }, .style = .{}, .occupancy = .single },
        .{ .x = 1, .y = 0, .grapheme = " ", .fg = .default, .bg = .{ .rgb = .{ .r = 0, .g = 0, .b = 1 } }, .style = .{}, .occupancy = .single },
    };
    try std.testing.expect(std.meta.eql(dominantOpaqueBg(&tied).?, Rgba.opaqueRgb(.{ 1, 0, 0 })));
}
