//! Adwaita application entry: window + enhance action (Ctrl+Shift+E).

const std = @import("std");
const adw = @import("adw");
const gio = @import("gio");
const gtk = @import("gtk");
const glib = @import("glib");
const shell = @import("shell");

pub fn run() u8 {
    const app = adw.Application.new("dev.herdr.native", .{});
    _ = gio.Application.signals.activate.connect(app.as(gio.Application), *adw.Application, &onActivate, app, .{});
    return @intCast(app.as(gio.Application).run(0, null));
}

fn onActivate(_: *gio.Application, app: *adw.Application) callconv(.c) void {
    if (app.as(gtk.Application).getActiveWindow() != null) return;

    const enhance = gio.SimpleAction.newStateful("enhance", null, glib.Variant.newBoolean(0));
    _ = gio.SimpleAction.signals.activate.connect(enhance, *gio.SimpleAction, &onEnhanceActivate, enhance, .{});
    app.as(gio.ActionMap).addAction(enhance.as(gio.Action));

    const accels = [_]?[*:0]const u8{ "<Control><Shift>e", null };
    app.as(gtk.Application).setAccelsForAction("app.enhance", @ptrCast(@constCast(&accels)));

    const sh = shell.Shell.create(std.heap.page_allocator, app) catch return;
    _ = gio.SimpleAction.signals.change_state.connect(enhance, *shell.Shell, &onEnhanceState, sh, .{});
    sh.present();
}

fn onEnhanceActivate(action: *gio.SimpleAction, _: ?*glib.Variant, _: *gio.SimpleAction) callconv(.c) void {
    const state = action.as(gio.Action).getState() orelse return;
    defer state.unref();
    action.as(gio.Action).changeState(glib.Variant.newBoolean(if (state.getBoolean() != 0) 0 else 1));
}

fn onEnhanceState(action: *gio.SimpleAction, value: ?*glib.Variant, sh: *shell.Shell) callconv(.c) void {
    const v = value orelse return;
    action.setState(v);
    sh.setEnhance(v.getBoolean() != 0);
}
