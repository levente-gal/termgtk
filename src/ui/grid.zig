//! One GTK widget that paints a contract.Frame via paint.zig + GSK.
const std = @import("std");
const contract = @import("contract");
const gtk = @import("gtk");
const paint = @import("paint");
const cards = @import("cards");

const DEFAULT_CELL_W: f64 = 8.0;
const DEFAULT_CELL_H: f64 = 16.0;
const DEFAULT_SCALE: f64 = 1.0;
const FONT_SIZE_PT: c_int = 12;
const PANGO_SCALE: c_int = 1024;

const GdkRgba = @typeInfo(@typeInfo(@TypeOf(gtk.Snapshot.appendColor)).@"fn".params[1].type.?).pointer.child;
const GrapheneRect = @typeInfo(@typeInfo(@TypeOf(gtk.Snapshot.appendColor)).@"fn".params[2].type.?).pointer.child;
const GraphenePoint = @typeInfo(@typeInfo(@TypeOf(gtk.Snapshot.translate)).@"fn".params[1].type.?).pointer.child;

extern fn g_type_register_static_simple(
    parent_type: usize,
    type_name: [*:0]const u8,
    class_size: c_uint,
    class_init: ?*const fn (?*anyopaque, ?*anyopaque) callconv(.c) void,
    instance_size: c_uint,
    instance_init: ?*const fn (*anyopaque, ?*anyopaque) callconv(.c) void,
    flags: u32,
) usize;
extern fn g_object_new(object_type: usize, first_property_name: ?[*:0]const u8, ...) *anyopaque;
extern fn g_object_ref_sink(object: *anyopaque) *anyopaque;
extern fn g_once_init_enter(location: *usize) c_int;
extern fn g_once_init_leave(location: *usize, result: usize) void;

extern fn pango_font_description_new() *anyopaque;
extern fn pango_font_description_set_family(desc: *anyopaque, family: [*:0]const u8) void;
extern fn pango_font_description_set_size(desc: *anyopaque, size: c_int) void;
extern fn pango_font_description_set_weight(desc: *anyopaque, weight: c_int) void;
extern fn pango_font_description_set_style(desc: *anyopaque, style: c_int) void;
extern fn pango_font_description_copy(desc: ?*const anyopaque) ?*anyopaque;
extern fn pango_font_description_free(desc: ?*anyopaque) void;

pub const SuppressCell = struct { x: u16, y: u16 };

pub const Grid = extern struct {
    parent: gtk.Widget,
    frame: ?*const contract.Frame = null,
    cell_w: f64 = DEFAULT_CELL_W,
    cell_h: f64 = DEFAULT_CELL_H,
    scale: f64 = DEFAULT_SCALE,
    font_desc: ?*anyopaque = null,
    metrics_ready: c_int = 0,
    alloc_w: c_int = 0,
    alloc_h: c_int = 0,
    suppress_ptr: [*]const SuppressCell = undefined,
    suppress_len: usize = 0,
    enhance: bool = false,
    overlay_layout: ?*gtk.Widget = null,
    overlay_dialogs: ?*gtk.Widget = null,
    dialogs_open: bool = false,
    scroll_accum: f64 = 0,
    pointer_x: f64 = 0,
    pointer_y: f64 = 0,
    has_pointer: bool = false,
    pane_id_ptr: [*]const u8 = undefined,
    pane_id_len: usize = 0,
    scroll_wired: bool = false,

    pub const Parent = gtk.Widget;

    pub const Class = extern struct {
        parent_class: gtk.Widget.Class,
        pub const Instance = Grid;
    };

    var type_id: usize = 0;

    pub fn getGObjectType() usize {
        if (g_once_init_enter(&type_id) != 0) {
            const id = g_type_register_static_simple(
                gtk.Widget.getGObjectType(),
                "TuiguiGrid",
                @sizeOf(Class),
                &classInit,
                @sizeOf(Grid),
                &instanceInit,
                0,
            );
            g_once_init_leave(&type_id, id);
        }
        return type_id;
    }

    pub fn asWidget(self: *Grid) *gtk.Widget {
        return &self.parent;
    }

    pub fn new() *Grid {
        const self: *Grid = @ptrCast(@alignCast(g_object_new(getGObjectType(), null)));
        // GtkWidget is floating. Own one ref so unparenting the split tree
        // does not finalize the grid while the shell still paints it.
        _ = g_object_ref_sink(self);
        return self;
    }

    pub fn setFrame(self: *Grid, frame: ?*const contract.Frame) void {
        self.frame = frame;
        self.asWidget().queueDraw();
    }

    pub fn setPaneId(self: *Grid, pane_id: []const u8) void {
        self.pane_id_ptr = pane_id.ptr;
        self.pane_id_len = pane_id.len;
    }

    pub fn paneId(self: *const Grid) []const u8 {
        if (self.pane_id_len == 0) return &.{};
        return self.pane_id_ptr[0..self.pane_id_len];
    }

    pub fn clearEnhance(self: *Grid) void {
        self.enhance = false;
        self.suppress_len = 0;
        self.overlay_layout = null;
        self.overlay_dialogs = null;
        self.dialogs_open = false;
    }

    pub fn setEnhanceMask(self: *Grid, mask: []const SuppressCell) void {
        self.enhance = true;
        self.suppress_len = mask.len;
        if (mask.len != 0) self.suppress_ptr = mask.ptr;
    }

    fn suppressSlice(self: *const Grid) []const SuppressCell {
        if (self.suppress_len == 0) return &.{};
        return self.suppress_ptr[0..self.suppress_len];
    }

    pub fn setOverlay(self: *Grid, layout: *gtk.Widget, dialogs: *gtk.Widget, dialogs_open: bool) void {
        self.overlay_layout = layout;
        self.overlay_dialogs = dialogs;
        self.dialogs_open = dialogs_open;
    }

    pub fn cellAtPointer(self: *Grid) ?struct { u16, u16 } {
        if (!self.has_pointer) return null;
        const frame = self.frame orelse return null;
        const widget = self.asWidget();
        const width: f64 = @floatFromInt(widget.getWidth());
        const height: f64 = @floatFromInt(widget.getHeight());
        const cell_w = self.cellWidth();
        const cell_h = self.cellHeight();
        const cap = paint.deviceColsRows(width, height, cell_w, cell_h, self.scale);
        return paint.cellUnderPointer(self.pointer_x, self.pointer_y, width, height, cell_w, cell_h, frame.cols, frame.rows, cap[0], cap[1]);
    }

    pub fn getFrame(self: *const Grid) ?*const contract.Frame {
        return self.frame;
    }

    pub fn cellWidth(self: *Grid) f64 {
        ensureFontMetrics(self);
        return paint.snapCellPx(self.cell_w, self.scale);
    }

    pub fn cellHeight(self: *Grid) f64 {
        ensureFontMetrics(self);
        return paint.snapCellPx(self.cell_h, self.scale);
    }

    pub fn getScale(self: *const Grid) f64 {
        return self.scale;
    }

    pub fn setScale(self: *Grid, scale: f64) void {
        if (std.math.isFinite(scale) and scale > 0.0) self.scale = scale;
    }

    pub fn colsRows(self: *Grid, width_px: f64, height_px: f64) struct { u16, u16 } {
        return paint.deviceColsRows(width_px, height_px, self.cellWidth(), self.cellHeight(), self.scale);
    }

    fn classInit(class: ?*anyopaque, _: ?*anyopaque) callconv(.c) void {
        const widget_class: *gtk.Widget.Class = @ptrCast(@alignCast(class.?));
        gtk.Widget.virtual_methods.measure.implement(widget_class, &measure);
        gtk.Widget.virtual_methods.size_allocate.implement(widget_class, &sizeAllocate);
        gtk.Widget.virtual_methods.snapshot.implement(widget_class, &snapshot);
        widget_class.setCssName("terminal-view");
    }

    fn instanceInit(instance: *anyopaque, _: ?*anyopaque) callconv(.c) void {
        const self: *Grid = @ptrCast(@alignCast(instance));
        self.frame = null;
        self.cell_w = DEFAULT_CELL_W;
        self.cell_h = DEFAULT_CELL_H;
        self.scale = DEFAULT_SCALE;
        self.font_desc = null;
        self.metrics_ready = 0;
        self.alloc_w = 0;
        self.alloc_h = 0;
        self.suppress_len = 0;
        self.enhance = false;
        self.overlay_layout = null;
        self.overlay_dialogs = null;
        self.dialogs_open = false;
        self.scroll_accum = 0;
        self.pointer_x = 0;
        self.pointer_y = 0;
        self.has_pointer = false;
        self.pane_id_len = 0;
        self.scroll_wired = false;
        const w = self.asWidget();
        w.setFocusable(1);
        w.setCanFocus(1);
        w.setFocusOnClick(1);
        w.setOverflow(.hidden);
    }

    fn measure(widget: *gtk.Widget, orientation: gtk.Orientation, _: c_int, minimum: ?*c_int, natural: ?*c_int, minimum_baseline: ?*c_int, natural_baseline: ?*c_int) callconv(.c) void {
        const self: *Grid = @ptrCast(@alignCast(widget));
        ensureFontMetrics(self);
        const cell = if (orientation == .horizontal) paint.snapCellPx(self.cell_w, self.scale) else paint.snapCellPx(self.cell_h, self.scale);
        const size: c_int = @intFromFloat(@max(@ceil(cell), 1.0));
        if (minimum) |m| m.* = size;
        if (natural) |n| n.* = size;
        if (minimum_baseline) |b| b.* = -1;
        if (natural_baseline) |b| b.* = -1;
    }

    fn sizeAllocate(widget: *gtk.Widget, width: c_int, height: c_int, _: c_int) callconv(.c) void {
        const self: *Grid = @ptrCast(@alignCast(widget));
        const scale: f64 = @floatFromInt(widget.getScaleFactor());
        if (std.math.isFinite(scale) and scale > 0.0) self.scale = scale;
        self.alloc_w = width;
        self.alloc_h = height;
        ensureFontMetrics(self);
    }

    fn snapshot(widget: *gtk.Widget, snap: *gtk.Snapshot) callconv(.c) void {
        const self: *Grid = @ptrCast(@alignCast(widget));
        ensureFontMetrics(self);
        const width: f32 = @floatFromInt(widget.getWidth());
        const height: f32 = @floatFromInt(widget.getHeight());
        if (width <= 0.0 or height <= 0.0) return;

        var bounds: GrapheneRect = .{
            .f_origin = .{ .f_x = 0, .f_y = 0 },
            .f_size = .{ .f_width = width, .f_height = height },
        };
        snap.pushClip(&bounds);

        const terminal = self.frame orelse {
            snap.pop();
            return;
        };

        const scale = self.scale;
        const cell_w = paint.snapCellPx(self.cell_w, scale);
        const cell_h = paint.snapCellPx(self.cell_h, scale);
        const cap = paint.deviceColsRows(@floatFromInt(widget.getWidth()), @floatFromInt(widget.getHeight()), cell_w, cell_h, scale);
        const origin = paint.letterboxOrigin(@floatFromInt(widget.getWidth()), @floatFromInt(widget.getHeight()), cell_w, cell_h, terminal.cols, terminal.rows, cap[0], cap[1]) orelse {
            snap.pop();
            return;
        };

        if (self.enhance) {
            const canvas, _ = cards.enhanced_chrome();
            appendSnapped(snap, .{ .r = canvas.r, .g = canvas.g, .b = canvas.b, .a = canvas.a }, 0, 0, @floatCast(width), @floatCast(height), scale);
        } else if (paint.dominantOpaqueBg(terminal.cells)) |bg| {
            appendSnapped(snap, bg, 0, 0, @floatCast(width), @floatCast(height), scale);
        }

        var bits: [8192]u8 = undefined;
        const ncells: usize = @as(usize, terminal.cols) * @as(usize, terminal.rows);
        const use_bits = self.enhance and ncells > 0 and ncells <= bits.len;
        const bitset: []const u8 = if (use_bits) bits[0..ncells] else bits[0..0];
        const mask = self.suppressSlice();
        if (use_bits) {
            @memset(bits[0..ncells], 0);
            for (mask) |p| {
                if (p.x < terminal.cols and p.y < terminal.rows) bits[@as(usize, p.y) * terminal.cols + p.x] = 1;
            }
        }
        const sup = struct {
            fn at(enhance: bool, bits_on: bool, bits_slice: []const u8, cols: u16, cells: []const SuppressCell, x: u16, y: u16) bool {
                if (!enhance) return false;
                if (bits_on) {
                    if (x >= cols) return false;
                    const i = @as(usize, y) * cols + x;
                    if (i >= bits_slice.len) return false;
                    return bits_slice[i] != 0;
                }
                for (cells) |p| if (p.x == x and p.y == y) return true;
                return false;
            }
        }.at;

        var arena_buf: [128 * 1024]u8 = undefined;
        var fba = std.heap.FixedBufferAllocator.init(&arena_buf);
        const allocator = fba.allocator();

        snap.save();
        var origin_pt: GraphenePoint = .{ .f_x = @floatCast(origin[0]), .f_y = @floatCast(origin[1]) };
        snap.translate(&origin_pt);

        if (paint.groupBackgroundRuns(allocator, terminal.cells)) |bg_runs| {
            defer allocator.free(bg_runs);
            for (bg_runs) |run| {
                var start: usize = 0;
                while (start < run.cells.len) {
                    while (start < run.cells.len and sup(self.enhance, use_bits, bitset, terminal.cols, mask, run.cells[start].x, run.cells[start].y)) start += 1;
                    var end = start;
                    while (end < run.cells.len and !sup(self.enhance, use_bits, bitset, terminal.cols, mask, run.cells[end].x, run.cells[end].y)) end += 1;
                    if (start < end) {
                        const first = run.cells[start];
                        const colors = paint.paintColors(first.fg, first.bg, first.style);
                        if (colors[1].isVisible()) {
                            const band = paint.cellBand(first.x, first.y, @intCast(end - start), cell_w, cell_h);
                            appendSnapped(snap, colors[1], band[0], band[1], band[2], band[3], scale);
                        }
                    }
                    start = end;
                }
            }
        } else |_| {}

        if (paint.groupRuns(allocator, terminal.cells)) |text_runs| {
            defer allocator.free(text_runs);
            for (text_runs) |run| {
                if (run.cells.len == 0) continue;
                const first = run.cells[0];
                const fg = paint.paintColors(first.fg, first.bg, first.style)[0];
                var underline_cols: f64 = 0;
                var underline_from: ?u16 = null;
                for (run.cells) |cell| {
                    if (cell.occupancy == .wide_continuation) continue;
                    if (sup(self.enhance, use_bits, bitset, terminal.cols, mask, cell.x, cell.y)) continue;
                    if (cell.grapheme.len == 0) continue;
                    const ox = @as(f64, @floatFromInt(cell.x)) * cell_w;
                    const oy = @as(f64, @floatFromInt(cell.y)) * cell_h;
                    var strokes_buf: [8]paint.BoxStroke = undefined;
                    if (paint.boxCellStrokes(cell.grapheme, cell_w, cell_h, scale, &strokes_buf)) |n| {
                        const stroke_fg = paint.paintColors(cell.fg, cell.bg, cell.style)[0];
                        for (strokes_buf[0..n]) |stroke| {
                            appendSnapped(snap, stroke_fg, ox + stroke.x, oy + stroke.y, stroke.width, stroke.height, scale);
                        }
                    } else {
                        paintGlyph(widget, snap, self.font_desc, first.style, cell.grapheme, fg, ox, oy);
                    }
                    if (first.style.underline) {
                        if (underline_from == null) underline_from = cell.x;
                        underline_cols += switch (cell.occupancy) {
                            .wide_leading => 2.0,
                            .wide_continuation => 0.0,
                            .single => 1.0,
                        };
                    }
                }
                if (first.style.underline) {
                    if (underline_from) |ux| {
                        const hairline = 1.0 / @max(scale, 1.0);
                        appendSnapped(snap, fg, @as(f64, @floatFromInt(ux)) * cell_w, (@as(f64, @floatFromInt(first.y)) + 1.0) * cell_h - hairline, cell_w * @max(underline_cols, 1.0), hairline, scale);
                    }
                }
            }
        } else |_| {}

        if (paint.cursorPaintCell(terminal.cursor)) |pos| {
            if (!sup(self.enhance, use_bits, bitset, terminal.cols, mask, pos.x, pos.y)) {
                var cursor_color: paint.Rgba = paint.Rgba.opaqueRgb(paint.DEFAULT_FG);
                cursor_color.a = @intFromFloat(0.35 * 255.0);
                appendSnapped(snap, cursor_color, @as(f64, @floatFromInt(pos.x)) * cell_w, @as(f64, @floatFromInt(pos.y)) * cell_h, cell_w, cell_h, scale);
            }
        }

        if (self.overlay_layout) |child| widget.snapshotChild(child, snap);
        snap.restore();
        if (self.dialogs_open) {
            const scrim: paint.Rgba = .{ .r = 0, .g = 0, .b = 0, .a = @intFromFloat(@as(f64, cards.SCRIM_ALPHA) * 255.0) };
            appendSnapped(snap, scrim, 0, 0, @floatCast(width), @floatCast(height), scale);
        }
        if (self.overlay_dialogs) |child| {
            snap.save();
            snap.translate(&origin_pt);
            widget.snapshotChild(child, snap);
            snap.restore();
        }
        snap.pop();
    }
};

fn appendSnapped(snap: *gtk.Snapshot, color: paint.Rgba, x: f64, y: f64, w: f64, h: f64, scale: f64) void {
    const r = paint.snapRect(x, y, w, h, scale);
    const gdk_color: GdkRgba = .{
        .f_red = @as(f32, @floatFromInt(color.r)) / 255.0,
        .f_green = @as(f32, @floatFromInt(color.g)) / 255.0,
        .f_blue = @as(f32, @floatFromInt(color.b)) / 255.0,
        .f_alpha = @as(f32, @floatFromInt(color.a)) / 255.0,
    };
    var bounds: GrapheneRect = .{
        .f_origin = .{ .f_x = @floatCast(r[0]), .f_y = @floatCast(r[1]) },
        .f_size = .{ .f_width = @floatCast(r[2]), .f_height = @floatCast(r[3]) },
    };
    snap.appendColor(&gdk_color, &bounds);
}

fn paintGlyph(widget: *gtk.Widget, snap: *gtk.Snapshot, font_desc: ?*anyopaque, style: contract.Style, grapheme: []const u8, fg: paint.Rgba, ox: f64, oy: f64) void {
    var tmp: [128]u8 = undefined;
    if (grapheme.len >= tmp.len) return;
    @memcpy(tmp[0..grapheme.len], grapheme);
    tmp[grapheme.len] = 0;
    const layout = widget.createPangoLayout(@ptrCast(tmp[0..grapheme.len :0]));
    defer layout.unref();
    if (font_desc) |base| {
        const desc = pango_font_description_copy(base) orelse base;
        defer if (desc != base) pango_font_description_free(desc);
        if (style.bold) pango_font_description_set_weight(desc, 700);
        if (style.italic) pango_font_description_set_style(desc, 2);
        layout.setFontDescription(@ptrCast(desc));
    }
    const gdk_color: GdkRgba = .{
        .f_red = @as(f32, @floatFromInt(fg.r)) / 255.0,
        .f_green = @as(f32, @floatFromInt(fg.g)) / 255.0,
        .f_blue = @as(f32, @floatFromInt(fg.b)) / 255.0,
        .f_alpha = @as(f32, @floatFromInt(fg.a)) / 255.0,
    };
    snap.save();
    var pt: GraphenePoint = .{ .f_x = @floatCast(ox), .f_y = @floatCast(oy) };
    snap.translate(&pt);
    snap.appendLayout(layout, &gdk_color);
    snap.restore();
}

fn ensureFontMetrics(self: *Grid) void {
    if (self.metrics_ready != 0) return;
    const widget = self.asWidget();
    const context = widget.createPangoContext();

    const desc = pango_font_description_new();
    pango_font_description_set_family(desc, "JetBrains Mono");
    pango_font_description_set_size(desc, FONT_SIZE_PT * PANGO_SCALE);
    if (!fontUsable(widget, context, desc)) {
        pango_font_description_set_family(desc, "monospace");
    }
    if (!fontUsable(widget, context, desc)) {
        pango_font_description_free(desc);
        return;
    }

    const width = glyphWidth(widget, context, desc, "M");
    const metrics = context.getMetrics(@ptrCast(desc), null);
    const height = @as(f64, @floatFromInt(metrics.getHeight())) / @as(f64, @floatFromInt(PANGO_SCALE));
    metrics.unref();
    if (!(width > 0.0 and std.math.isFinite(width) and height > 0.0 and std.math.isFinite(height))) {
        pango_font_description_free(desc);
        return;
    }

    self.cell_w = width;
    self.cell_h = height;
    if (self.font_desc) |old| pango_font_description_free(old);
    self.font_desc = desc;
    self.metrics_ready = 1;
    widget.queueResize();
}

fn fontUsable(widget: *gtk.Widget, context: anytype, desc: *anyopaque) bool {
    if (context.loadFont(@ptrCast(desc)) == null) return false;
    const width = glyphWidth(widget, context, desc, "M");
    if (!(std.math.isFinite(width) and width > 0.0)) return false;
    for ([_][:0]const u8{ "i", "0", "─", "│" }) |glyph| {
        if (@abs(glyphWidth(widget, context, desc, glyph) - width) >= 0.01) return false;
    }
    return true;
}

fn glyphWidth(widget: *gtk.Widget, context: anytype, desc: *anyopaque, text: [:0]const u8) f64 {
    _ = context;
    const layout = widget.createPangoLayout(text);
    defer layout.unref();
    layout.setFontDescription(@ptrCast(desc));
    var w: c_int = 0;
    var h: c_int = 0;
    layout.getSize(&w, &h);
    return @as(f64, @floatFromInt(w)) / @as(f64, @floatFromInt(PANGO_SCALE));
}
