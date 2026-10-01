//! Native GTK cards for the layout tree when enhance is on.

const std = @import("std");
const contract = @import("contract");
const gtk = @import("gtk");
const tree = @import("tree");
const cards = @import("cards");
const overlay_paint = @import("overlay_paint");

const Allocator = std.mem.Allocator;

const Layer = enum { layout, dialog };

const Slot = struct {
    card: *gtk.Box,
    header: *gtk.Box,
    title: *gtk.Label,
    count: *gtk.Label,
    body: *gtk.Box,
    fade: *gtk.Widget,
    geom: cards.CardSlot,
    signature: []u8,
    layer: Layer,
    ring: ?[:0]u8,
};

pub const PaneGui = struct {
    allocator: Allocator,
    layout: *gtk.Fixed,
    dialogs: *gtk.Fixed,
    slots: std.AutoHashMap(u64, Slot),
    chrome: std.ArrayList(*gtk.Label),
    chrome_signature: []u8,
    css: *gtk.CssProvider,
    css_signature: []u8,
    css_installed: bool,
    dialogs_open: bool,
    attached: bool,
    origin_x: f64,
    origin_y: f64,

    pub fn new(allocator: Allocator) !*PaneGui {
        const self = try allocator.create(PaneGui);
        errdefer allocator.destroy(self);
        self.* = .{
            .allocator = allocator,
            .layout = gtk.Fixed.new(),
            .dialogs = gtk.Fixed.new(),
            .slots = .init(allocator),
            .chrome = .empty,
            .chrome_signature = try allocator.dupe(u8, ""),
            .css = gtk.CssProvider.new(),
            .css_signature = try allocator.dupe(u8, ""),
            .css_installed = false,
            .dialogs_open = false,
            .attached = false,
            .origin_x = 0,
            .origin_y = 0,
        };
        inert(self.layout.as(gtk.Widget));
        inert(self.dialogs.as(gtk.Widget));
        return self;
    }

    pub fn deinit(self: *PaneGui) void {
        self.detach();
        var it = self.slots.iterator();
        while (it.next()) |e| {
            self.allocator.free(e.value_ptr.signature);
            if (e.value_ptr.ring) |r| self.allocator.free(r);
        }
        self.slots.deinit();
        self.chrome.deinit(self.allocator);
        self.allocator.free(self.chrome_signature);
        self.allocator.free(self.css_signature);
        self.css.unref();
        self.layout.as(gtk.Widget).unref();
        self.dialogs.as(gtk.Widget).unref();
        self.allocator.destroy(self);
    }

    pub fn attach(self: *PaneGui, parent: *gtk.Widget) void {
        if (self.attached) {
            if (self.layout.as(gtk.Widget).getParent() == parent) return;
            self.detach();
        }
        self.layout.as(gtk.Widget).setParent(parent);
        self.dialogs.as(gtk.Widget).setParent(parent);
        self.attached = true;
        self.installCss();
    }

    pub fn detach(self: *PaneGui) void {
        if (!self.attached) return;
        self.layout.as(gtk.Widget).unparent();
        self.dialogs.as(gtk.Widget).unparent();
        self.attached = false;
    }

    pub fn place(self: *PaneGui, origin_x: f64, origin_y: f64, cols: u16, rows: u16, cell_w: f64, cell_h: f64) void {
        self.origin_x = origin_x;
        self.origin_y = origin_y;
        if (!self.attached) return;
        const width: c_int = @intFromFloat(@max(@round(@as(f64, @floatFromInt(cols)) * cell_w), 1.0));
        const height: c_int = @intFromFloat(@max(@round(@as(f64, @floatFromInt(rows)) * cell_h), 1.0));
        // Cards stay in cell space. The grid snapshot translates to the letterbox
        // origin, so this allocation is the cell grid at (0, 0) of that space.
        self.layout.as(gtk.Widget).setSizeRequest(width, height);
        self.dialogs.as(gtk.Widget).setSizeRequest(width, height);
        self.layout.as(gtk.Widget).allocate(width, height, -1, null);
        self.dialogs.as(gtk.Widget).allocate(width, height, -1, null);
    }

    pub fn layoutWidget(self: *PaneGui) *gtk.Widget {
        return self.layout.as(gtk.Widget);
    }

    pub fn dialogsWidget(self: *PaneGui) *gtk.Widget {
        return self.dialogs.as(gtk.Widget);
    }

    pub fn dialogsOpen(self: *const PaneGui) bool {
        return self.dialogs_open;
    }

    pub fn sync(self: *PaneGui, layout_tree: *const tree.LayoutTree, cell_w: f64, cell_h: f64) !void {

        self.installCss();
        var faces_buf: [64]cards.PaneFace = undefined;
        const layout_faces = collectFaces(layout_tree.layout, &faces_buf);
        var dialog_buf: [64]cards.PaneFace = undefined;
        const dialog_faces = collectFaces(layout_tree.overlays, &dialog_buf);
        const dialog_open = dialog_faces.len != 0;
        self.dialogs_open = dialog_open;
        const focused: ?u64 = if (dialog_open)
            dialog_faces[dialog_faces.len - 1].id
        else
            cards.pick_focused(layout_faces);
        var ring: ?cards.Rgba = null;
        if (focused) |id| {
            for (layout_faces) |face| {
                if (face.id == id) {
                    ring = overlay_paint.toCardsRgba(face.border);
                    break;
                }
            }
            if (ring == null) {
                for (dialog_faces) |face| {
                    if (face.id == id) {
                        ring = overlay_paint.toCardsRgba(face.border);
                        break;
                    }
                }
            }
        }
        try self.applyCss(ring);

        var seen: std.AutoHashMap(u64, void) = .init(self.allocator);
        defer seen.deinit();

        const layout_slots = try paneSlots(self.allocator, layout_tree.layout, cell_w, cell_h, true);
        defer self.allocator.free(layout_slots.ids);
        defer self.allocator.free(layout_slots.slots);
        const dialog_slots = try paneSlots(self.allocator, layout_tree.overlays, cell_w, cell_h, false);
        defer self.allocator.free(dialog_slots.ids);
        defer self.allocator.free(dialog_slots.slots);

        try self.syncNodes(layout_tree.layout, cell_h, layout_slots.ids, layout_slots.slots, .layout, focused, dialog_open, ring, &seen);
        try self.syncNodes(layout_tree.overlays, cell_h, dialog_slots.ids, dialog_slots.slots, .dialog, focused, dialog_open, ring, &seen);
        try self.dropUnseen(&seen);

        const grid_w: c_int = @intFromFloat(@max(@round(@as(f64, @floatFromInt(layout_tree.cols)) * cell_w), 1.0));
        try self.syncChrome(layout_tree, cell_h, grid_w);
    }

    fn installCss(self: *PaneGui) void {
        if (self.css_installed or !self.attached) return;
        gtk.StyleContext.addProviderForDisplay(
            self.layout.as(gtk.Widget).getDisplay(),
            self.css.as(gtk.StyleProvider),
            gtk.STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
        self.css_installed = true;
    }

    fn applyCss(self: *PaneGui, ring: ?cards.Rgba) !void {
        const css = try cards.card_stylesheet(ring, self.allocator);
        defer self.allocator.free(css);
        if (std.mem.eql(u8, css, self.css_signature)) return;
        const z = try self.allocator.dupeZ(u8, css);
        defer self.allocator.free(z);
        self.css.loadFromString(z);
        self.allocator.free(self.css_signature);
        self.css_signature = try self.allocator.dupe(u8, css);
    }

    fn syncNodes(
        self: *PaneGui,
        nodes: []const tree.Node,
        cell_h: f64,
        ids: []const u64,
        slots: []const cards.CardSlot,
        layer: Layer,
        focused: ?u64,
        dialog_open: bool,
        ring: ?cards.Rgba,
        seen: *std.AutoHashMap(u64, void),
    ) !void {
        for (nodes) |*node| {
            if (isCard(node)) {
                try self.syncNode(node, cell_h, ids, slots, layer, focused, dialog_open, ring, seen);
            }
            try self.syncNodes(node.children, cell_h, ids, slots, layer, focused, dialog_open, ring, seen);
        }
    }

    fn syncNode(
        self: *PaneGui,
        node: *const tree.Node,
        cell_h: f64,
        ids: []const u64,
        slots: []const cards.CardSlot,
        layer: Layer,
        focused: ?u64,
        dialog_open: bool,
        ring: ?cards.Rgba,
        seen: *std.AutoHashMap(u64, void),
    ) !void {
        const id = node.id;
        const card_slot = blk: {
            for (ids, slots) |sid, slot| {
                if (sid == id) break :blk slot;
            }
            return;
        };
        try seen.put(id, {});
        const row_h: c_int = @intFromFloat(@max(@round(cell_h), 1.0));
        const signature = try cardSignature(self.allocator, node, row_h);
        const board = if (layer == .layout) self.layout else self.dialogs;

        if (self.slots.getPtr(id)) |slot| {
            if (slot.layer != layer) {
                const old = if (slot.layer == .layout) self.layout else self.dialogs;
                old.remove(slot.card.as(gtk.Widget));
            }
            if (!std.mem.eql(u8, slot.signature, signature)) {
                refillCard(slot, node, row_h);
                self.allocator.free(slot.signature);
                slot.signature = signature;
            } else {
                self.allocator.free(signature);
            }
            slot.layer = layer;
            placeCard(board, slot, card_slot, 0, 0);
            paintRole(slot, layer, id, focused, dialog_open, ring, self.allocator);
            return;
        }

        var slot = try newSlot(self.allocator, node, row_h, layer, signature);
        paintRole(&slot, layer, id, focused, dialog_open, ring, self.allocator);
        placeCard(board, &slot, card_slot, 0, 0);
        try self.slots.put(id, slot);
    }

    fn dropUnseen(self: *PaneGui, seen: *std.AutoHashMap(u64, void)) !void {
        var stale: std.ArrayList(u64) = .empty;
        defer stale.deinit(self.allocator);
        var it = self.slots.keyIterator();
        while (it.next()) |key| {
            if (!seen.contains(key.*)) try stale.append(self.allocator, key.*);
        }
        for (stale.items) |id| {
            if (self.slots.fetchRemove(id)) |kv| {
                var slot = kv.value;
                const board = if (slot.layer == .layout) self.layout else self.dialogs;
                board.remove(slot.card.as(gtk.Widget));
                self.allocator.free(slot.signature);
                if (slot.ring) |r| self.allocator.free(r);
            }
        }
    }

    fn syncChrome(self: *PaneGui, layout_tree: *const tree.LayoutTree, cell_h: f64, grid_w: c_int) !void {
        var sig: std.ArrayList(u8) = .empty;
        defer sig.deinit(self.allocator);
        for (layout_tree.chrome) |row| {
            var ybuf: [32]u8 = undefined;
            const ypart = std.fmt.bufPrint(&ybuf, "{d}:", .{row.y}) catch continue;
            try sig.appendSlice(self.allocator, ypart);
            for (row.line.spans) |span| try sig.appendSlice(self.allocator, span.text);
            try sig.append(self.allocator, '\n');
        }
        const height: c_int = @intFromFloat(@max(@round(cell_h), 1.0));
        if (!std.mem.eql(u8, sig.items, self.chrome_signature)) {
            for (self.chrome.items) |label| {
                self.layout.remove(label.as(gtk.Widget));
            }
            self.chrome.clearRetainingCapacity();
            for (layout_tree.chrome) |row| {
                const label = lineLabel(self.allocator, row.line) catch continue;
                inert(label.as(gtk.Widget));
                const y = @as(f64, @floatFromInt(row.y)) * cell_h;
                placeOn(self.layout, label.as(gtk.Widget), 0, y, grid_w, height);
                try self.chrome.append(self.allocator, label);
            }
            self.allocator.free(self.chrome_signature);
            self.chrome_signature = try self.allocator.dupe(u8, sig.items);
            return;
        }
        for (self.chrome.items, layout_tree.chrome) |label, row| {
            const y = @as(f64, @floatFromInt(row.y)) * cell_h;
            placeOn(self.layout, label.as(gtk.Widget), 0, y, grid_w, height);
        }
    }
};

/// Walk tree and stroke frames (POC paint_tree). Native cards are the enhance path; this is the stroke fallback.
pub fn paintTree(snapshot: *gtk.Snapshot, layout_tree: *const tree.LayoutTree, cell_w: f64, cell_h: f64, scale: f64) void {
    var holes_buf: [64]overlay_paint.Bounds = undefined;
    var holes_n: usize = 0;
    collectHoles(layout_tree.overlays, cell_w, cell_h, &holes_buf, &holes_n);
    paintNodeList(snapshot, layout_tree.layout, holes_buf[0..holes_n], cell_w, cell_h, scale);
    paintNodeList(snapshot, layout_tree.overlays, &.{}, cell_w, cell_h, scale);
}

fn paintNodeList(snapshot: *gtk.Snapshot, nodes: []const tree.Node, holes: []const overlay_paint.Bounds, cell_w: f64, cell_h: f64, scale: f64) void {
    for (nodes) |*node| {
        overlay_paint.paintFrame(snapshot, node.x, node.y, node.cols, node.rows, node.border, node.scroll, holes, cell_w, cell_h, scale);
        paintNodeList(snapshot, node.children, holes, cell_w, cell_h, scale);
    }
}

fn collectHoles(nodes: []const tree.Node, cell_w: f64, cell_h: f64, buf: *[64]overlay_paint.Bounds, n: *usize) void {
    for (nodes) |*node| {
        if (n.* < buf.len) {
            buf[n.*] = overlay_paint.overlayHole(node.x, node.y, node.cols, node.rows, cell_w, cell_h);
            n.* += 1;
        }
        collectHoles(node.children, cell_w, cell_h, buf, n);
    }
}

fn inert(w: *gtk.Widget) void {
    w.setCanTarget(0);
    w.setFocusable(0);
}

fn isCard(node: *const tree.Node) bool {
    return node.children.len == 0 or node.body != .empty;
}

fn collectFaces(nodes: []const tree.Node, buf: []cards.PaneFace) []cards.PaneFace {
    var n: usize = 0;
    collectFacesRec(nodes, buf, &n);
    return buf[0..n];
}

fn collectFacesRec(nodes: []const tree.Node, buf: []cards.PaneFace, n: *usize) void {
    for (nodes) |*node| {
        if (isCard(node) and n.* < buf.len) {
            buf[n.*] = .{ .id = node.id, .border = node.border };
            n.* += 1;
        }
        collectFacesRec(node.children, buf, n);
    }
}

const SlotList = struct { ids: []u64, slots: []cards.CardSlot };

fn paneSlots(allocator: Allocator, nodes: []const tree.Node, cell_w: f64, cell_h: f64, tile: bool) !SlotList {
    var frames: std.ArrayList(cards.FrameRect) = .empty;
    defer frames.deinit(allocator);
    var ids: std.ArrayList(u64) = .empty;
    errdefer ids.deinit(allocator);
    try collectFrames(nodes, &frames, &ids, allocator);
    if (tile) {
        const resolved = try cards.resolve_slots(frames.items, cell_w, cell_h, allocator);
        return .{ .ids = try ids.toOwnedSlice(allocator), .slots = resolved };
    }
    var out: std.ArrayList(cards.CardSlot) = .empty;
    errdefer out.deinit(allocator);
    for (frames.items) |frame| {
        const t = cards.inset_card(
            @as(f64, @floatFromInt(frame.x)) * cell_w,
            @as(f64, @floatFromInt(frame.y)) * cell_h,
            @as(f64, @floatFromInt(frame.cols)) * cell_w,
            @as(f64, @floatFromInt(frame.rows)) * cell_h,
        );
        try out.append(allocator, .{ .x = t[0], .y = t[1], .width = t[2], .height = t[3] });
    }
    return .{ .ids = try ids.toOwnedSlice(allocator), .slots = try out.toOwnedSlice(allocator) };
}

fn collectFrames(nodes: []const tree.Node, frames: *std.ArrayList(cards.FrameRect), ids: *std.ArrayList(u64), allocator: Allocator) !void {
    for (nodes) |*node| {
        if (isCard(node)) {
            try ids.append(allocator, node.id);
            try frames.append(allocator, .{ .x = node.x, .y = node.y, .cols = node.cols, .rows = node.rows });
        }
        try collectFrames(node.children, frames, ids, allocator);
    }
}

fn placeOn(board: *gtk.Fixed, child: *gtk.Widget, x: f64, y: f64, w: c_int, h: c_int) void {
    if (child.getParent() == board.as(gtk.Widget)) {
        board.move(child, x, y);
    } else {
        if (child.getParent() != null) child.unparent();
        board.put(child, x, y);
    }
    child.setSizeRequest(w, h);
}

fn placeCard(board: *gtk.Fixed, slot: *Slot, card_slot: cards.CardSlot, origin_x: f64, origin_y: f64) void {
    slot.geom = card_slot;
    placeOn(board, slot.card.as(gtk.Widget), origin_x + card_slot.x, origin_y + card_slot.y, card_slot.width, card_slot.height);
}

fn paintRole(slot: *Slot, layer: Layer, id: u64, focused: ?u64, dialog_open: bool, ring: ?cards.Rgba, allocator: Allocator) void {
    const card = slot.card.as(gtk.Widget);
    if (layer == .dialog) card.addCssClass("herdr-dialog") else card.removeCssClass("herdr-dialog");
    const dimmed = switch (layer) {
        .dialog => false,
        .layout => dialog_open or (focused != null and focused.? != id),
    };
    if (dimmed) card.addCssClass("herdr-dimmed") else card.removeCssClass("herdr-dimmed");
    const is_focused = focused == id;
    if (is_focused) card.addCssClass("focused") else card.removeCssClass("focused");
    if (slot.ring) |old| {
        card.removeCssClass(old);
        allocator.free(old);
        slot.ring = null;
    }
    if (is_focused) {
        if (ring) |color| {
            var buf: [32]u8 = undefined;
            const name = cards.ring_class(color, &buf);
            const owned = allocator.dupeZ(u8, name) catch return;
            card.addCssClass(owned);
            slot.ring = owned;
        }
    }
}

fn newSlot(allocator: Allocator, node: *const tree.Node, row_h: c_int, layer: Layer, signature: []u8) !Slot {
    _ = allocator;
    const card = gtk.Box.new(.vertical, 0);
    card.as(gtk.Widget).addCssClass("herdr-card");
    card.as(gtk.Widget).setOverflow(.hidden);
    inert(card.as(gtk.Widget));

    const title = gtk.Label.new(null);
    title.setXalign(0);
    title.as(gtk.Widget).addCssClass("herdr-card-header");
    inert(title.as(gtk.Widget));

    const count = gtk.Label.new(null);
    count.setXalign(1);
    count.as(gtk.Widget).addCssClass("herdr-meta");
    count.as(gtk.Widget).addCssClass("herdr-card-count");
    inert(count.as(gtk.Widget));

    const header = gtk.Box.new(.horizontal, 0);
    header.append(title.as(gtk.Widget));
    header.append(count.as(gtk.Widget));
    inert(header.as(gtk.Widget));

    const body = gtk.Box.new(.vertical, 0);
    body.as(gtk.Widget).addCssClass("herdr-pane-body");
    inert(body.as(gtk.Widget));

    const fade_box = gtk.Box.new(.vertical, 0);
    fade_box.as(gtk.Widget).addCssClass("herdr-fade");
    inert(fade_box.as(gtk.Widget));

    const pane = gtk.Overlay.new();
    pane.setChild(body.as(gtk.Widget));
    pane.addOverlay(fade_box.as(gtk.Widget));
    inert(pane.as(gtk.Widget));

    card.append(header.as(gtk.Widget));
    card.append(pane.as(gtk.Widget));

    var slot: Slot = .{
        .card = card,
        .header = header,
        .title = title,
        .count = count,
        .body = body,
        .fade = fade_box.as(gtk.Widget),
        .geom = .{ .x = 0, .y = 0, .width = 0, .height = 0 },
        .signature = signature,
        .layer = layer,
        .ring = null,
    };
    refillCard(&slot, node, row_h);
    return slot;
}

fn refillCard(slot: *Slot, node: *const tree.Node, row_h: c_int) void {
    while (slot.body.as(gtk.Widget).getFirstChild()) |child| {
        slot.body.remove(child);
    }
    switch (node.body) {
        .empty => {},
        .rows => |lines| {
            const list = gtk.ListBox.new();
            list.setSelectionMode(.none);
            list.as(gtk.Widget).addCssClass("herdr-rows");
            inert(list.as(gtk.Widget));
            for (lines) |line| {
                const label = lineLabelPlain(line);
                label.as(gtk.Widget).setValign(.center);
                const row = gtk.ListBoxRow.new();
                row.as(gtk.Widget).addCssClass("herdr-row");
                row.as(gtk.Widget).setSizeRequest(-1, row_h);
                if (line.selected) row.as(gtk.Widget).addCssClass("herdr-selected");
                row.setChild(label.as(gtk.Widget));
                inert(row.as(gtk.Widget));
                list.append(row.as(gtk.Widget));
            }
            slot.body.append(list.as(gtk.Widget));
        },
        .text => |lines| {
            const view = gtk.TextView.new();
            view.setEditable(0);
            view.setCursorVisible(0);
            view.setMonospace(1);
            view.setWrapMode(.none);
            inert(view.as(gtk.Widget));
            var text: std.ArrayList(u8) = .empty;
            defer text.deinit(std.heap.page_allocator);
            for (lines, 0..) |line, i| {
                if (i > 0) text.append(std.heap.page_allocator, '\n') catch {};
                for (line.spans) |span| text.appendSlice(std.heap.page_allocator, span.text) catch {};
            }
            const z = std.heap.page_allocator.dupeZ(u8, text.items) catch return;
            defer std.heap.page_allocator.free(z);
            view.getBuffer().setText(z, -1);
            slot.body.append(view.as(gtk.Widget));
        },
    }
    applyOverflow(slot, node, row_h);
}

fn applyOverflow(slot: *Slot, node: *const tree.Node, row_h: c_int) void {
    if (node.title) |title| {
        const z = std.heap.page_allocator.dupeZ(u8, title) catch {
            slot.title.as(gtk.Widget).setVisible(0);
            return;
        };
        defer std.heap.page_allocator.free(z);
        slot.title.setText(z);
        slot.title.as(gtk.Widget).setVisible(1);
    } else {
        slot.title.as(gtk.Widget).setVisible(0);
    }
    if (node.scroll) |pair| {
        var buf: [64]u8 = undefined;
        const text = std.fmt.bufPrintZ(&buf, "{d} / {d}", .{ pair[0], pair[1] }) catch {
            slot.count.as(gtk.Widget).setVisible(0);
            return;
        };
        slot.count.setText(text);
        slot.count.as(gtk.Widget).setVisible(1);
    } else {
        slot.count.as(gtk.Widget).setVisible(0);
    }
    slot.header.as(gtk.Widget).setVisible(@intFromBool(slot.title.as(gtk.Widget).getVisible() != 0 or slot.count.as(gtk.Widget).getVisible() != 0));
    const more = node.thumb != null or (node.scroll != null and node.scroll.?[1] > viewportRows(node));
    if (more) slot.card.as(gtk.Widget).addCssClass("herdr-more") else slot.card.as(gtk.Widget).removeCssClass("herdr-more");
    const fade_h = @max(@divTrunc(row_h * 2, 5) + 16, 20);
    slot.fade.setSizeRequest(-1, fade_h);
    slot.fade.setVisible(@intFromBool(more));
}

fn viewportRows(node: *const tree.Node) u32 {
    return @max(@as(u32, node.rows) -| 2, 1);
}

fn cardSignature(allocator: Allocator, node: *const tree.Node, row_h: c_int) ![]u8 {
    const body = switch (node.body) {
        .empty => "empty",
        .rows => "rows",
        .text => "text",
    };
    const header: []const u8 = if (node.title != null and node.scroll != null)
        "title-count"
    else if (node.title != null)
        "title"
    else if (node.scroll != null)
        "count"
    else
        "none";
    return std.fmt.allocPrint(allocator, "{s}|{s}|{d}|{d}", .{ header, body, row_h, node.id });
}

fn lineLabelPlain(line: tree.ContentLine) *gtk.Label {
    var buf: [512]u8 = undefined;
    var n: usize = 0;
    for (line.spans) |span| {
        const take = @min(span.text.len, buf.len - n);
        @memcpy(buf[n..][0..take], span.text[0..take]);
        n += take;
    }
    buf[n] = 0;
    const label = gtk.Label.new(buf[0..n :0]);
    inert(label.as(gtk.Widget));
    return label;
}

fn lineLabel(allocator: Allocator, line: tree.ContentLine) !*gtk.Label {
    var text: std.ArrayList(u8) = .empty;
    defer text.deinit(allocator);
    for (line.spans) |span| try text.appendSlice(allocator, span.text);
    const z = try allocator.dupeZ(u8, text.items);
    defer allocator.free(z);
    const label = gtk.Label.new(z);
    inert(label.as(gtk.Widget));
    return label;
}

comptime {
    _ = PaneGui.new;
    _ = PaneGui.deinit;
    _ = PaneGui.attach;
    _ = PaneGui.detach;
    _ = PaneGui.place;
    _ = PaneGui.layoutWidget;
    _ = PaneGui.dialogsWidget;
    _ = PaneGui.dialogsOpen;
    _ = PaneGui.sync;
    _ = paintTree;
}
