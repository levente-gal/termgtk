//! NDJSON observe/control frame parsing (pure; no I/O).

const std = @import("std");
const contract = @import("contract");

pub const Error = error{
    InvalidNdjson,
    UnexpectedFrameEncoding,
    MissingFrameBytes,
    InvalidFrameBase64,
    SequenceResyncRequired,
};

pub const FrameParser = struct {
    last_seq: ?u64 = null,

    pub fn ingest(self: *FrameParser, allocator: std.mem.Allocator, line: []const u8) (Error || std.mem.Allocator.Error)!contract.TerminalEvent {
        const trimmed = std.mem.trim(u8, line, " \t\r\n");
        if (trimmed.len == 0) return error.InvalidNdjson;

        const Parsed = struct {
            @"type": []const u8,
            seq: ?u64 = null,
            full: ?bool = null,
            encoding: ?[]const u8 = null,
            width: ?u64 = null,
            height: ?u64 = null,
            bytes: ?[]const u8 = null,
            reason: ?[]const u8 = null,
        };

        var parsed = std.json.parseFromSlice(Parsed, allocator, trimmed, .{
            .ignore_unknown_fields = true,
        }) catch return error.InvalidNdjson;
        defer parsed.deinit();
        const v = parsed.value;

        if (std.mem.eql(u8, v.@"type", "terminal.closed")) {
            const reason = try allocator.dupe(u8, v.reason orelse "");
            return .{ .closed = .{ .reason = reason } };
        }
        if (!std.mem.eql(u8, v.@"type", "terminal.frame")) return error.InvalidNdjson;

        const seq = v.seq orelse return error.InvalidNdjson;
        const full = v.full orelse return error.InvalidNdjson;
        const encoding = v.encoding orelse return error.InvalidNdjson;
        if (!std.mem.eql(u8, encoding, "ansi")) return error.UnexpectedFrameEncoding;
        const cols: u16 = @intCast(v.width orelse return error.InvalidNdjson);
        const rows: u16 = @intCast(v.height orelse return error.InvalidNdjson);

        const b64 = v.bytes orelse return error.MissingFrameBytes;
        const dec_len = std.base64.standard.Decoder.calcSizeForSlice(b64) catch return error.InvalidFrameBase64;
        const bytes = try allocator.alloc(u8, dec_len);
        errdefer allocator.free(bytes);
        std.base64.standard.Decoder.decode(bytes, b64) catch {
            return error.InvalidFrameBase64;
        };

        if (self.last_seq) |last| {
            if (seq <= last) return error.SequenceResyncRequired;
        } else if (seq != 1 or !full) {
            return error.SequenceResyncRequired;
        }

        self.last_seq = seq;
        return .{ .data = .{
            .bytes = bytes,
            .full = full,
            .seq = seq,
            .cols = cols,
            .rows = rows,
        } };
    }
};

pub fn scroll_command(
    direction: contract.ScrollDirection,
    lines: u16,
    source: contract.ScrollSource,
    column: ?u16,
    row: ?u16,
) contract.ScrollCommand {
    return .{
        .direction = direction,
        .lines = lines,
        .source = source,
        .column = column,
        .row = row,
    };
}

/// NDJSON line for `terminal.scroll`, including the trailing newline.
pub fn format_scroll_line(buf: []u8, cmd: contract.ScrollCommand) ?[]u8 {
    if (cmd.lines == 0) return null;
    const dir = switch (cmd.direction) {
        .up => "up",
        .down => "down",
    };
    const source = switch (cmd.source) {
        .wheel => "wheel",
        .page_key => "page_key",
    };
    if (cmd.column != null and cmd.row != null) {
        return std.fmt.bufPrint(buf, "{{\"type\":\"terminal.scroll\",\"direction\":\"{s}\",\"lines\":{d},\"source\":\"{s}\",\"column\":{d},\"row\":{d}}}\n", .{
            dir,
            cmd.lines,
            source,
            cmd.column.?,
            cmd.row.?,
        }) catch null;
    }
    return std.fmt.bufPrint(buf, "{{\"type\":\"terminal.scroll\",\"direction\":\"{s}\",\"lines\":{d},\"source\":\"{s}\"}}\n", .{
        dir,
        cmd.lines,
        source,
    }) catch null;
}

fn freeEvent(allocator: std.mem.Allocator, ev: contract.TerminalEvent) void {
    switch (ev) {
        .data => |fb| allocator.free(fb.bytes),
        .closed => |c| allocator.free(c.reason),
    }
}

const initial_stream =
    \\{"type":"terminal.frame","encoding":"ansi","bytes":"G1szMW1yZWQbWzBt","seq":1,"full":true,"width":120,"height":40}
;
const close_line =
    \\{"type":"terminal.closed","reason":"terminal session observe failed: terminal target w999:p999 not found"}
;
const malformed_record =
    \\{not-json
    \\{"type":"terminal.frame","encoding":"ansi","bytes":"G1szMW1yZWQbWzBt","seq":1,"full":true,"width":120,"height":40}
;
const invalid_payload =
    \\{"type":"terminal.frame","seq":1,"full":true,"encoding":"ansi","width":120,"height":40}
    \\{"type":"terminal.frame","seq":2,"full":false,"encoding":"ansi","width":120,"height":40,"bytes":"!!!not-base64!!!"}
    \\{"type":"terminal.frame","seq":3,"full":false,"encoding":"ansi","width":120,"height":40,"bytes":""}
;
const incremental_update =
    \\{"type":"terminal.frame","encoding":"ansi","bytes":"G1szMW1yZWQbWzBt","seq":1,"full":true,"width":120,"height":40}
    \\{"type":"terminal.frame","encoding":"ansi","bytes":"G1szMW1yZWQbWzBt","seq":2,"full":false,"width":120,"height":40}
;

test "initial stream decodes red ansi full frame" {
    const alloc = std.testing.allocator;
    var parser: FrameParser = .{};
    const ev = try parser.ingest(alloc, initial_stream);
    defer freeEvent(alloc, ev);
    const fb = ev.data;
    try std.testing.expectEqualSlices(u8, "\x1b[31mred\x1b[0m", fb.bytes);
    try std.testing.expect(fb.full);
    try std.testing.expectEqual(@as(u64, 1), fb.seq);
    try std.testing.expectEqual(@as(u16, 120), fb.cols);
    try std.testing.expectEqual(@as(u16, 40), fb.rows);
}

test "close fixture emits closed" {
    const alloc = std.testing.allocator;
    var parser: FrameParser = .{};
    const ev = try parser.ingest(alloc, close_line);
    defer freeEvent(alloc, ev);
    try std.testing.expect(std.mem.indexOf(u8, ev.closed.reason, "not found") != null);
}

test "malformed record errors without payload in display" {
    const alloc = std.testing.allocator;
    var lines = std.mem.splitScalar(u8, malformed_record, '\n');
    var parser: FrameParser = .{};
    try std.testing.expectError(error.InvalidNdjson, parser.ingest(alloc, lines.next().?));
    var parser2: FrameParser = .{};
    const ok = try parser2.ingest(alloc, lines.next().?);
    defer freeEvent(alloc, ok);
    _ = ok.data;
}

test "invalid payload missing bytes and bad base64" {
    const alloc = std.testing.allocator;
    var lines = std.mem.splitScalar(u8, invalid_payload, '\n');

    var parser: FrameParser = .{};
    try std.testing.expectError(error.MissingFrameBytes, parser.ingest(alloc, lines.next().?));
    try std.testing.expectError(error.InvalidFrameBase64, parser.ingest(alloc, lines.next().?));

    var parser3: FrameParser = .{};
    const primed = try parser3.ingest(alloc, initial_stream);
    defer freeEvent(alloc, primed);
    const ev = try parser3.ingest(alloc, lines.next().?);
    defer freeEvent(alloc, ev);
    try std.testing.expectEqual(@as(usize, 0), ev.data.bytes.len);
    try std.testing.expectEqual(@as(u64, 3), ev.data.seq);
}

test "seq backwards demands resync without emitting data" {
    const alloc = std.testing.allocator;
    var parser: FrameParser = .{};
    const ok = try parser.ingest(alloc, initial_stream);
    defer freeEvent(alloc, ok);
    try std.testing.expectError(
        error.SequenceResyncRequired,
        parser.ingest(alloc, "{\"type\":\"terminal.frame\",\"encoding\":\"ansi\",\"bytes\":\"G1szMW1yZWQbWzBt\",\"seq\":1,\"full\":true,\"width\":120,\"height\":40}"),
    );
}

test "incremental fixture is data not drop" {
    const alloc = std.testing.allocator;
    var parser: FrameParser = .{};
    var lines = std.mem.splitScalar(u8, incremental_update, '\n');
    const first = try parser.ingest(alloc, lines.next().?);
    defer freeEvent(alloc, first);
    try std.testing.expect(first.data.full);
    try std.testing.expectEqual(@as(u64, 1), first.data.seq);
    const second = try parser.ingest(alloc, lines.next().?);
    defer freeEvent(alloc, second);
    try std.testing.expect(!second.data.full);
    try std.testing.expectEqual(@as(u64, 2), second.data.seq);
}

test "scroll_command names direction lines and source" {
    const line = scroll_command(.up, 3, .wheel, 12, 4);
    try std.testing.expectEqual(contract.ScrollDirection.up, line.direction);
    try std.testing.expectEqual(@as(u16, 3), line.lines);
    try std.testing.expectEqual(contract.ScrollSource.wheel, line.source);
    try std.testing.expectEqual(@as(?u16, 12), line.column);
    try std.testing.expectEqual(@as(?u16, 4), line.row);
    const page = scroll_command(.down, 20, .page_key, null, null);
    try std.testing.expectEqual(contract.ScrollDirection.down, page.direction);
    try std.testing.expectEqual(contract.ScrollSource.page_key, page.source);
    try std.testing.expect(page.column == null);
    try std.testing.expect(page.row == null);
}

test "format_scroll_line writes terminal.scroll ndjson" {
    var buf: [256]u8 = undefined;
    const wheel = format_scroll_line(&buf, scroll_command(.up, 3, .wheel, 12, 4)).?;
    try std.testing.expectEqualStrings(
        "{\"type\":\"terminal.scroll\",\"direction\":\"up\",\"lines\":3,\"source\":\"wheel\",\"column\":12,\"row\":4}\n",
        wheel,
    );
    const page = format_scroll_line(&buf, scroll_command(.down, 20, .page_key, null, null)).?;
    try std.testing.expectEqualStrings(
        "{\"type\":\"terminal.scroll\",\"direction\":\"down\",\"lines\":20,\"source\":\"page_key\"}\n",
        page,
    );
    try std.testing.expect(format_scroll_line(&buf, scroll_command(.up, 0, .wheel, null, null)) == null);
}
