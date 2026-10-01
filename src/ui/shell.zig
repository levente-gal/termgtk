//! Adwaita shell: sidebar, agents, split grids, enhance overlay, session wiring.

const std = @import("std");
const adw = @import("adw");
const gtk = @import("gtk");
const gdk = @import("gdk");
const gio = @import("gio");
const glib = @import("glib");
const contract = @import("contract");
const session_mod = @import("session");
const input = @import("input");
const grid_mod = @import("grid");
const sidebar_mod = @import("sidebar");
const agents_mod = @import("agents");
const split_mod = @import("split");
const theme = @import("theme");
const pane_gui_mod = @import("pane_gui");
const protocol = @import("protocol");
const tree = @import("tree");
const rules = @import("rules");

const Allocator = std.mem.Allocator;
const RESIZE_MS: c_uint = 80;
const TICK_MS: c_uint = 50;
const SNAPSHOT_MS: c_uint = 2000;

var active: ?*Shell = null;

pub const Shell = struct {
    alloc: Allocator,
    app: *adw.Application,
    window: *adw.ApplicationWindow,
    banner: *adw.Banner,
    session: *session_mod.Session,
    sidebar: *sidebar_mod.WorkspaceSidebar,
    agents: *agents_mod.AgentPanel,
    terminal_host: *gtk.Overlay,
    grids_parent: *gtk.Box,
    enhance_host: *gtk.Fixed,
    pane_gui: *pane_gui_mod.PaneGui,
    takeover_btn: *gtk.ToggleButton,
    grids: std.StringHashMapUnmanaged(*grid_mod.Grid) = .empty,
    paint_arenas: [2]std.heap.ArenaAllocator,
    paint_i: u1 = 0,
    snap_arena: std.heap.ArenaAllocator,
    layout: ?contract.TabLayout = null,
    focused_pane: []u8 = &.{},
    enhance: bool = false,
    takeover: bool = false,
    resize_id: c_uint = 0,
    tick_id: c_uint = 0,
    snap_id: c_uint = 0,
    snap_busy: bool = false,
    snap_pending: bool = false,
    pending_w: c_int = 0,
    pending_h: c_int = 0,
    last_w: c_int = 0,
    last_h: c_int = 0,
    retained_tree: ?tree.LayoutTree = null,

    pub fn create(alloc: Allocator, app: *adw.Application) !*Shell {
        const self = try alloc.create(Shell);
        errdefer alloc.destroy(self);

        const sess = try session_mod.Session.create(alloc);
        errdefer sess.destroy();

        const window = adw.ApplicationWindow.new(app.as(gtk.Application));
        window.as(gtk.Window).setTitle("Herdr");
        window.as(gtk.Window).setDefaultSize(1280, 800);

        const header = adw.HeaderBar.new();
        const takeover = gtk.ToggleButton.newWithLabel("Takeover");
        header.packEnd(takeover.as(gtk.Widget));

        const banner = adw.Banner.new("Connecting to Herdr…");
        banner.setRevealed(1);

        const enhance_label = gtk.Label.new("Enhance TUI rendering");
        enhance_label.as(gtk.Widget).setHalign(.start);
        enhance_label.as(gtk.Widget).setHexpand(1);

        const enhance_switch = gtk.Switch.new();
        enhance_switch.as(gtk.Widget).setValign(.center);
        // Stateful app.enhance keeps Switch and Ctrl+Shift+E in sync.
        enhance_switch.as(gtk.Actionable).setActionName("app.enhance");

        const enhance_row = gtk.Box.new(.horizontal, 12);
        enhance_row.as(gtk.Widget).setMarginStart(12);
        enhance_row.as(gtk.Widget).setMarginEnd(12);
        enhance_row.as(gtk.Widget).setMarginTop(8);
        enhance_row.as(gtk.Widget).setMarginBottom(8);
        enhance_row.append(enhance_label.as(gtk.Widget));
        enhance_row.append(enhance_switch.as(gtk.Widget));

        const sidebar = sidebar_mod.WorkspaceSidebar.new(alloc);
        const agents = agents_mod.AgentPanel.new(alloc);
        const nav = gtk.Box.new(.vertical, 8);
        nav.as(gtk.Widget).setSizeRequest(200, -1);
        nav.append(sidebar.widget.as(gtk.Widget));
        nav.append(agents.widget.as(gtk.Widget));

        const grids_parent = gtk.Box.new(.vertical, 0);
        grids_parent.as(gtk.Widget).setHexpand(1);
        grids_parent.as(gtk.Widget).setVexpand(1);
        grids_parent.as(gtk.Widget).addCssClass(theme.CSS_CARD_CLASS);

        const enhance_host = gtk.Fixed.new();
        enhance_host.as(gtk.Widget).setHexpand(1);
        enhance_host.as(gtk.Widget).setVexpand(1);
        enhance_host.as(gtk.Widget).setVisible(0);
        enhance_host.as(gtk.Widget).setCanTarget(0);

        const terminal_host = gtk.Overlay.new();
        terminal_host.setChild(grids_parent.as(gtk.Widget));
        terminal_host.addOverlay(enhance_host.as(gtk.Widget));
        terminal_host.as(gtk.Widget).setHexpand(1);
        terminal_host.as(gtk.Widget).setVexpand(1);

        const pane_gui = try pane_gui_mod.PaneGui.new(alloc);
        pane_gui.attach(enhance_host.as(gtk.Widget));

        const body = gtk.Paned.new(.horizontal);
        body.as(gtk.Widget).addCssClass("herdr-split");
        body.setStartChild(nav.as(gtk.Widget));
        body.setEndChild(terminal_host.as(gtk.Widget));
        body.setResizeStartChild(0);
        body.setShrinkStartChild(0);

        const content_box = gtk.Box.new(.vertical, 0);
        content_box.as(gtk.Widget).addCssClass(theme.CSS_BACKGROUND_CLASS);
        content_box.append(banner.as(gtk.Widget));
        content_box.append(enhance_row.as(gtk.Widget));
        content_box.append(body.as(gtk.Widget));
        body.as(gtk.Widget).setVexpand(1);

        const toolbar = adw.ToolbarView.new();
        toolbar.addTopBar(header.as(gtk.Widget));
        toolbar.setContent(content_box.as(gtk.Widget));
        window.setContent(toolbar.as(gtk.Widget));

        self.* = .{
            .alloc = alloc,
            .app = app,
            .window = window,
            .banner = banner,
            .session = sess,
            .sidebar = sidebar,
            .agents = agents,
            .terminal_host = terminal_host,
            .grids_parent = grids_parent,
            .enhance_host = enhance_host,
            .pane_gui = pane_gui,
            .takeover_btn = takeover,
            .paint_arenas = .{ .init(alloc), .init(alloc) },
            .snap_arena = .init(alloc),
        };

        active = self;
        sess.on_snapshot = &onSessionSnapshot;
        sess.on_focus_done = &onSessionFocusDone;
        sess.on_frames_applied = &onFramesApplied;
        sidebar.connectActivated(&onWorkspaceActivated);
        agents.connectActivated(&onAgentActivated);

        _ = gtk.ToggleButton.signals.toggled.connect(takeover, *Shell, &onTakeover, self, .{});
        _ = gtk.Window.signals.close_request.connect(window.as(gtk.Window), *Shell, &onClose, self, .{});

        const keys = gtk.EventControllerKey.new();
        _ = gtk.EventControllerKey.signals.key_pressed.connect(keys, *Shell, &onKeyPressed, self, .{});
        window.as(gtk.Widget).addController(keys.as(gtk.EventController));

        theme.installForDisplay(window.as(gtk.Widget).getDisplay());
        return self;
    }

    pub fn present(self: *Shell) void {
        self.window.as(gtk.Window).present();
        self.session.attach();
        if (self.tick_id == 0) self.tick_id = glib.timeoutAdd(TICK_MS, &onTick, self);
        if (self.snap_id == 0) self.snap_id = glib.timeoutAdd(SNAPSHOT_MS, &onSnapshotTimer, self);
        self.scheduleResize();
    }

    pub fn setEnhance(self: *Shell, on: bool) void {
        if (self.enhance == on) return;
        self.enhance = on;
        // Cards are snapshot onto the focused grid, not this host overlay.
        self.enhance_host.as(gtk.Widget).setVisible(0);
        // Render-only: do not reconnect observers.
        self.paintNow();
    }

    fn destroy(self: *Shell) void {
        if (self.resize_id != 0) _ = glib.Source.remove(self.resize_id);
        if (self.tick_id != 0) _ = glib.Source.remove(self.tick_id);
        if (self.snap_id != 0) _ = glib.Source.remove(self.snap_id);
        if (active == self) active = null;
        self.releaseControl();
        self.pane_gui.deinit();
        if (self.retained_tree) |*t| t.deinit();
        var git = self.grids.iterator();
        while (git.next()) |e| {
            const widget = e.value_ptr.*.asWidget();
            if (widget.getParent() != null) widget.unparent();
            widget.unref();
            self.alloc.free(e.key_ptr.*);
        }
        self.grids.deinit(self.alloc);
        self.paint_arenas[0].deinit();
        self.paint_arenas[1].deinit();
        self.snap_arena.deinit();
        self.alloc.free(self.focused_pane);
        self.session.destroy();
        self.alloc.destroy(self);
    }

    fn releaseControl(self: *Shell) void {
        const focused = self.focused_pane;
        const visible = if (self.layout) |lay|
            session_mod.visiblePanes(self.alloc, lay) catch &.{}
        else
            &.{};
        defer if (self.layout != null and visible.len != 0) self.alloc.free(visible);
        _ = self.session.syncObservers(visible, focused, false, null) catch {};
    }

    fn scheduleResize(self: *Shell) void {
        if (self.resize_id != 0) _ = glib.Source.remove(self.resize_id);
        self.resize_id = glib.timeoutAdd(RESIZE_MS, &onResizeFire, self);
    }

    fn noteHostSize(self: *Shell) void {
        const w = self.terminal_host.as(gtk.Widget).getWidth();
        const h = self.terminal_host.as(gtk.Widget).getHeight();
        if (w == self.pending_w and h == self.pending_h) return;
        self.pending_w = w;
        self.pending_h = h;
        self.scheduleResize();
    }

    fn applyResize(self: *Shell) void {
        const w = self.pending_w;
        const h = self.pending_h;
        if (w <= 0 or h <= 0) return;
        self.last_w = w;
        self.last_h = h;

        const lay = self.layout orelse return;
        const probe = self.gridFor(lay.focused_pane_id);
        const cell_w = probe.cellWidth();
        const cell_h = probe.cellHeight();
        if (!(cell_w > 0 and cell_h > 0)) return;

        const visible = session_mod.visiblePanes(self.alloc, lay) catch return;
        defer self.alloc.free(visible);

        var capacity: ?contract.GridSize = null;
        for (lay.panes) |p| {
            if (std.mem.eql(u8, p.pane_id, lay.focused_pane_id)) {
                const px_w = @as(f64, @floatFromInt(p.width)) * cell_w;
                const px_h = @as(f64, @floatFromInt(p.height)) * cell_h;
                // Prefer host allocation for focused pane when single / zoomed.
                const use_w: f64 = if (lay.zoomed or lay.panes.len == 1) @floatFromInt(w) else px_w;
                const use_h: f64 = if (lay.zoomed or lay.panes.len == 1) @floatFromInt(h) else px_h;
                const cr = probe.colsRows(use_w, use_h);
                if (cr[0] == 0 or cr[1] == 0) return;
                capacity = .{
                    .cols = cr[0],
                    .rows = cr[1],
                    .cell_w = @intFromFloat(@round(cell_w)),
                    .cell_h = @intFromFloat(@round(cell_h)),
                };
                break;
            }
        }
        const cap = capacity orelse return;

        _ = self.session.syncObservers(visible, lay.focused_pane_id, self.takeover, cap) catch {};
        for (visible) |id| {
            // Per-pane desired size from placed rects.
            for (lay.panes) |p| {
                if (!std.mem.eql(u8, p.pane_id, id)) continue;
                const g = self.gridFor(id);
                const cr = g.colsRows(@as(f64, @floatFromInt(p.width)) * cell_w, @as(f64, @floatFromInt(p.height)) * cell_h);
                if (cr[0] == 0 or cr[1] == 0) continue;
                self.session.noteDesiredSize(id, .{
                    .cols = cr[0],
                    .rows = cr[1],
                    .cell_w = @intFromFloat(@round(cell_w)),
                    .cell_h = @intFromFloat(@round(cell_h)),
                });
            }
        }
    }

    fn rebuildSplits(self: *Shell) void {
        var child = self.grids_parent.as(gtk.Widget).getFirstChild();
        while (child) |c| {
            const next = c.getNextSibling();
            c.unparent();
            child = next;
        }
        const lay = self.layout orelse return;
        const root = split_mod.build(&lay, &paneWidgetCb);
        self.grids_parent.append(root);
    }

    fn gridFor(self: *Shell, pane_id: []const u8) *grid_mod.Grid {
        if (self.grids.get(pane_id)) |g| return g;
        const g = grid_mod.Grid.new();
        g.asWidget().setHexpand(1);
        g.asWidget().setVexpand(1);
        const key = self.alloc.dupe(u8, pane_id) catch return g;
        self.grids.put(self.alloc, key, g) catch {
            self.alloc.free(key);
            return g;
        };
        g.setPaneId(key);
        wireGridInput(g);
        return g;
    }

    fn paintNow(self: *Shell) void {
        self.paint_i ^= 1;
        _ = self.paint_arenas[self.paint_i].reset(.retain_capacity);
        const arena = self.paint_arenas[self.paint_i].allocator();

        var it = self.grids.iterator();
        while (it.next()) |e| {
            const id = e.key_ptr.*;
            const g = e.value_ptr.*;
            g.clearEnhance();
            const eng = self.session.engineFor(id) orelse {
                g.setFrame(null);
                continue;
            };
            const frame = eng.frame(arena) catch {
                g.setFrame(null);
                continue;
            };
            const heap = arena.create(contract.Frame) catch continue;
            heap.* = frame;
            g.setFrame(heap);
        }

        if (self.enhance) self.paintEnhance(arena) else self.pane_gui.detach();
    }

    fn paintEnhance(self: *Shell, arena: Allocator) void {
        const focused = self.focused_pane;
        if (focused.len == 0) return;
        const g = self.gridFor(focused);
        const frame_ptr = g.getFrame() orelse return;
        const frame = frame_ptr.*;
        self.retained_tree = tree.reconcile(self.alloc, frame, &self.retained_tree) catch return;
        const layout_tree = &self.retained_tree.?;
        const mask = arena.alloc(grid_mod.SuppressCell, layout_tree.suppress.len) catch return;
        for (layout_tree.suppress, mask) |src, *dst| dst.* = .{ .x = src.x, .y = src.y };
        g.setEnhanceMask(mask);
        self.pane_gui.attach(g.asWidget());
        self.pane_gui.sync(layout_tree, g.cellWidth(), g.cellHeight()) catch {};
        self.pane_gui.place(0, 0, frame.cols, frame.rows, g.cellWidth(), g.cellHeight());
        g.setOverlay(self.pane_gui.layoutWidget(), self.pane_gui.dialogsWidget(), self.pane_gui.dialogsOpen());
        g.asWidget().queueDraw();
    }

    fn applySnapshotJson(self: *Shell, json: []const u8) void {
        self.layout = null;
        _ = self.snap_arena.reset(.retain_capacity);
        const arena = self.snap_arena.allocator();
        const snap = protocol.fromSnapshotJson(arena, json) catch return;
        self.sidebar.setRows(snap.workspaces, snap.focused_workspace_id);
        const focused_pane_id: ?[]const u8 = if (snap.focused) |f| f.pane_id else null;
        self.agents.setRows(snap.agents, focused_pane_id);

        if (snap.focused) |f| {
            self.alloc.free(self.focused_pane);
            self.focused_pane = self.alloc.dupe(u8, f.pane_id) catch &.{};

            const root = std.json.parseFromSliceLeaky(std.json.Value, arena, json, .{}) catch return;
            const snapshot_val = blk: {
                const result = switch (root) {
                    .object => |o| o.get("result"),
                    else => null,
                } orelse break :blk null;
                break :blk switch (result) {
                    .object => |o| o.get("snapshot"),
                    else => null,
                };
            } orelse return;
            const lay = session_mod.tabLayout(arena, snapshot_val, f.workspace_id, f.tab_id) catch {
                self.banner.setTitle("Layout unavailable");
                self.banner.setRevealed(1);
                return;
            };
            self.layout = lay;
            var title_buf: [128]u8 = undefined;
            const title = std.fmt.bufPrintZ(&title_buf, "{s} · {s}", .{
                f.workspace_label orelse f.workspace_id,
                f.tab_label orelse f.tab_id,
            }) catch "Herdr";
            self.window.as(gtk.Window).setTitle(title);
            self.banner.setRevealed(0);
            self.rebuildSplits();
            self.scheduleResize();
        } else {
            self.banner.setTitle("No focused pane");
            self.banner.setRevealed(1);
        }
    }

    fn requestShellSnapshot(self: *Shell) void {
        if (!self.session.gated) return;
        if (self.snap_busy) {
            self.snap_pending = true;
            return;
        }
        self.snap_busy = true;
        var argv = [_]?[*:0]const u8{ "herdr", "api", "snapshot", null };
        const flags = gio.SubprocessFlags{ .stdout_pipe = true, .stderr_pipe = true };
        const proc = gio.Subprocess.newv(@ptrCast(&argv), flags, null) orelse {
            self.snap_busy = false;
            return;
        };
        const req = self.alloc.create(SnapReq) catch {
            proc.unref();
            self.snap_busy = false;
            return;
        };
        req.* = .{ .shell = self, .proc = proc };
        gio.Subprocess.communicateUtf8Async(proc, null, null, @ptrCast(&onSnapDone), req);
    }
};

const SnapReq = struct {
    shell: *Shell,
    proc: *gio.Subprocess,
};

extern fn g_free(mem: ?*anyopaque) void;

fn onSnapDone(_: ?*anyopaque, res: *gio.AsyncResult, data: ?*anyopaque) callconv(.c) void {
    const req: *SnapReq = @ptrCast(@alignCast(data.?));
    const self = req.shell;
    defer {
        req.proc.unref();
        self.alloc.destroy(req);
        self.snap_busy = false;
        if (self.snap_pending) {
            self.snap_pending = false;
            self.requestShellSnapshot();
        }
    }
    var stdout_z: ?[*:0]u8 = null;
    var stderr_z: ?[*:0]u8 = null;
    if (gio.Subprocess.communicateUtf8Finish(req.proc, res, @ptrCast(&stdout_z), @ptrCast(&stderr_z), null) == 0) {
        if (stdout_z) |s| g_free(s);
        if (stderr_z) |s| g_free(s);
        return;
    }
    defer if (stdout_z) |s| g_free(s);
    defer if (stderr_z) |s| g_free(s);
    const out: []const u8 = if (stdout_z) |s| std.mem.span(s) else "";
    self.applySnapshotJson(out);
}

fn paneWidgetCb(pane_id: []const u8) *gtk.Widget {
    const sh = active.?;
    return sh.gridFor(pane_id).asWidget();
}

fn onSessionSnapshot(session: *session_mod.Session, json: []const u8) void {
    _ = session;
    const sh = active orelse return;
    sh.applySnapshotJson(json);
}

fn onSessionFocusDone(session: *session_mod.Session) void {
    _ = session;
    const sh = active orelse return;
    // Spec: focus argv, then snapshot (do not race them).
    sh.requestShellSnapshot();
}

fn onFramesApplied(session: *session_mod.Session) void {
    _ = session;
    const sh = active orelse return;
    sh.paintNow();
}

fn onWorkspaceActivated(id: []const u8) void {
    const sh = active orelse return;
    sh.session.requestFocusWorkspace(id);
}

fn onAgentActivated(id: []const u8) void {
    const sh = active orelse return;
    sh.session.requestFocusAgent(id);
}

fn onTakeover(btn: *gtk.ToggleButton, self: *Shell) callconv(.c) void {
    self.takeover = btn.getActive() != 0;
    self.scheduleResize();
}

fn onClose(_: *gtk.Window, self: *Shell) callconv(.c) c_int {
    self.destroy();
    return 0;
}

fn onResizeFire(data: ?*anyopaque) callconv(.c) c_int {
    const self: *Shell = @ptrCast(@alignCast(data.?));
    self.resize_id = 0;
    self.applyResize();
    return 0;
}

fn onTick(data: ?*anyopaque) callconv(.c) c_int {
    const self: *Shell = @ptrCast(@alignCast(data.?));
    self.noteHostSize();
    if (self.session.gated) {
        if (self.banner.getRevealed() != 0 and self.layout == null) {
            self.banner.setTitle("Waiting for snapshot…");
        }
        self.paintNow();
    }
    return 1;
}

fn onSnapshotTimer(data: ?*anyopaque) callconv(.c) c_int {
    const self: *Shell = @ptrCast(@alignCast(data.?));
    if (self.session.gated) self.requestShellSnapshot();
    return 1;
}

fn onKeyPressed(
    _: *gtk.EventControllerKey,
    keyval: c_uint,
    _: c_uint,
    state: gdk.ModifierType,
    self: *Shell,
) callconv(.c) c_int {
    // Ctrl+Shift+E: activate app.enhance (key controller may beat window accels).
    if (state.control_mask and state.shift_mask and (keyval == gdk.KEY_e or keyval == gdk.KEY_E)) {
        self.app.as(gio.ActionGroup).activateAction("enhance", null);
        return 1;
    }
    if (state.alt_mask and !state.control_mask) {
        const direction: ?contract.PaneDirection = switch (keyval) {
            gdk.KEY_Left, gdk.KEY_KP_Left => .left,
            gdk.KEY_Right, gdk.KEY_KP_Right => .right,
            gdk.KEY_Up, gdk.KEY_KP_Up => .up,
            gdk.KEY_Down, gdk.KEY_KP_Down => .down,
            else => null,
        };
        if (direction) |dir| {
            if (self.focused_pane.len != 0) self.session.requestFocusPaneDirection(dir, self.focused_pane);
            return 1;
        }
    }
    if (!state.control_mask and !state.alt_mask) {
        const page_up = keyval == gdk.KEY_Page_Up or keyval == gdk.KEY_KP_Page_Up;
        const page_down = keyval == gdk.KEY_Page_Down or keyval == gdk.KEY_KP_Page_Down;
        if (page_up or page_down) {
            scrollFocused(self, if (page_up) .up else .down);
            return 1;
        }
    }
    if (!self.takeover) return 0;
    const focused = self.focused_pane;
    if (focused.len == 0) return 0;
    const eng = self.session.engineFor(focused) orelse return 0;

    const event = mapKey(keyval, state) orelse return 0;
    var buf: [64]u8 = undefined;
    const encoded = input.encode_key(eng.terminal, event, &buf) orelse return 0;
    self.session.writeInput(focused, encoded);
    return 1;
}

fn mapKey(keyval: c_uint, state: gdk.ModifierType) ?input.KeyEvent {
    const ctrl = state.control_mask;
    const alt = state.alt_mask;
    const shift = state.shift_mask;
    const key: input.Key = switch (keyval) {
        gdk.KEY_Return => .enter,
        gdk.KEY_BackSpace => .backspace,
        gdk.KEY_Tab => .tab,
        gdk.KEY_Escape => .escape,
        gdk.KEY_Up => .up,
        gdk.KEY_Down => .down,
        gdk.KEY_Left => .left,
        gdk.KEY_Right => .right,
        else => blk: {
            if (keyval >= 32 and keyval < 127) break :blk .{ .char = @intCast(keyval) };
            break :blk .other;
        },
    };
    return .{ .key = key, .ctrl = ctrl, .alt = alt, .shift = shift };
}

fn wireGridInput(grid: *grid_mod.Grid) void {
    if (grid.scroll_wired) return;
    grid.scroll_wired = true;
    const scroll = gtk.EventControllerScroll.new(.{ .vertical = true, .kinetic = true });
    _ = gtk.EventControllerScroll.signals.scroll.connect(scroll, *grid_mod.Grid, &onGridScroll, grid, .{});
    grid.asWidget().addController(scroll.as(gtk.EventController));

    const motion = gtk.EventControllerMotion.new();
    _ = gtk.EventControllerMotion.signals.enter.connect(motion, *grid_mod.Grid, &onGridPointer, grid, .{});
    _ = gtk.EventControllerMotion.signals.motion.connect(motion, *grid_mod.Grid, &onGridPointer, grid, .{});
    _ = gtk.EventControllerMotion.signals.leave.connect(motion, *grid_mod.Grid, &onGridPointerLeave, grid, .{});
    grid.asWidget().addController(motion.as(gtk.EventController));
}

fn onGridPointer(_: *gtk.EventControllerMotion, x: f64, y: f64, grid: *grid_mod.Grid) callconv(.c) void {
    grid.pointer_x = x;
    grid.pointer_y = y;
    grid.has_pointer = true;
}

fn onGridPointerLeave(_: *gtk.EventControllerMotion, grid: *grid_mod.Grid) callconv(.c) void {
    grid.has_pointer = false;
}

fn onGridScroll(controller: *gtk.EventControllerScroll, _: f64, dy: f64, grid: *grid_mod.Grid) callconv(.c) c_int {
    const self = active orelse return 1;
    if (grid.paneId().len == 0) return 1;
    const cell_h = @max(grid.cellHeight(), 1.0);
    const delta_lines = if (controller.getUnit() == .surface) dy / cell_h else dy * 3.0;
    const stepped = rules.push_scroll_delta(&grid.scroll_accum, delta_lines) orelse return 1;
    const size = gridSizeOf(grid);
    const cell = grid.cellAtPointer();
    self.session.writeScroll(grid.paneId(), .{
        .direction = stepped[0],
        .lines = stepped[1],
        .source = .wheel,
        .column = if (cell) |c| c[0] else null,
        .row = if (cell) |c| c[1] else null,
    }, size);
    return 1;
}

fn scrollFocused(self: *Shell, direction: contract.ScrollDirection) void {
    if (self.focused_pane.len == 0) return;
    const g = self.grids.get(self.focused_pane) orelse return;
    const size = gridSizeOf(g);
    const lines: u16 = if (size.rows > 1) size.rows - 1 else 1;
    self.session.writeScroll(self.focused_pane, .{
        .direction = direction,
        .lines = lines,
        .source = .page_key,
    }, size);
}

fn gridSizeOf(grid: *grid_mod.Grid) contract.GridSize {
    const frame = grid.getFrame();
    const widget = grid.asWidget();
    const cr = grid.colsRows(@floatFromInt(@max(widget.getWidth(), 1)), @floatFromInt(@max(widget.getHeight(), 1)));
    const cols: u16 = if (frame) |f| f.cols else if (cr[0] == 0) 80 else cr[0];
    const rows: u16 = if (frame) |f| f.rows else if (cr[1] == 0) 24 else cr[1];
    return .{
        .cols = cols,
        .rows = rows,
        .cell_w = @intFromFloat(@max(@round(grid.cellWidth()), 1)),
        .cell_h = @intFromFloat(@max(@round(grid.cellHeight()), 1)),
    };
}
