//! Workspace names to the left of the terminal card. The terminal widget never
//! sees this list.

const std = @import("std");
const contract = @import("contract");
const gtk = @import("gtk");
const theme = @import("theme");

pub const WorkspaceSidebar = struct {
    widget: *gtk.Box,
    list: *gtk.ListBox,
    ids: std.ArrayListUnmanaged([]const u8) = .empty,
    marked: ?[]const u8 = null,
    on_activated: ?*const fn ([]const u8) void = null,
    allocator: std.mem.Allocator,

    pub fn new(allocator: std.mem.Allocator) *WorkspaceSidebar {
        const self = allocator.create(WorkspaceSidebar) catch @panic("oom");
        const list = gtk.ListBox.new();
        list.setSelectionMode(.single);
        list.as(gtk.Widget).addCssClass("herdr-rows");
        list.as(gtk.Widget).setVexpand(1);
        list.as(gtk.Widget).setHexpand(1);

        const scroll = gtk.ScrolledWindow.new();
        scroll.setPolicy(.never, .automatic);
        scroll.as(gtk.Widget).setHexpand(1);
        scroll.as(gtk.Widget).setVexpand(1);
        scroll.setChild(list.as(gtk.Widget));
        scroll.as(gtk.Widget).addCssClass("herdr-scroll");

        const header = gtk.Label.new("Workspaces");
        header.setXalign(0.0);
        header.as(gtk.Widget).setHalign(.fill);
        header.as(gtk.Widget).addCssClass("herdr-card-header");

        const widget = gtk.Box.new(.vertical, 0);
        widget.as(gtk.Widget).addCssClass(theme.CSS_CARD_CLASS);
        widget.as(gtk.Widget).setSizeRequest(160, -1);
        widget.as(gtk.Widget).setHexpand(1);
        widget.as(gtk.Widget).setVexpand(1);
        widget.as(gtk.Widget).setOverflow(.hidden);
        widget.append(header.as(gtk.Widget));
        widget.append(scroll.as(gtk.Widget));

        self.* = .{
            .widget = widget,
            .list = list,
            .allocator = allocator,
        };
        _ = gtk.ListBox.signals.row_activated.connect(
            list,
            *WorkspaceSidebar,
            &onRowActivated,
            self,
            .{},
        );
        return self;
    }

    pub fn setRows(self: *WorkspaceSidebar, rows: []const contract.WorkspaceRow, focused_workspace_id: ?[]const u8) void {
        self.list.removeAll();
        for (self.ids.items) |id| self.allocator.free(id);
        self.ids.clearRetainingCapacity();
        if (self.marked) |m| {
            self.allocator.free(m);
            self.marked = null;
        }

        for (rows) |row| {
            const list_row = gtk.ListBoxRow.new();
            list_row.as(gtk.Widget).addCssClass("herdr-row");
            list_row.as(gtk.Widget).addCssClass("herdr-nav");

            const title = gtk.Label.new(null);
            {
                const z = self.allocator.dupeZ(u8, row.label) catch continue;
                defer self.allocator.free(z);
                title.setLabel(z);
            }
            title.as(gtk.Widget).setHalign(.start);
            title.as(gtk.Widget).setHexpand(1);
            title.setEllipsize(.end);
            title.as(gtk.Widget).addCssClass("herdr-title");

            const text = gtk.Box.new(.vertical, 1);
            text.as(gtk.Widget).setHexpand(1);
            text.as(gtk.Widget).setValign(.center);
            text.append(title.as(gtk.Widget));

            const branch = if (row.branch) |b| if (b.len > 0) b else null else null;
            const git_status = if (row.git_status) |s| if (s.len > 0) s else null else null;
            if (branch != null or git_status != null) {
                var meta_buf: [256]u8 = undefined;
                const meta = blk: {
                    if (branch) |b| {
                        if (git_status) |s| break :blk std.fmt.bufPrintZ(&meta_buf, "{s}  {s}", .{ b, s }) catch null;
                        break :blk std.fmt.bufPrintZ(&meta_buf, "{s}", .{b}) catch null;
                    }
                    break :blk std.fmt.bufPrintZ(&meta_buf, "{s}", .{git_status.?}) catch null;
                };
                if (meta) |m| {
                    const second = gtk.Label.new(m);
                    second.as(gtk.Widget).setHalign(.start);
                    second.as(gtk.Widget).setHexpand(1);
                    second.setEllipsize(.end);
                    second.as(gtk.Widget).addCssClass("herdr-meta");
                    text.append(second.as(gtk.Widget));
                }
            }

            const dot = gtk.Box.new(.horizontal, 0);
            dot.as(gtk.Widget).addCssClass("herdr-status");
            dot.as(gtk.Widget).addCssClass(switch (row.agent_status) {
                .idle => "herdr-status-idle",
                .working => "herdr-status-working",
                .blocked => "herdr-status-blocked",
                .done => "herdr-status-done",
                .unknown => "herdr-status-unknown",
            });
            dot.as(gtk.Widget).setSizeRequest(8, 8);
            dot.as(gtk.Widget).setValign(.center);
            dot.as(gtk.Widget).setHalign(.center);

            const head = gtk.Box.new(.horizontal, 8);
            head.append(dot.as(gtk.Widget));
            head.append(text.as(gtk.Widget));
            list_row.setChild(head.as(gtk.Widget));

            if (focused_workspace_id) |fid| {
                if (std.mem.eql(u8, row.workspace_id, fid)) {
                    list_row.as(gtk.Widget).addCssClass("herdr-selected");
                }
            }
            self.list.append(list_row.as(gtk.Widget));
            const id = self.allocator.dupe(u8, row.workspace_id) catch continue;
            self.ids.append(self.allocator, id) catch {
                self.allocator.free(id);
            };
        }
        if (focused_workspace_id) |fid| {
            self.marked = self.allocator.dupe(u8, fid) catch null;
        }
        self.selectMarked();
    }

    pub fn restoreMark(self: *WorkspaceSidebar) void {
        self.selectMarked();
    }

    pub fn connectActivated(self: *WorkspaceSidebar, callback: *const fn ([]const u8) void) void {
        self.on_activated = callback;
    }

    fn selectMarked(self: *WorkspaceSidebar) void {
        const marked = self.marked orelse {
            self.list.selectRow(null);
            return;
        };
        for (self.ids.items, 0..) |id, i| {
            if (std.mem.eql(u8, id, marked)) {
                self.list.selectRow(self.list.getRowAtIndex(@intCast(i)));
                return;
            }
        }
        self.list.selectRow(null);
    }

    fn onRowActivated(_: *gtk.ListBox, row: *gtk.ListBoxRow, self: *WorkspaceSidebar) callconv(.c) void {
        const index = row.getIndex();
        if (index < 0) return;
        const i: usize = @intCast(index);
        if (i >= self.ids.items.len) return;
        const id = self.ids.items[i];
        if (self.marked) |m| {
            if (std.mem.eql(u8, m, id)) return;
        }
        if (self.on_activated) |cb| cb(id);
    }
};
