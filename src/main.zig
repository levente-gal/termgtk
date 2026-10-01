const app = @import("app");

// Pull Wave-1/2 modules into the run graph for `zig build check`.
comptime {
    _ = @import("shell");
    _ = @import("session");
    _ = @import("grid");
    _ = @import("sidebar");
    _ = @import("agents");
    _ = @import("split");
    _ = @import("theme");
    _ = @import("pane_gui");
    _ = @import("overlay_paint");
}

pub fn main() void {
    std.process.exit(app.run());
}

const std = @import("std");
