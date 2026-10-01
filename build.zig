const std = @import("std");

pub fn build(b: *std.Build) void {
    const target = b.standardTargetOptions(.{});
    const optimize = b.standardOptimizeOption(.{});

    const ghostty_dep = b.dependency("ghostty", .{
        .@"app-runtime" = .none,
    });
    const ghostty_vt = ghostty_dep.module("ghostty-vt");

    const contract_mod = b.createModule(.{
        .root_source_file = b.path("src/contract.zig"),
        .target = target,
        .optimize = optimize,
    });

    const engine_mod = b.createModule(.{
        .root_source_file = b.path("src/terminal/engine.zig"),
        .target = target,
        .optimize = optimize,
        .imports = &.{
            .{ .name = "ghostty-vt", .module = ghostty_vt },
            .{ .name = "contract", .module = contract_mod },
        },
    });
    const protocol_mod = b.createModule(.{
        .root_source_file = b.path("src/herdr/protocol.zig"),
        .target = target,
        .optimize = optimize,
        .imports = &.{
            .{ .name = "contract", .module = contract_mod },
        },
    });
    const layout_mod = b.createModule(.{
        .root_source_file = b.path("src/herdr/layout.zig"),
        .target = target,
        .optimize = optimize,
        .imports = &.{
            .{ .name = "contract", .module = contract_mod },
        },
    });
    protocol_mod.addImport("layout", layout_mod);

    const ndjson_mod = b.createModule(.{
        .root_source_file = b.path("src/herdr/ndjson.zig"),
        .target = target,
        .optimize = optimize,
        .imports = &.{
            .{ .name = "contract", .module = contract_mod },
        },
    });
    const rules_mod = b.createModule(.{
        .root_source_file = b.path("src/herdr/rules.zig"),
        .target = target,
        .optimize = optimize,
        .imports = &.{
            .{ .name = "contract", .module = contract_mod },
            .{ .name = "ndjson", .module = ndjson_mod },
        },
    });
    const input_mod = b.createModule(.{
        .root_source_file = b.path("src/herdr/input.zig"),
        .target = target,
        .optimize = optimize,
        .imports = &.{
            .{ .name = "ghostty-vt", .module = ghostty_vt },
            .{ .name = "contract", .module = contract_mod },
        },
    });
    const cards_mod = b.createModule(.{
        .root_source_file = b.path("src/semantic/cards.zig"),
        .target = target,
        .optimize = optimize,
        .imports = &.{
            .{ .name = "contract", .module = contract_mod },
        },
    });
    const tree_mod = b.createModule(.{
        .root_source_file = b.path("src/semantic/tree.zig"),
        .target = target,
        .optimize = optimize,
        .imports = &.{
            .{ .name = "contract", .module = contract_mod },
            .{ .name = "cards", .module = cards_mod },
        },
    });
    const paint_mod = b.createModule(.{
        .root_source_file = b.path("src/render/paint.zig"),
        .target = target,
        .optimize = optimize,
        .imports = &.{
            .{ .name = "contract", .module = contract_mod },
        },
    });

    const test_mod = b.createModule(.{
        .root_source_file = b.path("src/terminal/link_test.zig"),
        .target = target,
        .optimize = optimize,
        .imports = &.{
            .{ .name = "ghostty-vt", .module = ghostty_vt },
            .{ .name = "contract", .module = contract_mod },
        },
    });
    const test_step = b.step("test", "Run unit tests");
    for ([_]*std.Build.Module{
        test_mod,
        engine_mod,
        protocol_mod,
        layout_mod,
        ndjson_mod,
        rules_mod,
        input_mod,
        tree_mod,
        cards_mod,
        paint_mod,
    }) |mod| {
        const t = b.addTest(.{ .root_module = mod });
        test_step.dependOn(&b.addRunArtifact(t).step);
    }

    const gobject_dep = b.dependency("gobject", .{
        .target = target,
        .optimize = optimize,
    });
    const adw_mod = gobject_dep.module("adw1");
    const gtk_mod = gobject_dep.module("gtk4");
    const gio_mod = gobject_dep.module("gio2");
    const glib_mod = gobject_dep.module("glib2");
    const gobject_mod = gobject_dep.module("gobject2");
    const gdk_mod = gobject_dep.module("gdk4");
    const graphene_mod = gobject_dep.module("graphene1");
    const pango_mod = gobject_dep.module("pango1");

    const session_mod = b.createModule(.{
        .root_source_file = b.path("src/herdr/session.zig"),
        .target = target,
        .optimize = optimize,
        .imports = &.{
            .{ .name = "contract", .module = contract_mod },
            .{ .name = "gio", .module = gio_mod },
            .{ .name = "glib", .module = glib_mod },
            .{ .name = "rules", .module = rules_mod },
            .{ .name = "ndjson", .module = ndjson_mod },
            .{ .name = "protocol", .module = protocol_mod },
            .{ .name = "layout", .module = layout_mod },
            .{ .name = "engine", .module = engine_mod },
            .{ .name = "input", .module = input_mod },
        },
    });
    const grid_mod = b.createModule(.{
        .root_source_file = b.path("src/ui/grid.zig"),
        .target = target,
        .optimize = optimize,
        .imports = &.{
            .{ .name = "contract", .module = contract_mod },
            .{ .name = "gtk", .module = gtk_mod },
            .{ .name = "gdk", .module = gdk_mod },
            .{ .name = "glib", .module = glib_mod },
            .{ .name = "gobject", .module = gobject_mod },
            .{ .name = "graphene", .module = graphene_mod },
            .{ .name = "pango", .module = pango_mod },
            .{ .name = "paint", .module = paint_mod },
            .{ .name = "cards", .module = cards_mod },
        },
    });
    const theme_mod = b.createModule(.{
        .root_source_file = b.path("src/ui/theme.zig"),
        .target = target,
        .optimize = optimize,
        .imports = &.{
            .{ .name = "gtk", .module = gtk_mod },
        },
    });
    const sidebar_mod = b.createModule(.{
        .root_source_file = b.path("src/ui/sidebar.zig"),
        .target = target,
        .optimize = optimize,
        .imports = &.{
            .{ .name = "contract", .module = contract_mod },
            .{ .name = "gtk", .module = gtk_mod },
            .{ .name = "theme", .module = theme_mod },
        },
    });
    const agents_mod = b.createModule(.{
        .root_source_file = b.path("src/ui/agents.zig"),
        .target = target,
        .optimize = optimize,
        .imports = &.{
            .{ .name = "contract", .module = contract_mod },
            .{ .name = "gtk", .module = gtk_mod },
            .{ .name = "theme", .module = theme_mod },
        },
    });
    const split_mod = b.createModule(.{
        .root_source_file = b.path("src/ui/split.zig"),
        .target = target,
        .optimize = optimize,
        .imports = &.{
            .{ .name = "contract", .module = contract_mod },
            .{ .name = "gtk", .module = gtk_mod },
        },
    });
    const overlay_paint_mod = b.createModule(.{
        .root_source_file = b.path("src/ui/overlay_paint.zig"),
        .target = target,
        .optimize = optimize,
        .imports = &.{
            .{ .name = "contract", .module = contract_mod },
            .{ .name = "gtk", .module = gtk_mod },
            .{ .name = "paint", .module = paint_mod },
            .{ .name = "cards", .module = cards_mod },
        },
    });
    const pane_gui_mod = b.createModule(.{
        .root_source_file = b.path("src/ui/pane_gui.zig"),
        .target = target,
        .optimize = optimize,
        .imports = &.{
            .{ .name = "contract", .module = contract_mod },
            .{ .name = "gtk", .module = gtk_mod },
            .{ .name = "tree", .module = tree_mod },
            .{ .name = "cards", .module = cards_mod },
            .{ .name = "overlay_paint", .module = overlay_paint_mod },
        },
    });
    const shell_mod = b.createModule(.{
        .root_source_file = b.path("src/ui/shell.zig"),
        .target = target,
        .optimize = optimize,
        .imports = &.{
            .{ .name = "adw", .module = adw_mod },
            .{ .name = "gtk", .module = gtk_mod },
            .{ .name = "gdk", .module = gdk_mod },
            .{ .name = "gio", .module = gio_mod },
            .{ .name = "glib", .module = glib_mod },
            .{ .name = "contract", .module = contract_mod },
            .{ .name = "session", .module = session_mod },
            .{ .name = "input", .module = input_mod },
            .{ .name = "grid", .module = grid_mod },
            .{ .name = "sidebar", .module = sidebar_mod },
            .{ .name = "agents", .module = agents_mod },
            .{ .name = "split", .module = split_mod },
            .{ .name = "theme", .module = theme_mod },
            .{ .name = "pane_gui", .module = pane_gui_mod },
            .{ .name = "protocol", .module = protocol_mod },
            .{ .name = "tree", .module = tree_mod },
            .{ .name = "engine", .module = engine_mod },
            .{ .name = "rules", .module = rules_mod },
        },
    });
    const app_mod = b.createModule(.{
        .root_source_file = b.path("src/app.zig"),
        .target = target,
        .optimize = optimize,
        .imports = &.{
            .{ .name = "adw", .module = adw_mod },
            .{ .name = "gio", .module = gio_mod },
            .{ .name = "gtk", .module = gtk_mod },
            .{ .name = "glib", .module = glib_mod },
            .{ .name = "shell", .module = shell_mod },
        },
    });

    const run_mod = b.createModule(.{
        .root_source_file = b.path("src/main.zig"),
        .target = target,
        .optimize = optimize,
        .imports = &.{
            .{ .name = "adw", .module = adw_mod },
            .{ .name = "gtk", .module = gtk_mod },
            .{ .name = "gio", .module = gio_mod },
            .{ .name = "app", .module = app_mod },
            .{ .name = "shell", .module = shell_mod },
            .{ .name = "session", .module = session_mod },
            .{ .name = "grid", .module = grid_mod },
            .{ .name = "sidebar", .module = sidebar_mod },
            .{ .name = "agents", .module = agents_mod },
            .{ .name = "split", .module = split_mod },
            .{ .name = "theme", .module = theme_mod },
            .{ .name = "pane_gui", .module = pane_gui_mod },
            .{ .name = "overlay_paint", .module = overlay_paint_mod },
            .{ .name = "contract", .module = contract_mod },
            .{ .name = "engine", .module = engine_mod },
            .{ .name = "paint", .module = paint_mod },
            .{ .name = "tree", .module = tree_mod },
            .{ .name = "cards", .module = cards_mod },
            .{ .name = "rules", .module = rules_mod },
            .{ .name = "ndjson", .module = ndjson_mod },
            .{ .name = "protocol", .module = protocol_mod },
            .{ .name = "layout", .module = layout_mod },
            .{ .name = "input", .module = input_mod },
            .{ .name = "ghostty-vt", .module = ghostty_vt },
        },
    });
    run_mod.link_libc = true;
    run_mod.linkSystemLibrary("gtk4", .{});
    run_mod.linkSystemLibrary("libadwaita-1", .{});
    run_mod.linkSystemLibrary("pango", .{});
    shell_mod.link_libc = true;
    shell_mod.linkSystemLibrary("gtk4", .{});
    shell_mod.linkSystemLibrary("libadwaita-1", .{});
    shell_mod.linkSystemLibrary("pango", .{});
    app_mod.link_libc = true;
    app_mod.linkSystemLibrary("gtk4", .{});
    app_mod.linkSystemLibrary("libadwaita-1", .{});

    const exe = b.addExecutable(.{
        .name = "herdr",
        .root_module = run_mod,
    });
    b.installArtifact(exe);

    const run_cmd = b.addRunArtifact(exe);
    run_cmd.step.dependOn(b.getInstallStep());
    if (b.args) |args| run_cmd.addArgs(args);
    const run_step = b.step("run", "Run herdr");
    run_step.dependOn(&run_cmd.step);

    // Typecheck the run module without executing (no display needed).
    const check_step = b.step("check", "Typecheck run module without opening a window");
    check_step.dependOn(&exe.step);
}
