//! Agents under the workspace list. The terminal widget never sees this list.

const std = @import("std");
const contract = @import("contract");
const gtk = @import("gtk");
const theme = @import("theme");

pub const AgentPanel = struct {
    widget: *gtk.Box,
    list: *gtk.ListBox,
    slots: std.ArrayListUnmanaged(?[]const u8) = .empty,
    marked: ?[]const u8 = null,
    on_activated: ?*const fn ([]const u8) void = null,
    allocator: std.mem.Allocator,

    pub fn new(allocator: std.mem.Allocator) *AgentPanel {
        const self = allocator.create(AgentPanel) catch @panic("oom");
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

        const header = gtk.Label.new("Agents");
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
            *AgentPanel,
            &onRowActivated,
            self,
            .{},
        );
        return self;
    }

    pub fn setRows(self: *AgentPanel, rows: []const contract.AgentRow, focused_pane_id: ?[]const u8) void {
        self.list.removeAll();
        for (self.slots.items) |slot| {
            if (slot) |id| self.allocator.free(id);
        }
        self.slots.clearRetainingCapacity();
        if (self.marked) |m| {
            self.allocator.free(m);
            self.marked = null;
        }

        var group: ?[]const u8 = null;
        for (rows) |row| {
            if (group == null or !std.mem.eql(u8, group.?, row.workspace_id)) {
                group = row.workspace_id;
                const title = gtk.Label.new(null);
                {
                    const z = self.allocator.dupeZ(u8, row.workspace_label) catch continue;
                    defer self.allocator.free(z);
                    title.setLabel(z);
                }
                title.as(gtk.Widget).setHalign(.start);
                title.as(gtk.Widget).setMarginTop(8);
                title.as(gtk.Widget).setMarginBottom(2);
                title.as(gtk.Widget).setMarginStart(8);
                title.as(gtk.Widget).setMarginEnd(8);
                title.as(gtk.Widget).addCssClass("dim-label");
                const header_row = gtk.ListBoxRow.new();
                header_row.setChild(title.as(gtk.Widget));
                header_row.setActivatable(0);
                header_row.setSelectable(0);
                self.list.append(header_row.as(gtk.Widget));
                self.slots.append(self.allocator, null) catch {};
            }

            var place_buf: [256]u8 = undefined;
            const place_z: [:0]const u8 = blk: {
                if (row.workspace_label.len > 0 and row.tab_label.len > 0) {
                    break :blk std.fmt.bufPrintZ(&place_buf, "{s}  {s}", .{ row.workspace_label, row.tab_label }) catch "pane";
                }
                if (row.workspace_label.len > 0) {
                    break :blk std.fmt.bufPrintZ(&place_buf, "{s}", .{row.workspace_label}) catch "pane";
                }
                if (row.tab_label.len > 0) {
                    break :blk std.fmt.bufPrintZ(&place_buf, "{s}", .{row.tab_label}) catch "pane";
                }
                break :blk std.fmt.bufPrintZ(&place_buf, "{s}", .{row.pane_id}) catch "pane";
            };

            const title = gtk.Label.new(place_z);
            title.as(gtk.Widget).setHalign(.start);
            title.as(gtk.Widget).setHexpand(1);
            title.setEllipsize(.end);
            title.as(gtk.Widget).addCssClass("herdr-title");

            const text = gtk.Box.new(.vertical, 1);
            text.as(gtk.Widget).setHexpand(1);
            text.as(gtk.Widget).setValign(.center);
            text.append(title.as(gtk.Widget));
            if (row.identity.len > 0) {
                const identity = gtk.Label.new(null);
                {
                    const z = self.allocator.dupeZ(u8, row.identity) catch continue;
                    defer self.allocator.free(z);
                    identity.setLabel(z);
                }
                identity.as(gtk.Widget).setHalign(.start);
                identity.as(gtk.Widget).setHexpand(1);
                identity.setEllipsize(.end);
                identity.as(gtk.Widget).addCssClass("herdr-meta");
                text.append(identity.as(gtk.Widget));
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

            const list_row = gtk.ListBoxRow.new();
            list_row.as(gtk.Widget).addCssClass("herdr-row");
            list_row.as(gtk.Widget).addCssClass("herdr-nav");
            list_row.setChild(head.as(gtk.Widget));
            if (focused_pane_id) |fid| {
                if (std.mem.eql(u8, row.pane_id, fid)) {
                    list_row.as(gtk.Widget).addCssClass("herdr-selected");
                }
            }
            self.list.append(list_row.as(gtk.Widget));
            const id = self.allocator.dupe(u8, row.pane_id) catch continue;
            self.slots.append(self.allocator, id) catch {
                self.allocator.free(id);
            };
        }
        if (focused_pane_id) |fid| {
            self.marked = self.allocator.dupe(u8, fid) catch null;
        }
        self.selectMarked();
    }

    pub fn restoreMark(self: *AgentPanel) void {
        self.selectMarked();
    }

    pub fn connectActivated(self: *AgentPanel, callback: *const fn ([]const u8) void) void {
        self.on_activated = callback;
    }

    fn selectMarked(self: *AgentPanel) void {
        const marked = self.marked orelse {
            self.list.selectRow(null);
            return;
        };
        for (self.slots.items, 0..) |slot, i| {
            if (slot) |id| {
                if (std.mem.eql(u8, id, marked)) {
                    self.list.selectRow(self.list.getRowAtIndex(@intCast(i)));
                    return;
                }
            }
        }
        self.list.selectRow(null);
    }

    fn onRowActivated(_: *gtk.ListBox, row: *gtk.ListBoxRow, self: *AgentPanel) callconv(.c) void {
        const index = row.getIndex();
        if (index < 0) return;
        const i: usize = @intCast(index);
        if (i >= self.slots.items.len) return;
        const id = self.slots.items[i] orelse return;
        if (self.marked) |m| {
            if (std.mem.eql(u8, m, id)) return;
        }
        if (self.on_activated) |cb| cb(id);
    }
};
