//! Limited terminal key encoding for Herdr `terminal.input`.
//!
//! Printable text uses `.text`. Controls use `.bytes`. Compose, IME, mouse,
//! and key-release are out of scope.

const std = @import("std");
const ghostty_vt = @import("ghostty-vt");
const contract = @import("contract");

comptime {
    _ = contract.Frame;
}

pub const KeyInput = union(enum) {
    text: []const u8,
    bytes: []const u8,
};

pub const Key = union(enum) {
    char: u8,
    enter,
    backspace,
    tab,
    escape,
    up,
    down,
    left,
    right,
    other,
};

pub const KeyEvent = struct {
    key: Key,
    ctrl: bool = false,
    alt: bool = false,
    shift: bool = false,
};

/// Map one key press into a Herdr input record body.
///
/// Returns `null` for Alt, Ctrl+Shift (app enhance), Shift+Tab, other Ctrl
/// letters, and unsupported keys. Allowed keys are encoded via
/// `ghostty_vt.input.encodeKey` with `KeyEncodeOptions.fromTerminal`.
/// `buf` holds the encoded payload; returned slices borrow from it.
pub fn encode_key(t: *const ghostty_vt.Terminal, event: KeyEvent, buf: []u8) ?KeyInput {
    if (event.alt or (event.ctrl and event.shift)) return null;
    if (event.ctrl) switch (event.key) {
        .char => |c| switch (c) {
            'c', 'C', 'd', 'D', 'l', 'L', 'r', 'R' => {},
            else => return null,
        },
        else => return null,
    };
    if (event.shift and event.key == .tab) return null;

    var utf8_byte: [1]u8 = undefined;
    const ge: ghostty_vt.input.KeyEvent = switch (event.key) {
        .char => |c| blk: {
            if (std.ascii.isControl(c)) return null;
            utf8_byte[0] = c;
            break :blk .{
                .key = .unidentified,
                .mods = .{ .ctrl = event.ctrl, .shift = event.shift },
                .utf8 = &utf8_byte,
                .unshifted_codepoint = if (c >= 'A' and c <= 'Z') c + ('a' - 'A') else c,
            };
        },
        .enter => .{ .key = .enter },
        .backspace => .{ .key = .backspace },
        .tab => .{ .key = .tab },
        .escape => .{ .key = .escape },
        .up => .{ .key = .arrow_up },
        .down => .{ .key = .arrow_down },
        .right => .{ .key = .arrow_right },
        .left => .{ .key = .arrow_left },
        .other => return null,
    };

    var writer: std.Io.Writer = .fixed(buf);
    ghostty_vt.input.encodeKey(
        &writer,
        ge,
        ghostty_vt.input.KeyEncodeOptions.fromTerminal(t),
    ) catch return null;
    const out = writer.buffered();
    if (out.len == 0) return null;

    return switch (event.key) {
        .char => if (!event.ctrl) .{ .text = out } else .{ .bytes = out },
        else => .{ .bytes = out },
    };
}

test "printable text and required controls" {
    const alloc = std.testing.allocator;
    const io_impl = ghostty_vt.TinyIo.init.io();
    var t: ghostty_vt.Terminal = try .init(io_impl, alloc, .{ .cols = 40, .rows = 1 });
    defer t.deinit(alloc);
    var buf: [64]u8 = undefined;

    {
        const got = encode_key(&t, .{ .key = .{ .char = 'a' } }, &buf).?;
        try std.testing.expectEqualStrings("a", got.text);
    }
    {
        const got = encode_key(&t, .{ .key = .{ .char = 'A' }, .shift = true }, &buf).?;
        try std.testing.expectEqualStrings("A", got.text);
    }
    {
        const got = encode_key(&t, .{ .key = .enter }, &buf).?;
        try std.testing.expectEqualStrings("\r", got.bytes);
    }
    {
        const got = encode_key(&t, .{ .key = .backspace }, &buf).?;
        try std.testing.expectEqualSlices(u8, &.{0x7f}, got.bytes);
    }
    {
        const got = encode_key(&t, .{ .key = .tab }, &buf).?;
        try std.testing.expectEqualStrings("\t", got.bytes);
    }
    {
        const got = encode_key(&t, .{ .key = .escape }, &buf).?;
        try std.testing.expectEqualSlices(u8, &.{0x1b}, got.bytes);
    }
}

test "arrows match control fixture bytes" {
    const alloc = std.testing.allocator;
    const io_impl = ghostty_vt.TinyIo.init.io();
    var t: ghostty_vt.Terminal = try .init(io_impl, alloc, .{ .cols = 40, .rows = 1 });
    defer t.deinit(alloc);
    var buf: [64]u8 = undefined;

    const up = encode_key(&t, .{ .key = .up }, &buf).?;
    try std.testing.expectEqualStrings("\x1b[A", up.bytes);
    var b64_buf: [std.base64.standard.Encoder.calcSize(3)]u8 = undefined;
    const b64 = std.base64.standard.Encoder.encode(&b64_buf, up.bytes);
    try std.testing.expectEqualStrings("G1tB", b64);

    try std.testing.expectEqualStrings(
        "\x1b[B",
        encode_key(&t, .{ .key = .down }, &buf).?.bytes,
    );
    try std.testing.expectEqualStrings(
        "\x1b[C",
        encode_key(&t, .{ .key = .right }, &buf).?.bytes,
    );
    try std.testing.expectEqualStrings(
        "\x1b[D",
        encode_key(&t, .{ .key = .left }, &buf).?.bytes,
    );
}

test "ctrl letters and app shortcut stay out" {
    const alloc = std.testing.allocator;
    const io_impl = ghostty_vt.TinyIo.init.io();
    var t: ghostty_vt.Terminal = try .init(io_impl, alloc, .{ .cols = 40, .rows = 1 });
    defer t.deinit(alloc);
    var buf: [64]u8 = undefined;

    try std.testing.expectEqualSlices(
        u8,
        &.{0x03},
        encode_key(&t, .{ .key = .{ .char = 'c' }, .ctrl = true }, &buf).?.bytes,
    );
    try std.testing.expectEqualSlices(
        u8,
        &.{0x04},
        encode_key(&t, .{ .key = .{ .char = 'D' }, .ctrl = true }, &buf).?.bytes,
    );
    try std.testing.expectEqualSlices(
        u8,
        &.{0x0c},
        encode_key(&t, .{ .key = .{ .char = 'l' }, .ctrl = true }, &buf).?.bytes,
    );
    try std.testing.expectEqualSlices(
        u8,
        &.{0x12},
        encode_key(&t, .{ .key = .{ .char = 'R' }, .ctrl = true }, &buf).?.bytes,
    );

    try std.testing.expect(encode_key(&t, .{ .key = .{ .char = 'e' }, .ctrl = true, .shift = true }, &buf) == null);
    try std.testing.expect(encode_key(&t, .{ .key = .{ .char = 'E' }, .ctrl = true, .shift = true }, &buf) == null);
    try std.testing.expect(encode_key(&t, .{ .key = .{ .char = 'x' }, .ctrl = true }, &buf) == null);
    try std.testing.expect(encode_key(&t, .{ .key = .up, .ctrl = true }, &buf) == null);
    try std.testing.expect(encode_key(&t, .{ .key = .{ .char = 'a' }, .alt = true }, &buf) == null);
    try std.testing.expect(encode_key(&t, .{ .key = .tab, .shift = true }, &buf) == null);
    try std.testing.expect(encode_key(&t, .{ .key = .other }, &buf) == null);
}

test "encode from terminal for allowed arrow" {
    const alloc = std.testing.allocator;
    const io_impl = ghostty_vt.TinyIo.init.io();
    var t: ghostty_vt.Terminal = try .init(io_impl, alloc, .{ .cols = 40, .rows = 1 });
    defer t.deinit(alloc);

    const opts = ghostty_vt.input.KeyEncodeOptions.fromTerminal(&t);
    var buf: [64]u8 = undefined;
    var writer: std.Io.Writer = .fixed(&buf);
    try ghostty_vt.input.encodeKey(&writer, .{ .key = .arrow_up }, opts);
    try std.testing.expectEqualStrings("\x1b[A", writer.buffered());

    try std.testing.expectEqualStrings(
        "\x1b[A",
        encode_key(&t, .{ .key = .up }, &buf).?.bytes,
    );
}
