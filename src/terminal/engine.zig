const std = @import("std");
const ghostty_vt = @import("ghostty-vt");
const contract = @import("contract");

const Allocator = std.mem.Allocator;

/// Ghostty VT engine: one Terminal + one Stream for the engine lifetime.
pub const Engine = struct {
    alloc: Allocator,
    terminal: *ghostty_vt.Terminal,
    stream: ghostty_vt.TerminalStream,
    render: ghostty_vt.RenderState,
    cols: u16,
    rows: u16,

    pub fn init(cols: u16, rows: u16) !Engine {
        const alloc = std.heap.page_allocator;
        const io_impl = ghostty_vt.TinyIo.init.io();

        const terminal = try alloc.create(ghostty_vt.Terminal);
        errdefer alloc.destroy(terminal);
        terminal.* = try .init(io_impl, alloc, .{ .cols = cols, .rows = rows });
        errdefer terminal.deinit(alloc);
        try terminal.resize(alloc, .{
            .cols = cols,
            .rows = rows,
            .cell_size_px = .{ .width = 1, .height = 1 },
        });

        var self: Engine = .{
            .alloc = alloc,
            .terminal = terminal,
            .stream = undefined,
            .render = .empty,
            .cols = cols,
            .rows = rows,
        };
        self.stream = self.terminal.vtStream();
        return self;
    }

    pub fn deinit(self: *Engine) void {
        self.stream.deinit();
        self.render.deinit(self.alloc);
        self.terminal.deinit(self.alloc);
        self.alloc.destroy(self.terminal);
        self.* = undefined;
    }

    pub fn feed(self: *Engine, bytes: []const u8, full: bool) !void {
        if (full) {
            self.stream.deinit();
            self.terminal.deinit(self.alloc);
            const io_impl = ghostty_vt.TinyIo.init.io();
            self.terminal.* = try .init(io_impl, self.alloc, .{ .cols = self.cols, .rows = self.rows });
            try self.terminal.resize(self.alloc, .{
                .cols = self.cols,
                .rows = self.rows,
                .cell_size_px = .{ .width = 1, .height = 1 },
            });
            self.stream = self.terminal.vtStream();
        }
        if (bytes.len == 0) return;
        self.stream.nextSlice(bytes);
    }

    pub fn resize(self: *Engine, cols: u16, rows: u16) !void {
        if (cols == 0 or rows == 0) return;
        try self.terminal.resize(self.alloc, .{
            .cols = cols,
            .rows = rows,
            .cell_size_px = .{ .width = 1, .height = 1 },
        });
        self.cols = cols;
        self.rows = rows;
    }

    pub fn frame(self: *Engine, arena: Allocator) !contract.Frame {
        try self.render.update(self.alloc, self.terminal);

        const cols = self.cols;
        const rows = self.rows;
        const out = try arena.alloc(contract.Cell, @as(usize, cols) * @as(usize, rows));

        var y: u16 = 0;
        while (y < rows) : (y += 1) {
            const row_cells = self.render.row_data.items(.cells)[y];
            var x: u16 = 0;
            while (x < cols) : (x += 1) {
                const rc = row_cells.get(x);
                const raw = rc.raw;
                const occupancy: contract.Occupancy = switch (raw.wide) {
                    .wide => .wide_leading,
                    .spacer_tail, .spacer_head => .wide_continuation,
                    .narrow => .single,
                };

                const grapheme: []const u8 = if (occupancy == .wide_continuation)
                    ""
                else
                    try encodeGrapheme(arena, raw, if (raw.hasGrapheme()) rc.grapheme else &.{});

                const st = if (raw.hasStyling()) rc.style else ghostty_vt.Style{};
                out[@as(usize, y) * @as(usize, cols) + @as(usize, x)] = .{
                    .x = x,
                    .y = y,
                    .grapheme = grapheme,
                    .fg = mapColor(st.fg_color),
                    .bg = mapColor(st.bg_color),
                    .style = .{
                        .bold = st.flags.bold,
                        .italic = st.flags.italic,
                        .underline = st.flags.underline != .none,
                        .reverse = st.flags.inverse,
                    },
                    .occupancy = occupancy,
                };
            }
        }

        const cursor: contract.Cursor = if (self.render.cursor.viewport) |vp| .{
            .x = @intCast(@min(vp.x, cols -| 1)),
            .y = @intCast(@min(vp.y, rows -| 1)),
            .visible = self.render.cursor.visible,
            .wide_tail = vp.wide_tail,
        } else .{
            .x = 0,
            .y = 0,
            .visible = self.render.cursor.visible,
            .wide_tail = false,
        };

        return .{
            .cols = cols,
            .rows = rows,
            .cursor = cursor,
            .cells = out,
        };
    }
};

fn mapColor(c: ghostty_vt.Style.Color) contract.Color {
    return switch (c) {
        .none => .default,
        .palette => |idx| .{ .indexed = idx },
        .rgb => |rgb| .{ .rgb = .{ .r = rgb.r, .g = rgb.g, .b = rgb.b } },
    };
}

fn encodeGrapheme(arena: Allocator, raw: ghostty_vt.Cell, extra: []const u21) ![]const u8 {
    if (!raw.hasText()) return "";
    const first = raw.codepoint();
    if (first < 0x80 and extra.len == 0) {
        const buf = try arena.alloc(u8, 1);
        buf[0] = @intCast(first);
        return buf;
    }
    var needed: usize = try std.unicode.utf8CodepointSequenceLength(first);
    for (extra) |cp| needed += try std.unicode.utf8CodepointSequenceLength(cp);
    const buf = try arena.alloc(u8, needed);
    var i: usize = 0;
    i += try std.unicode.utf8Encode(first, buf[i..]);
    for (extra) |cp| i += try std.unicode.utf8Encode(cp, buf[i..]);
    return buf[0..i];
}

fn cellAt(f: contract.Frame, x: u16, y: u16) contract.Cell {
    return f.cells[@as(usize, y) * @as(usize, f.cols) + @as(usize, x)];
}

test "text" {
    var eng = try Engine.init(20, 5);
    defer eng.deinit();
    try eng.feed("hello", false);

    var arena = std.heap.ArenaAllocator.init(std.testing.allocator);
    defer arena.deinit();
    const f = try eng.frame(arena.allocator());
    try std.testing.expectEqualStrings("h", cellAt(f, 0, 0).grapheme);
    try std.testing.expectEqualStrings("e", cellAt(f, 1, 0).grapheme);
    try std.testing.expectEqualStrings("l", cellAt(f, 2, 0).grapheme);
    try std.testing.expectEqualStrings("l", cellAt(f, 3, 0).grapheme);
    try std.testing.expectEqualStrings("o", cellAt(f, 4, 0).grapheme);
}

test "color" {
    var eng = try Engine.init(20, 5);
    defer eng.deinit();
    try eng.feed("\x1b[31;1mHi\x1b[0m", false);

    var arena = std.heap.ArenaAllocator.init(std.testing.allocator);
    defer arena.deinit();
    const f = try eng.frame(arena.allocator());
    const c0 = cellAt(f, 0, 0);
    try std.testing.expectEqualStrings("H", c0.grapheme);
    try std.testing.expectEqual(contract.Color{ .indexed = 1 }, c0.fg);
    try std.testing.expect(c0.style.bold);
    const c1 = cellAt(f, 1, 0);
    try std.testing.expectEqualStrings("i", c1.grapheme);
    try std.testing.expectEqual(contract.Color{ .indexed = 1 }, c1.fg);
}

test "cursor" {
    var eng = try Engine.init(20, 5);
    defer eng.deinit();
    try eng.feed("\x1b[31;1mHi\x1b[0m\x1b[3;6H", false);

    var arena = std.heap.ArenaAllocator.init(std.testing.allocator);
    defer arena.deinit();
    const f = try eng.frame(arena.allocator());
    try std.testing.expectEqual(@as(u16, 5), f.cursor.x);
    try std.testing.expectEqual(@as(u16, 2), f.cursor.y);
    try std.testing.expect(f.cursor.visible);
}

test "wide cell" {
    var eng = try Engine.init(8, 3);
    defer eng.deinit();
    try eng.feed("あA", false);

    var arena = std.heap.ArenaAllocator.init(std.testing.allocator);
    defer arena.deinit();
    const f = try eng.frame(arena.allocator());
    const lead = cellAt(f, 0, 0);
    const cont = cellAt(f, 1, 0);
    const a = cellAt(f, 2, 0);
    try std.testing.expectEqual(contract.Occupancy.wide_leading, lead.occupancy);
    try std.testing.expectEqual(contract.Occupancy.wide_continuation, cont.occupancy);
    try std.testing.expectEqualStrings("", cont.grapheme);
    try std.testing.expectEqualStrings("A", a.grapheme);
    try std.testing.expectEqual(contract.Occupancy.single, a.occupancy);
}

test "box glyph" {
    var eng = try Engine.init(8, 3);
    defer eng.deinit();
    try eng.feed("あA┌┐", false);

    var arena = std.heap.ArenaAllocator.init(std.testing.allocator);
    defer arena.deinit();
    const f = try eng.frame(arena.allocator());
    const box_l = cellAt(f, 3, 0);
    const box_r = cellAt(f, 4, 0);
    try std.testing.expectEqualStrings("┌", box_l.grapheme);
    try std.testing.expectEqualStrings("┐", box_r.grapheme);
    try std.testing.expectEqual(contract.Occupancy.single, box_l.occupancy);
    try std.testing.expectEqual(contract.Occupancy.single, box_r.occupancy);
}

test "split CSI across two feeds" {
    var eng = try Engine.init(20, 5);
    defer eng.deinit();
    try eng.feed("\x1b[31;1", false);
    try eng.feed("mX\x1b[0m", false);

    var arena = std.heap.ArenaAllocator.init(std.testing.allocator);
    defer arena.deinit();
    const f = try eng.frame(arena.allocator());
    const c0 = cellAt(f, 0, 0);
    try std.testing.expectEqualStrings("X", c0.grapheme);
    try std.testing.expectEqual(contract.Color{ .indexed = 1 }, c0.fg);
    try std.testing.expect(c0.style.bold);
}
