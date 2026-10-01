const std = @import("std");
const ghostty_vt = @import("ghostty-vt");
const contract = @import("contract");

test "contract surface is importable" {
    _ = contract.Frame;
    _ = contract.HerdrSnapshot;
    _ = contract.ScrollCommand;
    _ = contract.FrameFit.use;
    _ = contract.ObserveFit.draw;
    _ = contract.ResizeSync;
}

test "ghostty-vt stream + RenderState sees ANSI text" {
    const alloc = std.testing.allocator;
    const io_impl = ghostty_vt.TinyIo.init.io();

    var t: ghostty_vt.Terminal = try .init(io_impl, alloc, .{ .cols = 40, .rows = 1 });
    defer t.deinit(alloc);

    var stream = t.vtStream();
    defer stream.deinit();

    stream.nextSlice("\x1b[0mhello\x1b[0m");

    var state: ghostty_vt.RenderState = .empty;
    defer state.deinit(alloc);
    try state.update(alloc, &t);

    const cells = state.row_data.items(.cells)[0];
    try std.testing.expectEqual(@as(u21, 'h'), cells.get(0).raw.codepoint());
    try std.testing.expectEqual(@as(u21, 'e'), cells.get(1).raw.codepoint());
    try std.testing.expectEqual(@as(u21, 'l'), cells.get(2).raw.codepoint());
    try std.testing.expectEqual(@as(u21, 'l'), cells.get(3).raw.codepoint());
    try std.testing.expectEqual(@as(u21, 'o'), cells.get(4).raw.codepoint());
}
