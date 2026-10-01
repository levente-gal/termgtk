//! Herdr session: GIO subprocess streams, idle-coalesced feed/resize/release.

const std = @import("std");
const contract = @import("contract");
const gio = @import("gio");
const rules = @import("rules");
const ndjson = @import("ndjson");
const protocol = @import("protocol");
const layout = @import("layout");
const engine = @import("engine");
const input_mod = @import("input");

const Allocator = std.mem.Allocator;

const GError = opaque {};

extern fn g_free(mem: ?*anyopaque) void;
extern fn g_error_free(err: *GError) void;
extern fn g_idle_add(function: *const fn (?*anyopaque) callconv(.c) c_int, data: ?*anyopaque) c_uint;
extern fn g_source_remove(tag: c_uint) c_int;

const RELEASE_WAIT_MS: u64 = 300;
const RELEASE_LINE = "{\"type\":\"terminal.release\"}\n";

pub const Error = error{
    SpawnFailed,
    ProtocolMismatch,
    NotAttached,
    OutOfMemory,
};

/// Called with raw `herdr api snapshot` stdout when a Session-owned snapshot finishes.
pub const SnapshotHandler = *const fn (*Session, []const u8) void;
/// Called when `workspace focus` / `agent focus` / `pane focus` finishes.
pub const FocusDoneHandler = *const fn (*Session) void;
/// Called once after an idle drain that fed at least one drawable frame.
pub const FramesAppliedHandler = *const fn (*Session) void;

const StreamKind = enum { observe, control, scroll };

pub const Session = struct {
    alloc: Allocator,
    gated: bool = false,
    attaching: bool = false,
    controlling: bool = false,
    focused: []u8 = &.{},
    panes: std.StringHashMapUnmanaged(*Pane) = .empty,
    /// Non-takeover control sessions used only to send `terminal.scroll`.
    scrollers: std.StringHashMapUnmanaged(*Child) = .empty,
    control: ?*Child = null,
    idle_id: c_uint = 0,
    pending_lines: std.ArrayListUnmanaged(PendingLine) = .empty,
    pending_resize: bool = false,
    pending_release_control: bool = false,
    frames_pending: bool = false,
    on_snapshot: ?SnapshotHandler = null,
    on_focus_done: ?FocusDoneHandler = null,
    on_frames_applied: ?FramesAppliedHandler = null,

    pub fn create(alloc: Allocator) !*Session {
        const self = try alloc.create(Session);
        self.* = .{ .alloc = alloc };
        return self;
    }

    pub fn destroy(self: *Session) void {
        if (self.idle_id != 0) _ = g_source_remove(self.idle_id);
        self.drainPendingLines();
        self.pending_lines.deinit(self.alloc);
        var it = self.panes.iterator();
        while (it.next()) |e| {
            e.value_ptr.*.destroy();
            self.alloc.free(e.key_ptr.*);
        }
        self.panes.deinit(self.alloc);
        var sit = self.scrollers.iterator();
        while (sit.next()) |e| {
            e.value_ptr.*.destroy();
            self.alloc.free(e.key_ptr.*);
        }
        self.scrollers.deinit(self.alloc);
        if (self.control) |c| c.destroy();
        self.alloc.free(self.focused);
        self.alloc.destroy(self);
    }

    /// Start one-shot version + schema gate, then snapshot. Never call from size-allocate.
    pub fn attach(self: *Session) void {
        if (self.gated or self.attaching) return;
        self.attaching = true;
        runCli(self, &.{ "herdr", "--version" }, .version);
    }

    /// Refresh snapshot (async). Not for size-allocate.
    pub fn requestSnapshot(self: *Session) void {
        if (!self.gated) return;
        runCli(self, &.{ "herdr", "api", "snapshot" }, .snapshot);
    }

    /// `herdr workspace focus` / `herdr agent focus` — async, never size-allocate.
    pub fn requestFocusWorkspace(self: *Session, workspace_id: []const u8) void {
        const id = self.alloc.dupeZ(u8, workspace_id) catch return;
        defer self.alloc.free(id);
        runCli(self, &.{ "herdr", "workspace", "focus", id }, .focus);
    }

    pub fn requestFocusAgent(self: *Session, pane_id: []const u8) void {
        const id = self.alloc.dupeZ(u8, pane_id) catch return;
        defer self.alloc.free(id);
        runCli(self, &.{ "herdr", "agent", "focus", id }, .focus);
    }

    /// One step of `herdr pane focus --direction`. Snapshot follows via `on_focus_done`.
    pub fn requestFocusPaneDirection(self: *Session, direction: contract.PaneDirection, pane_id: []const u8) void {
        const dir: [*:0]const u8 = switch (direction) {
            .left => "left",
            .right => "right",
            .up => "up",
            .down => "down",
        };
        const id = self.alloc.dupeZ(u8, pane_id) catch return;
        defer self.alloc.free(id);
        runCli(self, &.{ "herdr", "pane", "focus", "--direction", dir, "--pane", id }, .focus);
    }

    /// `git status --porcelain=v2 --branch` in worktree — async, never size-allocate.
    pub fn requestGitStatus(self: *Session, worktree: []const u8) void {
        const path = self.alloc.dupeZ(u8, worktree) catch return;
        defer self.alloc.free(path);
        runCli(self, &.{ "git", "-C", path, "status", "--porcelain=v2", "--branch" }, .git_status);
    }

    /// Sync observe set from visible panes + takeover. Other panes stay observed.
    pub fn syncObservers(
        self: *Session,
        visible: []const []const u8,
        focused: []const u8,
        controlling: bool,
        capacity: ?contract.GridSize,
    ) Error!void {
        if (!self.gated) return error.NotAttached;
        const spawn_size = rules.observe_spawn_size(capacity) orelse return;

        self.alloc.free(self.focused);
        self.focused = self.alloc.dupe(u8, focused) catch return error.OutOfMemory;
        self.controlling = controlling;

        var running: std.ArrayListUnmanaged([]const u8) = .empty;
        defer running.deinit(self.alloc);
        var it = self.panes.iterator();
        while (it.next()) |e| try running.append(self.alloc, e.key_ptr.*);
        var sit = self.scrollers.iterator();
        while (sit.next()) |e| try running.append(self.alloc, e.key_ptr.*);

        const diff = rules.plan_observers(self.alloc, running.items, visible, controlling, focused) catch return error.OutOfMemory;
        defer {
            self.alloc.free(diff.reap);
            self.alloc.free(diff.spawn);
        }

        for (diff.reap) |id| {
            if (self.panes.fetchRemove(id)) |kv| {
                kv.value.destroy();
                self.alloc.free(kv.key);
            }
            self.releaseScroller(id);
        }
        for (diff.spawn) |id| {
            const pane = Pane.spawn(self, id, spawn_size) catch continue;
            const key = self.alloc.dupe(u8, id) catch {
                pane.destroy();
                continue;
            };
            self.panes.put(self.alloc, key, pane) catch {
                self.alloc.free(key);
                pane.destroy();
            };
        }

        if (controlling) {
            if (self.control) |c| {
                if (!std.mem.eql(u8, c.pane_id, focused)) {
                    self.queueReleaseControl();
                }
            }
            if (self.control == null) {
                if (Child.spawnControl(self.alloc, focused, spawn_size, true)) |c| {
                    c.session = self;
                    c.startRead();
                    self.control = c;
                } else |_| {}
            }
        } else if (self.control != null) {
            self.queueReleaseControl();
        }
    }

    /// Queue desired grid; resize stdin write happens on idle — never spawns CLIs.
    pub fn noteDesiredSize(self: *Session, pane_id: []const u8, grid: contract.GridSize) void {
        if (self.panes.get(pane_id)) |pane| {
            if (rules.note_desired(&pane.resize_sync, grid)) |_| {
                self.pending_resize = true;
                self.scheduleIdle();
            }
        }
        if (self.control) |c| {
            if (std.mem.eql(u8, c.pane_id, pane_id)) {
                if (rules.note_desired(&c.resize_sync, grid)) |_| {
                    self.pending_resize = true;
                    self.scheduleIdle();
                }
            }
        }
        if (self.scrollers.get(pane_id)) |c| {
            if (rules.note_desired(&c.resize_sync, grid)) |_| {
                self.pending_resize = true;
                self.scheduleIdle();
            }
        }
    }

    pub fn engineFor(self: *Session, pane_id: []const u8) ?*engine.Engine {
        // Control owns the paint engine while takeover is active (observe Pane is reaped).
        if (self.control) |c| {
            if (std.mem.eql(u8, c.pane_id, pane_id)) {
                if (c.eng) |*eng| return eng;
            }
        }
        if (self.scrollers.get(pane_id)) |c| {
            if (c.eng) |*eng| return eng;
        }
        const pane = self.panes.get(pane_id) orelse return null;
        return &pane.eng;
    }

    /// Write one `terminal.input` line to the control child's stdin. No-op if not controlling that pane.
    pub fn writeInput(self: *Session, pane_id: []const u8, payload: input_mod.KeyInput) void {
        const c = self.control orelse return;
        if (!std.mem.eql(u8, c.pane_id, pane_id)) return;
        c.writeInput(payload);
    }

    /// `terminal.scroll` on the takeover control, or a short-lived non-takeover control.
    pub fn writeScroll(self: *Session, pane_id: []const u8, cmd: contract.ScrollCommand, size: contract.GridSize) void {
        if (cmd.lines == 0 or pane_id.len == 0) return;
        if (self.control) |c| {
            if (std.mem.eql(u8, c.pane_id, pane_id)) {
                c.writeScroll(cmd);
                return;
            }
        }
        if (self.scrollers.get(pane_id) == null) {
            if (self.panes.fetchRemove(pane_id)) |kv| {
                kv.value.destroy();
                self.alloc.free(kv.key);
            }
            const spawned = Child.spawnControl(self.alloc, pane_id, size, false) catch return;
            spawned.session = self;
            spawned.is_scroller = true;
            const key = self.alloc.dupe(u8, pane_id) catch {
                spawned.destroy();
                return;
            };
            self.scrollers.put(self.alloc, key, spawned) catch {
                self.alloc.free(key);
                spawned.destroy();
                return;
            };
            spawned.startRead();
        }
        if (self.scrollers.get(pane_id)) |c| c.writeScroll(cmd);
    }

    fn releaseScroller(self: *Session, pane_id: []const u8) void {
        if (self.scrollers.fetchRemove(pane_id)) |kv| {
            kv.value.destroy();
            self.alloc.free(kv.key);
        }
    }

    fn scheduleIdle(self: *Session) void {
        if (self.idle_id != 0) return;
        self.idle_id = g_idle_add(&onIdle, self);
    }

    fn queueReleaseControl(self: *Session) void {
        self.pending_release_control = true;
        self.scheduleIdle();
    }

    fn drainPendingLines(self: *Session) void {
        for (self.pending_lines.items) |pl| {
            self.alloc.free(pl.pane_id);
            self.alloc.free(pl.line);
        }
        self.pending_lines.clearRetainingCapacity();
    }

    fn pushLine(self: *Session, pane_id: []const u8, line: []const u8, kind: StreamKind) void {
        const pid = self.alloc.dupe(u8, pane_id) catch return;
        const dup = self.alloc.dupe(u8, line) catch {
            self.alloc.free(pid);
            return;
        };
        self.pending_lines.append(self.alloc, .{
            .pane_id = pid,
            .line = dup,
            .kind = kind,
        }) catch {
            self.alloc.free(pid);
            self.alloc.free(dup);
            return;
        };
        self.scheduleIdle();
    }

    fn onIdle(data: ?*anyopaque) callconv(.c) c_int {
        const self: *Session = @ptrCast(@alignCast(data.?));
        self.idle_id = 0;
        self.frames_pending = false;

        // feed
        var i: usize = 0;
        while (i < self.pending_lines.items.len) : (i += 1) {
            const pl = self.pending_lines.items[i];
            defer {
                self.alloc.free(pl.pane_id);
                self.alloc.free(pl.line);
            }
            self.handleLine(pl.pane_id, pl.line, pl.kind);
        }
        self.pending_lines.clearRetainingCapacity();

        // resize
        if (self.pending_resize) {
            self.pending_resize = false;
            if (self.control) |c| {
                if (rules.next_pending(&c.resize_sync)) |g| {
                    c.writeResize(g);
                }
            }
            var sit = self.scrollers.iterator();
            while (sit.next()) |e| {
                if (rules.next_pending(&e.value_ptr.*.resize_sync)) |g| {
                    e.value_ptr.*.writeResize(g);
                }
            }
            var it = self.panes.iterator();
            while (it.next()) |e| {
                _ = rules.next_pending(&e.value_ptr.*.resize_sync);
                // observe-only: size applied on respawn, not stdin
            }
        }

        // release control
        if (self.pending_release_control) {
            self.pending_release_control = false;
            if (self.control) |c| {
                c.releaseAndKill();
                self.control = null;
            }
        }

        if (self.frames_pending) {
            if (self.on_frames_applied) |cb| cb(self);
        }

        return 0;
    }

    fn handleLine(self: *Session, pane_id: []const u8, line: []const u8, kind: StreamKind) void {
        if (kind == .scroll) {
            self.handleScrollLine(pane_id, line);
            return;
        }
        if (kind == .control) {
            const c = self.control orelse return;
            if (!std.mem.eql(u8, c.pane_id, pane_id)) return;
            const ev = c.parser.ingest(self.alloc, line) catch |err| {
                if (err == error.SequenceResyncRequired) {
                    const grid = rules.confirmed(&c.resize_sync);
                    const cell_w = c.resize_sync.confirmed.cell_w;
                    const cell_h = c.resize_sync.confirmed.cell_h;
                    c.releaseAndKill();
                    self.control = null;
                    if (Child.spawnControl(self.alloc, pane_id, .{
                        .cols = grid[0],
                        .rows = grid[1],
                        .cell_w = cell_w,
                        .cell_h = cell_h,
                    }, true)) |nc| {
                        nc.session = self;
                        nc.startRead();
                        self.control = nc;
                    } else |_| {}
                }
                return;
            };
            defer freeEvent(self.alloc, ev);
            switch (ev) {
                .closed => self.queueReleaseControl(),
                .data => |fb| {
                    const fit = rules.on_frame(&c.resize_sync, fb.cols, fb.rows);
                    if (fit == .acknowledged) {
                        if (rules.next_pending(&c.resize_sync)) |_| self.pending_resize = true;
                    }
                    if (c.eng) |*eng| {
                        const conf = rules.confirmed(&c.resize_sync);
                        switch (rules.classify_observe_frame(fb.full, fb.cols, fb.rows, conf[0], conf[1])) {
                            .draw => {
                                eng.resize(fb.cols, fb.rows) catch {};
                                eng.feed(fb.bytes, fb.full) catch {};
                                self.frames_pending = true;
                            },
                            .skip => {},
                            .disconnect => {
                                const grid = rules.confirmed(&c.resize_sync);
                                const cell_w = c.resize_sync.confirmed.cell_w;
                                const cell_h = c.resize_sync.confirmed.cell_h;
                                c.releaseAndKill();
                                self.control = null;
                                if (Child.spawnControl(self.alloc, pane_id, .{
                                    .cols = grid[0],
                                    .rows = grid[1],
                                    .cell_w = cell_w,
                                    .cell_h = cell_h,
                                }, true)) |nc| {
                                    nc.session = self;
                                    nc.startRead();
                                    self.control = nc;
                                } else |_| {}
                            },
                        }
                    }
                },
            }
            return;
        }

        const pane = self.panes.get(pane_id) orelse return;
        const ev = pane.parser.ingest(self.alloc, line) catch |err| {
            if (err == error.SequenceResyncRequired) {
                const grid = pane.resize_sync.confirmed;
                pane.respawn(grid) catch {};
            }
            return;
        };
        defer freeEvent(self.alloc, ev);
        switch (ev) {
            .closed => {
                if (self.panes.fetchRemove(pane_id)) |kv| {
                    kv.value.destroy();
                    self.alloc.free(kv.key);
                }
            },
            .data => |fb| self.applyFrame(pane, fb),
        }
    }

    fn handleScrollLine(self: *Session, pane_id: []const u8, line: []const u8) void {
        const c = self.scrollers.get(pane_id) orelse return;
        const ev = c.parser.ingest(self.alloc, line) catch |err| {
            if (err == error.SequenceResyncRequired) self.releaseScroller(pane_id);
            return;
        };
        defer freeEvent(self.alloc, ev);
        switch (ev) {
            .closed => self.releaseScroller(pane_id),
            .data => |fb| {
                const eng = &(c.eng orelse return);
                const conf = rules.confirmed(&c.resize_sync);
                switch (rules.classify_observe_frame(fb.full, fb.cols, fb.rows, conf[0], conf[1])) {
                    .draw => {
                        _ = rules.on_frame(&c.resize_sync, fb.cols, fb.rows);
                        eng.resize(fb.cols, fb.rows) catch {};
                        eng.feed(fb.bytes, fb.full) catch {};
                        self.frames_pending = true;
                    },
                    .skip => {},
                    .disconnect => self.releaseScroller(pane_id),
                }
            },
        }
    }

    fn applyFrame(self: *Session, pane: *Pane, fb: contract.FrameBytes) void {
        const conf = rules.confirmed(&pane.resize_sync);
        switch (rules.classify_observe_frame(fb.full, fb.cols, fb.rows, conf[0], conf[1])) {
            .draw => {
                _ = rules.on_frame(&pane.resize_sync, fb.cols, fb.rows);
                pane.eng.resize(fb.cols, fb.rows) catch {};
                pane.eng.feed(fb.bytes, fb.full) catch {};
                self.frames_pending = true;
            },
            .skip => {},
            .disconnect => {
                const grid = pane.resize_sync.confirmed;
                pane.respawn(grid) catch {};
            },
        }
    }

    fn onCliDone(self: *Session, kind: CliKind, stdout: []const u8) void {
        switch (kind) {
            .version => {
                const ver = parseHerdrVersion(stdout) orelse {
                    self.attaching = false;
                    return;
                };
                const ver_owned = self.alloc.dupe(u8, ver) catch {
                    self.attaching = false;
                    return;
                };
                // stash version in focused temporarily unused — use pending via heap tag
                const ctx = self.alloc.create(AttachCtx) catch {
                    self.alloc.free(ver_owned);
                    self.attaching = false;
                    return;
                };
                ctx.* = .{ .session = self, .version = ver_owned };
                runCliCtx(ctx, &.{ "herdr", "api", "schema", "--json" }, .schema);
            },
            .schema => {}, // handled via AttachCtx path
            .snapshot => {
                if (self.on_snapshot) |cb| cb(self, stdout);
            },
            .focus => {
                if (self.on_focus_done) |cb| cb(self);
            },
            .git_status => {},
        }
    }

    fn onSchemaDone(ctx: *AttachCtx, stdout: []const u8) void {
        const self = ctx.session;
        defer {
            self.alloc.free(ctx.version);
            self.alloc.destroy(ctx);
        }
        protocol.checkProtocolGate(ctx.version, stdout) catch {
            self.attaching = false;
            return;
        };
        self.gated = true;
        self.attaching = false;
        runCli(self, &.{ "herdr", "api", "snapshot" }, .snapshot);
    }
};

const PendingLine = struct {
    pane_id: []u8,
    line: []u8,
    kind: StreamKind,
};

const CliKind = enum { version, schema, snapshot, focus, git_status };

const AttachCtx = struct {
    session: *Session,
    version: []u8,
};

const CliReq = struct {
    session: *Session,
    kind: CliKind,
    proc: *gio.Subprocess,
    attach: ?*AttachCtx = null,
};

fn freeEvent(alloc: Allocator, ev: contract.TerminalEvent) void {
    switch (ev) {
        .data => |fb| alloc.free(fb.bytes),
        .closed => |c| alloc.free(c.reason),
    }
}

fn parseHerdrVersion(stdout: []const u8) ?[]const u8 {
    const t = std.mem.trim(u8, stdout, " \t\r\n");
    if (std.mem.startsWith(u8, t, "herdr ")) return t["herdr ".len..];
    return null;
}

fn runCli(session: *Session, argv: []const [*:0]const u8, kind: CliKind) void {
    runCliInner(session, null, argv, kind);
}

fn runCliCtx(ctx: *AttachCtx, argv: []const [*:0]const u8, kind: CliKind) void {
    runCliInner(ctx.session, ctx, argv, kind);
}

fn runCliInner(session: *Session, attach: ?*AttachCtx, argv: []const [*:0]const u8, kind: CliKind) void {
    var buf: [16]?[*:0]const u8 = undefined;
    if (argv.len >= buf.len) return;
    for (argv, 0..) |a, i| buf[i] = a;
    buf[argv.len] = null;

    const flags = gio.SubprocessFlags{ .stdout_pipe = true, .stderr_pipe = true };
    const proc = gio.Subprocess.newv(@ptrCast(&buf), flags, null) orelse return;
    const req = session.alloc.create(CliReq) catch {
        proc.unref();
        return;
    };
    req.* = .{ .session = session, .kind = kind, .proc = proc, .attach = attach };
    gio.Subprocess.communicateUtf8Async(proc, null, null, @ptrCast(&onCliAsync), req);
}

fn onCliAsync(_: ?*anyopaque, res: *gio.AsyncResult, data: ?*anyopaque) callconv(.c) void {
    const req: *CliReq = @ptrCast(@alignCast(data.?));
    const session = req.session;
    defer {
        req.proc.unref();
        session.alloc.destroy(req);
    }
    var stdout_z: ?[*:0]u8 = null;
    var stderr_z: ?[*:0]u8 = null;
    if (gio.Subprocess.communicateUtf8Finish(req.proc, res, @ptrCast(&stdout_z), @ptrCast(&stderr_z), null) == 0) {
        if (stdout_z) |s| g_free(s);
        if (stderr_z) |s| g_free(s);
        if (req.kind == .version or req.kind == .schema) session.attaching = false;
        // Still advance UI after a failed focus so we do not wait on the 2s poll.
        if (req.kind == .focus) {
            if (session.on_focus_done) |cb| cb(session);
        }
        return;
    }
    defer if (stdout_z) |s| g_free(s);
    defer if (stderr_z) |s| g_free(s);
    const out: []const u8 = if (stdout_z) |s| std.mem.span(s) else "";
    if (req.kind == .schema) {
        if (req.attach) |ctx| {
            Session.onSchemaDone(ctx, out);
            return;
        }
    }
    session.onCliDone(req.kind, out);
}

const Pane = struct {
    session: *Session,
    pane_id: []u8,
    child: ?*Child,
    parser: ndjson.FrameParser = .{},
    resize_sync: contract.ResizeSync,
    eng: engine.Engine,

    fn spawn(session: *Session, pane_id: []const u8, grid: contract.GridSize) !*Pane {
        const pane = try session.alloc.create(Pane);
        errdefer session.alloc.destroy(pane);
        const id = try session.alloc.dupe(u8, pane_id);
        errdefer session.alloc.free(id);
        const eng = try engine.Engine.init(grid.cols, grid.rows);
        const child = try Child.spawnObserve(session.alloc, pane_id, grid);
        pane.* = .{
            .session = session,
            .pane_id = id,
            .child = child,
            .resize_sync = rules.resize_sync_new(grid),
            .eng = eng,
        };
        child.owner_pane = pane;
        child.startRead();
        return pane;
    }

    fn destroy(self: *Pane) void {
        if (self.child) |c| {
            c.forceKill();
            self.child = null;
        }
        self.eng.deinit();
        self.session.alloc.free(self.pane_id);
        self.session.alloc.destroy(self);
    }

    fn respawn(self: *Pane, grid: contract.GridSize) !void {
        if (self.child) |c| {
            c.forceKill();
            self.child = null;
        }
        self.parser = .{};
        self.resize_sync = rules.resize_sync_new(grid);
        try self.eng.resize(grid.cols, grid.rows);
        const child = try Child.spawnObserve(self.session.alloc, self.pane_id, grid);
        child.owner_pane = self;
        self.child = child;
        child.startRead();
    }
};

const Child = struct {
    alloc: Allocator,
    pane_id: []u8,
    proc: *gio.Subprocess,
    din: *gio.DataInputStream,
    stdin: ?*gio.OutputStream,
    cancellable: *gio.Cancellable,
    parser: ndjson.FrameParser = .{},
    resize_sync: contract.ResizeSync,
    is_control: bool,
    is_scroller: bool = false,
    /// Paint source while controlling; observe children use Pane.eng instead.
    eng: ?engine.Engine = null,
    owner_pane: ?*Pane = null,
    session: ?*Session = null,
    reading: bool = false,
    closing: bool = false,

    fn spawnObserve(alloc: Allocator, pane_id: []const u8, grid: contract.GridSize) !*Child {
        return spawn(alloc, pane_id, grid, false, false);
    }

    fn spawnControl(alloc: Allocator, pane_id: []const u8, grid: contract.GridSize, takeover: bool) !*Child {
        const c = try spawn(alloc, pane_id, grid, true, takeover);
        return c;
    }

    fn spawn(alloc: Allocator, pane_id: []const u8, grid: contract.GridSize, control: bool, takeover: bool) !*Child {
        var cols_buf: [8]u8 = undefined;
        var rows_buf: [8]u8 = undefined;
        const cols_z = try std.fmt.bufPrintZ(&cols_buf, "{d}", .{grid.cols});
        const rows_z = try std.fmt.bufPrintZ(&rows_buf, "{d}", .{grid.rows});
        const id_z = try alloc.dupeZ(u8, pane_id);
        defer alloc.free(id_z);

        var argv_buf: [12]?[*:0]const u8 = undefined;
        var n: usize = 0;
        argv_buf[n] = "herdr";
        n += 1;
        argv_buf[n] = "terminal";
        n += 1;
        argv_buf[n] = "session";
        n += 1;
        argv_buf[n] = if (control) "control" else "observe";
        n += 1;
        argv_buf[n] = id_z.ptr;
        n += 1;
        argv_buf[n] = "--cols";
        n += 1;
        argv_buf[n] = cols_z.ptr;
        n += 1;
        argv_buf[n] = "--rows";
        n += 1;
        argv_buf[n] = rows_z.ptr;
        n += 1;
        if (control and takeover) {
            argv_buf[n] = "--takeover";
            n += 1;
        }
        argv_buf[n] = null;

        var flags: gio.SubprocessFlags = .{ .stdout_pipe = true, .stderr_pipe = true };
        if (control) flags.stdin_pipe = true;

        const proc = gio.Subprocess.newv(@ptrCast(&argv_buf), flags, null) orelse return error.SpawnFailed;
        errdefer proc.unref();

        const stdout = proc.getStdoutPipe() orelse return error.SpawnFailed;
        const din = gio.DataInputStream.new(stdout);
        errdefer din.unref();

        const cancellable = gio.Cancellable.new();
        errdefer cancellable.unref();

        const id = try alloc.dupe(u8, pane_id);
        errdefer alloc.free(id);

        var eng: ?engine.Engine = if (control) try engine.Engine.init(grid.cols, grid.rows) else null;
        errdefer if (eng) |*e| e.deinit();

        const child = try alloc.create(Child);
        child.* = .{
            .alloc = alloc,
            .pane_id = id,
            .proc = proc,
            .din = din,
            .stdin = if (control) proc.getStdinPipe() else null,
            .cancellable = cancellable,
            .resize_sync = rules.resize_sync_new(grid),
            .is_control = control,
            .eng = eng,
        };
        eng = null; // ownership moved to child
        return child;
    }

    fn destroy(self: *Child) void {
        self.releaseAndKill();
    }

    fn startRead(self: *Child) void {
        if (self.reading or self.closing) return;
        self.reading = true;
        gio.DataInputStream.readLineAsync(self.din, 0, self.cancellable, @ptrCast(&onLine), self);
    }

    fn onLine(_: ?*anyopaque, res: *gio.AsyncResult, data: ?*anyopaque) callconv(.c) void {
        const self: *Child = @ptrCast(@alignCast(data.?));
        var len: usize = 0;
        var gerr: ?*GError = null;
        const line_z = gio.DataInputStream.readLineFinishUtf8(self.din, res, &len, @ptrCast(&gerr));
        if (gerr) |e| g_error_free(e);
        self.reading = false;
        if (self.closing) {
            if (line_z) |p| g_free(p);
            self.finishFree();
            return;
        }
        if (line_z == null) return;
        defer g_free(line_z);
        const line = line_z.?[0..len];

        if (self.owner_pane) |pane| {
            pane.session.pushLine(self.pane_id, line, .observe);
        } else if (self.session) |sess| {
            sess.pushLine(self.pane_id, line, if (self.is_scroller) .scroll else .control);
        }
        // never log frame bytes
        self.startRead();
    }

    fn writeResize(self: *Child, grid: contract.GridSize) void {
        const stdin = self.stdin orelse return;
        var buf: [128]u8 = undefined;
        const msg = std.fmt.bufPrint(&buf, "{{\"type\":\"terminal.resize\",\"cols\":{d},\"rows\":{d},\"cell_width_px\":{d},\"cell_height_px\":{d}}}\n", .{
            grid.cols,
            grid.rows,
            grid.cell_w,
            grid.cell_h,
        }) catch return;
        var written: usize = 0;
        _ = gio.OutputStream.writeAll(stdin, @constCast(msg.ptr), msg.len, &written, null, null);
        _ = gio.OutputStream.flush(stdin, null, null);
    }

    fn writeScroll(self: *Child, cmd: contract.ScrollCommand) void {
        const stdin = self.stdin orelse return;
        var buf: [256]u8 = undefined;
        const msg = ndjson.format_scroll_line(&buf, cmd) orelse return;
        var written: usize = 0;
        _ = gio.OutputStream.writeAll(stdin, @constCast(msg.ptr), msg.len, &written, null, null);
        _ = gio.OutputStream.flush(stdin, null, null);
    }

    fn writeInput(self: *Child, payload: input_mod.KeyInput) void {
        const stdin = self.stdin orelse return;
        var line_buf: [512]u8 = undefined;
        const msg: []const u8 = switch (payload) {
            .text => |t| blk: {
                var i: usize = 0;
                const prefix = "{\"type\":\"terminal.input\",\"text\":\"";
                const suffix = "\"}\n";
                if (prefix.len + t.len * 2 + suffix.len > line_buf.len) return;
                @memcpy(line_buf[0..prefix.len], prefix);
                i = prefix.len;
                for (t) |c| {
                    if (c == '\\' or c == '"') {
                        line_buf[i] = '\\';
                        i += 1;
                    }
                    line_buf[i] = c;
                    i += 1;
                }
                @memcpy(line_buf[i..][0..suffix.len], suffix);
                break :blk line_buf[0 .. i + suffix.len];
            },
            .bytes => |b| blk: {
                var b64_buf: [256]u8 = undefined;
                const b64_len = std.base64.standard.Encoder.calcSize(b.len);
                if (b64_len > b64_buf.len) return;
                const b64 = std.base64.standard.Encoder.encode(b64_buf[0..b64_len], b);
                break :blk std.fmt.bufPrint(&line_buf, "{{\"type\":\"terminal.input\",\"bytes\":\"{s}\"}}\n", .{b64}) catch return;
            },
        };
        var written: usize = 0;
        _ = gio.OutputStream.writeAll(stdin, @constCast(msg.ptr), msg.len, &written, null, null);
        _ = gio.OutputStream.flush(stdin, null, null);
    }

    fn writeRelease(self: *Child) void {
        const stdin = self.stdin orelse return;
        var written: usize = 0;
        _ = gio.OutputStream.writeAll(stdin, @constCast(RELEASE_LINE.ptr), RELEASE_LINE.len, &written, null, null);
        _ = gio.OutputStream.flush(stdin, null, null);
        _ = gio.OutputStream.close(stdin, null, null);
        self.stdin = null;
    }

    fn releaseAndKill(self: *Child) void {
        self.beginClose(true);
    }

    fn forceKill(self: *Child) void {
        self.beginClose(false);
    }

    /// Drop the process, then free this child only after the in-flight
    /// `readLineAsync` callback has finished. Cancelling that read can run
    /// `onLine` before this returns, and that callback frees `self`.
    fn beginClose(self: *Child, release: bool) void {
        if (self.closing) return;
        self.closing = true;
        self.owner_pane = null;
        self.session = null;
        if (release) self.writeRelease();
        if (!release) {
            self.proc.forceExit();
        } else {
            var waited: u64 = 0;
            while (waited < RELEASE_WAIT_MS) : (waited += 20) {
                if (self.proc.getIdentifier() == null) break;
                var rem: std.os.linux.timespec = undefined;
                var req_ts: std.os.linux.timespec = .{ .sec = 0, .nsec = 20 * std.time.ns_per_ms };
                _ = std.os.linux.nanosleep(&req_ts, &rem);
            }
            if (self.proc.getIdentifier() != null) self.proc.forceExit();
        }
        _ = self.proc.wait(null, null);
        const reading = self.reading;
        self.cancellable.cancel();
        if (!reading) self.finishFree();
    }

    fn finishFree(self: *Child) void {
        if (self.eng) |*eng| {
            eng.deinit();
            self.eng = null;
        }
        self.din.unref();
        self.proc.unref();
        self.cancellable.unref();
        self.alloc.free(self.pane_id);
        self.alloc.destroy(self);
    }
};

// Silence unused import when layout is only for Wave 3 callers via re-export.
pub const tabLayout = layout.tabLayout;
pub const visiblePanes = layout.visiblePanes;
