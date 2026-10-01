//! Layout tree. Parse closed box frames; reconcile identity across frames.

const std = @import("std");
const contract = @import("contract");

const Allocator = std.mem.Allocator;

const H = "─";
const V = "│";
const MAX_RECTS: usize = 256;
pub const MIN_EDGE_CELLS: u16 = 3;

pub const Thumb = struct { start: u16, end: u16, track_top: u16, track_bottom: u16 };

pub const Span = struct {
    text: []const u8,
    fg: contract.Color,
    bg: contract.Color,
    style: contract.Style,
};

pub const ContentLine = struct { spans: []Span, selected: bool };

pub const PaneBody = union(enum) {
    empty,
    rows: []ContentLine,
    text: []ContentLine,
};

pub const ChromeRow = struct { y: u16, line: ContentLine };

pub const Node = struct {
    id: u64,
    x: u16,
    y: u16,
    cols: u16,
    rows: u16,
    title: ?[]const u8 = null,
    index: ?u16 = null,
    scroll: ?struct { u32, u32 } = null,
    thumb: ?Thumb = null,
    border: contract.Color = .default,
    body: PaneBody = .empty,
    children: []Node = &.{},
};

pub const CellPos = struct { x: u16, y: u16 };

pub const LayoutTree = struct {
    gpa: Allocator,
    arena_ptr: *std.heap.ArenaAllocator,
    cols: u16,
    rows: u16,
    layout: []Node = &.{},
    overlays: []Node = &.{},
    chrome: []ChromeRow = &.{},
    suppress: []CellPos = &.{},
    history_layout: []Node = &.{},
    history_overlays: []Node = &.{},
    next_id: u64 = 1,
    log_signature: []const u8 = "",

    pub fn deinit(self: *LayoutTree) void {
        self.arena_ptr.deinit();
        self.gpa.destroy(self.arena_ptr);
        self.* = undefined;
    }

    fn arenaAlloc(self: *LayoutTree) Allocator {
        return self.arena_ptr.allocator();
    }

    pub fn nodeCount(self: *const LayoutTree) u64 {
        return countNodes(self.layout) + countNodes(self.overlays);
    }

    pub fn toString(self: *const LayoutTree, a: Allocator) Allocator.Error![]u8 {
        var list: std.ArrayList(u8) = .empty;
        errdefer list.deinit(a);
        const header = try std.fmt.allocPrint(a, "Screen {d}×{d}\n├── Layout\n", .{ self.cols, self.rows });
        defer a.free(header);
        try list.appendSlice(a, header);
        try appendNodes(a, &list, self.layout, "│   ", "Panel");
        try list.appendSlice(a, "└── Overlay\n");
        try appendNodes(a, &list, self.overlays, "    ", "Dialog");
        return try list.toOwnedSlice(a);
    }
};

const CornerFamily = enum { square, rounded };
const Rect = struct { x: u16, y: u16, cols: u16, rows: u16 };

fn cornersOf(f: CornerFamily) [4][]const u8 {
    return switch (f) {
        .square => .{ "┌", "┐", "└", "┘" },
        .rounded => .{ "╭", "╮", "╰", "╯" },
    };
}

fn cellAt(frame: contract.Frame, x: u16, y: u16) ?*const contract.Cell {
    if (x >= frame.cols or y >= frame.rows) return null;
    const idx = @as(usize, y) * frame.cols + x;
    if (idx < frame.cells.len and frame.cells[idx].x == x and frame.cells[idx].y == y)
        return &frame.cells[idx];
    for (frame.cells) |*c| if (c.x == x and c.y == y) return c;
    return null;
}

fn glyphAt(frame: contract.Frame, x: u16, y: u16) []const u8 {
    return if (cellAt(frame, x, y)) |c| c.grapheme else "";
}

fn cellKey(x: u16, y: u16) u32 {
    return (@as(u32, x) << 16) | y;
}

fn countNodes(nodes: []const Node) u64 {
    var n: u64 = 0;
    for (nodes) |node| n += 1 + countNodes(node.children);
    return n;
}

fn areaOf(n: Node) u32 {
    return @as(u32, n.cols) * n.rows;
}

fn iou(a: Node, b: Node) f32 {
    const ax1 = @as(u32, a.x) + a.cols;
    const ay1 = @as(u32, a.y) + a.rows;
    const bx1 = @as(u32, b.x) + b.cols;
    const by1 = @as(u32, b.y) + b.rows;
    const ix0 = @max(@as(u32, a.x), @as(u32, b.x));
    const iy0 = @max(@as(u32, a.y), @as(u32, b.y));
    const ix1 = @min(ax1, bx1);
    const iy1 = @min(ay1, by1);
    if (ix1 <= ix0 or iy1 <= iy0) return 0;
    const inter: f32 = @floatFromInt((ix1 - ix0) * (iy1 - iy0));
    const uni = @as(f32, @floatFromInt(areaOf(a) + areaOf(b))) - inter;
    return if (uni <= 0) 0 else inter / uni;
}

fn strictlyContains(outer: Node, inner: Node) bool {
    return inner.x > outer.x and inner.y > outer.y and
        inner.x + inner.cols < outer.x + outer.cols and
        inner.y + inner.rows < outer.y + outer.rows;
}

fn rectStrictContains(outer: Rect, inner: Rect) bool {
    return inner.x > outer.x and inner.y > outer.y and
        inner.x +| inner.cols < outer.x +| outer.cols and
        inner.y +| inner.rows < outer.y +| outer.rows;
}

fn interior(node: Node) ?[4]u16 {
    if (node.cols < 3 or node.rows < 3) return null;
    return .{ node.x + 1, node.y + 1, node.x + node.cols - 2, node.y + node.rows - 2 };
}

fn interiorsOverlap(a: Node, b: Node) bool {
    const ai = interior(a) orelse return false;
    const bi = interior(b) orelse return false;
    return ai[0] <= bi[2] and bi[0] <= ai[2] and ai[1] <= bi[3] and bi[1] <= ai[3];
}

fn rectContains(r: Rect, x: u16, y: u16) bool {
    return x >= r.x and y >= r.y and x < r.x +| r.cols and y < r.y +| r.rows;
}

fn isFloatingDialog(node: Node, cols: u16, rows: u16) bool {
    return node.index == null and node.x > 0 and node.y > 0 and
        node.x +| node.cols < cols and node.y +| node.rows < rows;
}

fn isDialogNode(node: Node, panes: []const Node) bool {
    if (node.index != null or panes.len == 0) return false;
    for (panes) |p| if (strictlyContains(node, p)) return false;
    for (panes) |p| if (interiorsOverlap(p, node)) return true;
    return false;
}

fn isBox(text: []const u8) bool {
    inline for (.{ "─", "│", "┌", "┐", "└", "┘", "╭", "╮", "╰", "╯", "├", "┤", "┬", "┴", "┼", "┏", "┓", "┗", "┛", "━", "┃", "╔", "╗", "╚", "╝", "═", "║", "█", "▐", "▌", "▓" }) |b| {
        if (std.mem.eql(u8, text, b)) return true;
    }
    return false;
}

fn isThumbGlyph(text: []const u8) bool {
    inline for (.{ "┃", "█", "▐", "▌", "▓" }) |b| if (std.mem.eql(u8, text, b)) return true;
    return false;
}

fn isFrameGlyph(text: []const u8) bool {
    return text.len == 0 or isBox(text);
}

fn cornerFamilyOf(text: []const u8) ?CornerFamily {
    if (std.mem.eql(u8, text, "┌")) return .square;
    if (std.mem.eql(u8, text, "╭")) return .rounded;
    return null;
}

fn allWhitespace(s: []const u8) bool {
    for (s) |c| if (c != ' ' and c != '\t') return false;
    return s.len > 0 or true;
}

fn hasAlnum(s: []const u8) bool {
    for (s) |c| {
        if ((c >= '0' and c <= '9') or (c >= 'A' and c <= 'Z') or (c >= 'a' and c <= 'z')) return true;
    }
    // also check utf8 letters roughly: non-ascii non-box
    var i: usize = 0;
    while (i < s.len) {
        const len = std.unicode.utf8ByteSequenceLength(s[i]) catch {
            i += 1;
            continue;
        };
        if (len > 1) {
            const slice = s[i..][0..len];
            if (!isBox(slice) and !std.mem.eql(u8, slice, "─")) return true;
        }
        i += len;
    }
    return false;
}

fn trimAsciiWs(s: []const u8) []const u8 {
    var start: usize = 0;
    var end = s.len;
    while (start < end and (s[start] == ' ' or s[start] == '\t')) start += 1;
    while (end > start and (s[end - 1] == ' ' or s[end - 1] == '\t')) end -= 1;
    return s[start..end];
}

/// Parse one frame. Copies retained title/line strings into the tree arena.
pub fn parse(gpa: Allocator, frame: contract.Frame) Allocator.Error!LayoutTree {
    const arena_ptr = try gpa.create(std.heap.ArenaAllocator);
    arena_ptr.* = .init(gpa);
    var tree = LayoutTree{
        .gpa = gpa,
        .arena_ptr = arena_ptr,
        .cols = frame.cols,
        .rows = frame.rows,
    };
    errdefer tree.deinit();
    const a = tree.arenaAlloc();

    const rects = try findRects(frame, a);
    var nodes: std.ArrayList(Node) = .empty;
    for (rects, 0..) |rect, i| {
        try nodes.append(a, try rectNode(a, frame, rect, @as(u64, @intCast(i)) + 1));
    }
    std.mem.sort(Node, nodes.items, {}, nodeLess);
    tree.next_id = nodes.items.len + 1;
    tree.layout = try nest(a, nodes.items);
    try fillBodies(a, frame, tree.layout, &.{});
    tree.history_layout = try flattenOwned(a, tree.layout);
    tree.chrome = try chromeRows(&tree, frame);
    tree.suppress = try suppressFor(&tree, frame);
    return tree;
}

/// Merge fresh parse into retained identity. Copies strings into the result arena.
pub fn reconcile(gpa: Allocator, frame: contract.Frame, retained: *?LayoutTree) Allocator.Error!LayoutTree {
    if (retained.*) |*prev| {
        if (prev.cols != frame.cols or prev.rows != frame.rows) {
            prev.deinit();
            retained.* = null;
        }
    }
    var fresh = try parse(gpa, frame);
    const merged: LayoutTree = if (retained.*) |*prev| blk: {
        const out = try mergeTrees(gpa, prev, &fresh);
        fresh.deinit();
        break :blk out;
    } else blk: {
        const out = try classifyInitial(gpa, &fresh);
        // classifyInitial returns ownership of fresh's arena (moved).
        break :blk out;
    };
    // Finish chrome/suppress on a mutable local.
    var result = merged;
    result.chrome = try chromeRows(&result, frame);
    result.suppress = try suppressFor(&result, frame);

    if (retained.*) |*prev| prev.deinit();
    retained.* = try cloneTree(gpa, &result);
    return result;
}

/// Cells hidden only when replacement (card/chrome) is drawn.
pub fn suppress_cells(gpa: Allocator, frame: contract.Frame, tree: *const LayoutTree) Allocator.Error![]CellPos {
    return try computeSuppress(gpa, tree, frame);
}

fn nodeLess(_: void, lhs: Node, rhs: Node) bool {
    if (lhs.y != rhs.y) return lhs.y < rhs.y;
    if (lhs.x != rhs.x) return lhs.x < rhs.x;
    if (lhs.cols != rhs.cols) return lhs.cols < rhs.cols;
    return lhs.rows < rhs.rows;
}

fn findRects(frame: contract.Frame, a: Allocator) Allocator.Error![]Rect {
    var rects: std.ArrayList(Rect) = .empty;
    const cols = frame.cols;
    const rows = frame.rows;
    const work_limit = @max(@as(usize, cols) * @as(usize, rows) * 32, 1024);
    var work: usize = 0;
    var y: u16 = 0;
    while (y < rows) : (y += 1) {
        var x: u16 = 0;
        while (x < cols) : (x += 1) {
            const family = cornerFamilyOf(glyphAt(frame, x, y)) orelse continue;
            const cnr = cornersOf(family);
            const top_right = cnr[1];
            const bottom_left = cnr[2];
            const bottom_right = cnr[3];
            var right = x +| (MIN_EDGE_CELLS - 1);
            while (right < cols) : (right += 1) {
                work += 1;
                if (work > work_limit) return &.{};
                if (!std.mem.eql(u8, glyphAt(frame, right, y), top_right)) continue;
                work += right -% x;
                if (work > work_limit) return &.{};
                if (!rowIsEdge(frame, x + 1, right, y)) continue;
                var bottom = y +| (MIN_EDGE_CELLS - 1);
                while (bottom < rows) : (bottom += 1) {
                    work += 1;
                    if (work > work_limit) return &.{};
                    const left = glyphAt(frame, x, bottom);
                    if (std.mem.eql(u8, left, V)) continue;
                    if (!std.mem.eql(u8, left, bottom_left)) break;
                    if (!std.mem.eql(u8, glyphAt(frame, right, bottom), bottom_right)) continue;
                    work += (right -% x) + @as(usize, bottom -% y) * 2;
                    if (work > work_limit) return &.{};
                    if (!rowIsEdge(frame, x + 1, right, bottom)) continue;
                    if (!colIsEdge(frame, y + 1, bottom, x, false) or !colIsEdge(frame, y + 1, bottom, right, true)) continue;
                    try rects.append(a, .{ .x = x, .y = y, .cols = right - x + 1, .rows = bottom - y + 1 });
                    if (rects.items.len > MAX_RECTS) return &.{};
                }
            }
        }
    }
    std.mem.sort(Rect, rects.items, {}, struct {
        fn less(_: void, l: Rect, r: Rect) bool {
            if (l.y != r.y) return l.y < r.y;
            if (l.x != r.x) return l.x < r.x;
            if (l.cols != r.cols) return l.cols < r.cols;
            return l.rows < r.rows;
        }
    }.less);
    // dedup
    var dedup: std.ArrayList(Rect) = .empty;
    for (rects.items) |r| {
        if (dedup.items.len > 0) {
            const p = dedup.items[dedup.items.len - 1];
            if (p.x == r.x and p.y == r.y and p.cols == r.cols and p.rows == r.rows) continue;
        }
        try dedup.append(a, r);
    }
    return try rejectCrossing(a, dedup.items);
}

fn rejectCrossing(a: Allocator, rects: []const Rect) Allocator.Error![]Rect {
    var rejected = try a.alloc(bool, rects.len);
    @memset(rejected, false);
    for (rects, 0..) |ra, i| {
        for (rects[i + 1 ..], i + 1..) |rb, j| {
            if (rectsCross(ra, rb)) {
                rejected[i] = true;
                rejected[j] = true;
            }
        }
    }
    var out: std.ArrayList(Rect) = .empty;
    for (rects, rejected) |r, bad| if (!bad) try out.append(a, r);
    return try out.toOwnedSlice(a);
}

fn rectsCross(a: Rect, b: Rect) bool {
    const ox0 = @max(a.x, b.x);
    const oy0 = @max(a.y, b.y);
    const ox1 = @min(a.x +| a.cols, b.x +| b.cols);
    const oy1 = @min(a.y +| a.rows, b.y +| b.rows);
    if (ox1 <= ox0 or oy1 <= oy0) return false;
    return !rectStrictContains(a, b) and !rectStrictContains(b, a);
}

fn rowIsEdge(frame: contract.Frame, start: u16, end: u16, y: u16) bool {
    var fb = std.heap.stackFallback(4096, std.heap.page_allocator);
    const tmp = fb.get();
    const TextSeg = struct { kind: u8, data: []u8 };
    var segs: std.ArrayList(TextSeg) = .empty;
    defer {
        for (segs.items) |s| tmp.free(s.data);
        segs.deinit(tmp);
    }
    var run: std.ArrayList(u8) = .empty;
    defer run.deinit(tmp);
    var line_cells: u16 = 0;

    var x = start;
    while (x < end) {
        const text = glyphAt(frame, x, y);
        if (cellAt(frame, x, y)) |c| {
            if (c.occupancy == .wide_continuation) {
                x +%= 1;
                continue;
            }
        }
        if (std.mem.eql(u8, text, H)) {
            if (run.items.len > 0) {
                const owned = tmp.dupe(u8, run.items) catch return false;
                segs.append(tmp, .{ .kind = 0, .data = owned }) catch return false;
                run.clearRetainingCapacity();
            }
            line_cells +%= 1;
            x +%= 1;
            continue;
        }
        if (std.mem.eql(u8, text, "┤")) {
            if (run.items.len > 0) {
                const owned = tmp.dupe(u8, run.items) catch return false;
                segs.append(tmp, .{ .kind = 0, .data = owned }) catch return false;
                run.clearRetainingCapacity();
            }
            x +%= 1;
            var label: std.ArrayList(u8) = .empty;
            defer label.deinit(tmp);
            while (x < end) {
                const part = glyphAt(frame, x, y);
                if (cellAt(frame, x, y)) |c| {
                    if (c.occupancy == .wide_continuation) {
                        x +%= 1;
                        continue;
                    }
                }
                if (std.mem.eql(u8, part, "├")) break;
                if (std.mem.eql(u8, part, H) or part.len == 0 or isBox(part)) return false;
                label.appendSlice(tmp, part) catch return false;
                x +%= 1;
            }
            if (x >= end or !std.mem.eql(u8, glyphAt(frame, x, y), "├")) return false;
            if (label.items.len == 0 or trimAsciiWs(label.items).len == 0) return false;
            const owned = tmp.dupe(u8, label.items) catch return false;
            segs.append(tmp, .{ .kind = 1, .data = owned }) catch return false;
            x +%= 1;
            continue;
        }
        if (text.len == 0 or isBox(text)) return false;
        run.appendSlice(tmp, text) catch return false;
        x +%= 1;
    }
    if (run.items.len > 0) {
        const owned = tmp.dupe(u8, run.items) catch return false;
        segs.append(tmp, .{ .kind = 0, .data = owned }) catch return false;
    }
    if (line_cells == 0) return false;
    for (segs.items) |s| {
        if (trimAsciiWs(s.data).len == 0) return false;
    }
    if (segs.items.len == 0) return true;
    if (segs.items.len == 1 and segs.items[0].kind == 0) return true;
    if (segs.items.len == 2 and segs.items[0].kind == 0 and segs.items[1].kind == 0) {
        return panelIndexPrefix(segs.items[0].data) and startsAlpha(segs.items[1].data);
    }
    for (segs.items) |s| if (s.kind != 1) return false;
    return true;
}

fn startsAlpha(s: []const u8) bool {
    if (s.len == 0) return false;
    const c = s[0];
    return (c >= 'A' and c <= 'Z') or (c >= 'a' and c <= 'z');
}

fn panelIndexPrefix(text: []const u8) bool {
    if (text.len < 3 or text[0] != '[') return false;
    const close = std.mem.indexOfScalar(u8, text, ']') orelse return false;
    if (close == 0) return false;
    const num = text[1..close];
    if (num.len == 0) return false;
    for (num) |c| if (c < '0' or c > '9') return false;
    return close == text.len - 1;
}

fn colIsEdge(frame: contract.Frame, start: u16, end: u16, x: u16, allow_thumb: bool) bool {
    if (!allow_thumb) {
        var y = start;
        while (y < end) : (y += 1) {
            if (!std.mem.eql(u8, glyphAt(frame, x, y), V)) return false;
        }
        return true;
    }
    const corner_fg = if (cellAt(frame, x, start -% 1)) |c| c.fg else contract.Color.default;
    var marked: usize = 0;
    var runs: usize = 0;
    var in_run = false;
    var y = start;
    while (y < end) : (y += 1) {
        const text = glyphAt(frame, x, y);
        if (!std.mem.eql(u8, text, V) and !isThumbGlyph(text)) return false;
        const hit = isThumbCell(frame, x, y, corner_fg);
        if (hit) {
            marked += 1;
            if (!in_run) {
                runs += 1;
                in_run = true;
            }
        } else in_run = false;
    }
    const len: usize = end -% start;
    return runs <= 1 and (marked == 0 or marked < len);
}

fn isThumbCell(frame: contract.Frame, x: u16, y: u16, corner_fg: contract.Color) bool {
    const c = cellAt(frame, x, y) orelse return false;
    if (isThumbGlyph(c.grapheme)) return true;
    return std.mem.eql(u8, c.grapheme, V) and !std.meta.eql(c.fg, corner_fg);
}

fn rectNode(a: Allocator, frame: contract.Frame, rect: Rect, id: u64) Allocator.Error!Node {
    const title = try readTitle(a, frame, rect);
    const index = if (title) |t| panelIndex(t) else null;
    return .{
        .id = id,
        .x = rect.x,
        .y = rect.y,
        .cols = rect.cols,
        .rows = rect.rows,
        .title = title,
        .index = index,
        .scroll = readScroll(frame, rect),
        .thumb = readThumb(frame, rect),
        .border = if (cellAt(frame, rect.x, rect.y)) |c| c.fg else .default,
        .body = .empty,
        .children = &.{},
    };
}

fn readTitle(a: Allocator, frame: contract.Frame, rect: Rect) Allocator.Error!?[]const u8 {
    if (rect.cols < 3) return null;
    const y = rect.y;
    const left = rect.x + 1;
    const right = rect.x + rect.cols - 1;
    var first: ?u16 = null;
    var last: ?u16 = null;
    var x = left;
    while (x < right) : (x += 1) {
        if (isFrameGlyph(glyphAt(frame, x, y))) continue;
        if (first == null) first = x;
        last = x;
    }
    var start = first orelse return null;
    var end = last orelse return null;
    while (start < end and trimAsciiWs(glyphAt(frame, start, y)).len == 0) start += 1;
    while (end > start and trimAsciiWs(glyphAt(frame, end, y)).len == 0) end -= 1;
    var title: std.ArrayList(u8) = .empty;
    x = start;
    while (x <= end) : (x += 1) try title.appendSlice(a, glyphAt(frame, x, y));
    if (!hasAlnum(title.items)) return null;
    return try title.toOwnedSlice(a);
}

fn panelIndex(title: []const u8) ?u16 {
    if (title.len < 3 or title[0] != '[') return null;
    const close = std.mem.indexOfScalar(u8, title, ']') orelse return null;
    const num = title[1..close];
    if (num.len != 1 or num[0] < '0' or num[0] > '9') return null;
    var after = title[close + 1 ..];
    while (after.len > 0 and (after[0] == ' ' or std.mem.startsWith(u8, after, H))) {
        if (after[0] == ' ') after = after[1..] else after = after[H.len..];
    }
    if (!(after.len == 0 or startsAlpha(after))) return null;
    const index = num[0] - '0';
    if (index > 5) return null;
    return index;
}

fn readScroll(frame: contract.Frame, rect: Rect) ?struct { u32, u32 } {
    if (rect.rows < 2) return null;
    const bottom = rect.y + rect.rows - 1;
    for ([_]u16{ bottom, bottom -% 1 }) |y| {
        if (findNOfM(frame, rect.x, rect.x + rect.cols, y)) |hit| return .{ hit[0], hit[1] };
    }
    return null;
}

fn findNOfM(frame: contract.Frame, x0: u16, x1: u16, y: u16) ?struct { u32, u32, void } {
    var text_buf: [128]u8 = undefined;
    var text_len: usize = 0;
    var x = x0;
    while (x < x1) : (x += 1) {
        const g = glyphAt(frame, x, y);
        var it = std.unicode.Utf8View.init(g) catch {
            if (matchNOfM(text_buf[0..text_len])) |h| return .{ h[0], h[1], {} };
            text_len = 0;
            continue;
        };
        var cit = it.iterator();
        const ch = cit.nextCodepoint() orelse {
            if (matchNOfM(text_buf[0..text_len])) |h| return .{ h[0], h[1], {} };
            text_len = 0;
            continue;
        };
        const ok_ch = ch <= 0x7f and (std.ascii.isDigit(@intCast(ch)) or ch == ' ' or ch == 'o' or ch == 'f');
        if (cit.nextCodepoint() != null or !ok_ch) {
            if (matchNOfM(text_buf[0..text_len])) |h| return .{ h[0], h[1], {} };
            text_len = 0;
            continue;
        }
        if (text_len < text_buf.len) {
            text_buf[text_len] = @intCast(ch);
            text_len += 1;
        }
    }
    if (matchNOfM(text_buf[0..text_len])) |h| return .{ h[0], h[1], {} };
    return null;
}

fn matchNOfM(text: []const u8) ?struct { u32, u32 } {
    var index: usize = 0;
    while (index < text.len) {
        if (!std.ascii.isDigit(text[index])) {
            index += 1;
            continue;
        }
        const n_start = index;
        while (index < text.len and std.ascii.isDigit(text[index])) index += 1;
        if (index + 4 > text.len or !std.mem.eql(u8, text[index .. index + 4], " of ")) continue;
        const m_start = index + 4;
        var m_end = m_start;
        while (m_end < text.len and std.ascii.isDigit(text[m_end])) m_end += 1;
        if (m_end == m_start) {
            index = m_start;
            continue;
        }
        const current = std.fmt.parseInt(u32, text[n_start..index], 10) catch {
            index = m_end;
            continue;
        };
        const total = std.fmt.parseInt(u32, text[m_start..m_end], 10) catch {
            index = m_end;
            continue;
        };
        if (current == 0 or total == 0 or current > total) {
            index = m_end;
            continue;
        }
        return .{ current, total };
    }
    return null;
}

fn readThumb(frame: contract.Frame, rect: Rect) ?Thumb {
    if (rect.cols < 2 or rect.rows < 4) return null;
    const right = rect.x + rect.cols - 1;
    const track_top = rect.y + 1;
    const track_bottom = rect.y + rect.rows - 2;
    if (track_bottom < track_top) return null;
    const corner_fg = blk: {
        if (cellAt(frame, right, rect.y)) |c| break :blk c.fg;
        if (cellAt(frame, rect.x, rect.y)) |c| break :blk c.fg;
        break :blk contract.Color.default;
    };
    var marked_buf: [256]bool = undefined;
    const track_len = track_bottom - track_top + 1;
    if (track_len > marked_buf.len) return null;
    var any = false;
    var all = true;
    var yi: u16 = 0;
    while (yi < track_len) : (yi += 1) {
        const hit = isThumbCell(frame, right, track_top + yi, corner_fg);
        marked_buf[yi] = hit;
        any = any or hit;
        all = all and hit;
    }
    if (all or !any) return null;
    var best_start: u16 = 0;
    var best_end: u16 = 0;
    var best_len: u32 = 0;
    var run_start: ?u16 = null;
    yi = 0;
    while (yi < track_len) : (yi += 1) {
        const y = track_top + yi;
        if (marked_buf[yi]) {
            if (run_start == null) run_start = y;
        } else if (run_start) |s| {
            const e = y - 1;
            const len = @as(u32, e - s) + 1;
            if (len > best_len) {
                best_len = len;
                best_start = s;
                best_end = e;
            }
            run_start = null;
        }
    }
    if (run_start) |s| {
        const len = @as(u32, track_bottom - s) + 1;
        if (len > best_len) {
            best_start = s;
            best_end = track_bottom;
            best_len = len;
        }
    }
    if (best_len == 0) return null;
    return .{ .start = best_start, .end = best_end, .track_top = track_top, .track_bottom = track_bottom };
}

fn nest(a: Allocator, nodes: []Node) Allocator.Error![]Node {
    const parent = try a.alloc(?usize, nodes.len);
    @memset(parent, null);
    for (nodes, 0..) |_, index| {
        var best: ?usize = null;
        for (nodes, 0..) |_, other| {
            if (index == other or !strictlyContains(nodes[other], nodes[index])) continue;
            const tighter = if (best) |cur| areaOf(nodes[other]) < areaOf(nodes[cur]) else true;
            if (tighter) best = other;
        }
        parent[index] = best;
    }
    var child_indexes = try a.alloc(std.ArrayList(usize), nodes.len);
    for (child_indexes) |*ci| ci.* = .empty;
    var roots: std.ArrayList(usize) = .empty;
    for (parent, 0..) |p, index| {
        if (p) |pi| try child_indexes[pi].append(a, index) else try roots.append(a, index);
    }
    const build = struct {
        fn call(a2: Allocator, index: usize, nodes2: []const Node, children: []std.ArrayList(usize)) Allocator.Error!Node {
            var node = nodes2[index];
            var kids: std.ArrayList(Node) = .empty;
            for (children[index].items) |ci| try kids.append(a2, try call(a2, ci, nodes2, children));
            std.mem.sort(Node, kids.items, {}, struct {
                fn less(_: void, l: Node, r: Node) bool {
                    if (l.y != r.y) return l.y < r.y;
                    if (l.x != r.x) return l.x < r.x;
                    return l.id < r.id;
                }
            }.less);
            node.children = try kids.toOwnedSlice(a2);
            return node;
        }
    }.call;
    var rooted: std.ArrayList(Node) = .empty;
    for (roots.items) |ri| try rooted.append(a, try build(a, ri, nodes, child_indexes));
    std.mem.sort(Node, rooted.items, {}, struct {
        fn less(_: void, l: Node, r: Node) bool {
            if (l.y != r.y) return l.y < r.y;
            if (l.x != r.x) return l.x < r.x;
            return l.id < r.id;
        }
    }.less);
    return try rooted.toOwnedSlice(a);
}

fn flattenOwned(a: Allocator, nodes: []const Node) Allocator.Error![]Node {
    var out: std.ArrayList(Node) = .empty;
    try flattenIntoList(a, &out, nodes);
    return try out.toOwnedSlice(a);
}

fn flattenIntoList(a: Allocator, out: *std.ArrayList(Node), nodes: []const Node) Allocator.Error!void {
    for (nodes) |node| {
        var copy = node;
        copy.children = &.{};
        try out.append(a, copy);
        try flattenIntoList(a, out, node.children);
    }
}

fn fillBodies(a: Allocator, frame: contract.Frame, nodes: []Node, holes: []const Rect) Allocator.Error!void {
    for (nodes) |*node| {
        var covered: std.ArrayList(Rect) = .empty;
        try covered.appendSlice(a, holes);
        for (node.children) |child| try covered.append(a, .{ .x = child.x, .y = child.y, .cols = child.cols, .rows = child.rows });
        node.body = try readBody(a, frame, node.*, covered.items);
        try fillBodies(a, frame, node.children, holes);
    }
}

fn readBody(a: Allocator, frame: contract.Frame, node: Node, holes: []const Rect) Allocator.Error!PaneBody {
    const bounds = interior(node) orelse return .empty;
    var lines: std.ArrayList(ContentLine) = .empty;
    var y = bounds[1];
    while (y <= bounds[3]) : (y += 1) {
        var xs: std.ArrayList(u16) = .empty;
        var x = bounds[0];
        while (x <= bounds[2]) : (x += 1) {
            var covered = false;
            for (holes) |h| if (rectContains(h, x, y)) {
                covered = true;
                break;
            };
            if (!covered) try xs.append(a, x);
        }
        if (try collectLine(a, frame, y, xs.items)) |line| try lines.append(a, line);
    }
    return classify(try lines.toOwnedSlice(a));
}

fn collectLine(a: Allocator, frame: contract.Frame, y: u16, xs: []const u16) Allocator.Error!?ContentLine {
    var spans: std.ArrayList(Span) = .empty;
    var selected = false;
    for (xs) |x| {
        const cell = cellAt(frame, x, y) orelse continue;
        if (cell.occupancy == .wide_continuation) continue;
        if (cell.style.reverse) selected = true;
        try pushSpan(a, &spans, cell);
    }
    var any = false;
    for (spans.items) |sp| {
        if (trimAsciiWs(sp.text).len > 0) {
            any = true;
            break;
        }
    }
    if (!any) return null;
    return .{ .spans = try spans.toOwnedSlice(a), .selected = selected };
}

fn pushSpan(a: Allocator, spans: *std.ArrayList(Span), cell: *const contract.Cell) Allocator.Error!void {
    const text = if (cell.grapheme.len == 0) " " else cell.grapheme;
    if (spans.items.len > 0) {
        const last = &spans.items[spans.items.len - 1];
        if (std.meta.eql(last.fg, cell.fg) and std.meta.eql(last.bg, cell.bg) and std.meta.eql(last.style, cell.style)) {
            const joined = try std.mem.concat(a, u8, &.{ last.text, text });
            last.text = joined;
            return;
        }
    }
    try spans.append(a, .{ .text = try a.dupe(u8, text), .fg = cell.fg, .bg = cell.bg, .style = cell.style });
}

fn classify(lines: []ContentLine) PaneBody {
    if (lines.len == 0) return .empty;
    var diffs: usize = 0;
    for (lines) |line| {
        if (isDiffLine(line)) diffs += 1;
    }
    if (diffs >= 3) return .{ .text = lines };
    return .{ .rows = lines };
}

fn lineText(line: ContentLine, a: Allocator) Allocator.Error![]u8 {
    var out: std.ArrayList(u8) = .empty;
    for (line.spans) |sp| try out.appendSlice(a, sp.text);
    return try out.toOwnedSlice(a);
}

fn isDiffLine(line: ContentLine) bool {
    // stack concat
    var buf: [512]u8 = undefined;
    var len: usize = 0;
    for (line.spans) |sp| {
        const n = @min(sp.text.len, buf.len - len);
        @memcpy(buf[len..][0..n], sp.text[0..n]);
        len += n;
    }
    const text = buf[0..len];
    const trimmed = trimAsciiWs(text);
    return std.mem.startsWith(u8, trimmed, "+") or std.mem.startsWith(u8, trimmed, "-") or std.mem.indexOf(u8, text, "@@") != null;
}

fn chromeRows(tree: *LayoutTree, frame: contract.Frame) Allocator.Error![]ChromeRow {
    const a = tree.arenaAlloc();
    if (tree.layout.len == 0 and tree.overlays.len == 0) return &.{};
    var rects: std.ArrayList(Rect) = .empty;
    try collectRects(a, &rects, tree.layout);
    try collectRects(a, &rects, tree.overlays);
    for (tree.history_layout) |n| try rects.append(a, .{ .x = n.x, .y = n.y, .cols = n.cols, .rows = n.rows });
    var rows: std.ArrayList(ChromeRow) = .empty;
    var y: u16 = 0;
    while (y < frame.rows) : (y += 1) {
        var inside = false;
        for (rects.items) |r| {
            if (y >= r.y and y < r.y +| r.rows) {
                inside = true;
                break;
            }
        }
        if (inside) continue;
        var xs: std.ArrayList(u16) = .empty;
        var x: u16 = 0;
        while (x < frame.cols) : (x += 1) try xs.append(a, x);
        if (try collectLine(a, frame, y, xs.items)) |line| try rows.append(a, .{ .y = y, .line = line });
    }
    return try rows.toOwnedSlice(a);
}

fn collectRects(a: Allocator, out: *std.ArrayList(Rect), nodes: []const Node) Allocator.Error!void {
    for (nodes) |n| {
        try out.append(a, .{ .x = n.x, .y = n.y, .cols = n.cols, .rows = n.rows });
        try collectRects(a, out, n.children);
    }
}

fn suppressFor(tree: *LayoutTree, frame: contract.Frame) Allocator.Error![]CellPos {
    return try computeSuppress(tree.arenaAlloc(), tree, frame);
}

fn computeSuppress(a: Allocator, tree: *const LayoutTree, frame: contract.Frame) Allocator.Error![]CellPos {
    var set: std.AutoHashMapUnmanaged(u32, void) = .empty;
    defer set.deinit(a);
    const nodes = try flattenOwned(a, tree.layout);
    defer a.free(nodes);
    const overlays = try flattenOwned(a, tree.overlays);
    defer a.free(overlays);
    for (nodes) |node| try markRect(&set, a, tree, node);
    for (overlays) |node| try markRect(&set, a, tree, node);
    for (tree.chrome) |row| {
        var x: u16 = 0;
        while (x < tree.cols) : (x += 1) try set.put(a, cellKey(x, row.y), {});
    }
    var keys: std.ArrayList(u32) = .empty;
    defer keys.deinit(a);
    var it = set.keyIterator();
    while (it.next()) |k| try keys.append(a, k.*);
    for (keys.items) |k| {
        const x: u16 = @intCast(k >> 16);
        const y: u16 = @intCast(k & 0xffff);
        const c = cellAt(frame, x, y) orelse continue;
        switch (c.occupancy) {
            .wide_leading => if (x + 1 < tree.cols) try set.put(a, cellKey(x + 1, y), {}),
            .wide_continuation => if (x > 0) try set.put(a, cellKey(x -% 1, y), {}),
            .single => {},
        }
    }
    var out: std.ArrayList(CellPos) = .empty;
    errdefer out.deinit(a);
    var it2 = set.keyIterator();
    while (it2.next()) |k| try out.append(a, .{ .x = @intCast(k.* >> 16), .y = @intCast(k.* & 0xffff) });
    return try out.toOwnedSlice(a);
}

fn markRect(set: *std.AutoHashMapUnmanaged(u32, void), a: Allocator, tree: *const LayoutTree, node: Node) Allocator.Error!void {
    const right = @min(node.x +| node.cols, tree.cols);
    const bottom = @min(node.y +| node.rows, tree.rows);
    var y = node.y;
    while (y < bottom) : (y += 1) {
        var x = node.x;
        while (x < right) : (x += 1) try set.put(a, cellKey(x, y), {});
    }
}

fn classifyInitial(gpa: Allocator, tree: *LayoutTree) Allocator.Error!LayoutTree {
    const a = tree.arenaAlloc();
    const nodes = try flattenOwned(a, tree.layout);
    var overlays: std.ArrayList(Node) = .empty;
    var layout: std.ArrayList(Node) = .empty;
    for (nodes) |node| {
        if (isFloatingDialog(node, tree.cols, tree.rows)) try overlays.append(a, node) else try layout.append(a, node);
    }
    if (overlays.items.len == 0) return tree.*;
    tree.layout = try nest(a, layout.items);
    tree.overlays = try nest(a, overlays.items);
    tree.history_layout = try layout.toOwnedSlice(a);
    tree.history_overlays = try overlays.toOwnedSlice(a);
    tree.chrome = &.{};
    tree.suppress = &.{};
    _ = gpa;
    return tree.*;
}

fn mergeTrees(gpa: Allocator, prev: *LayoutTree, fresh: *LayoutTree) Allocator.Error!LayoutTree {
    const arena_ptr = try gpa.create(std.heap.ArenaAllocator);
    arena_ptr.* = .init(gpa);
    errdefer {
        arena_ptr.deinit();
        gpa.destroy(arena_ptr);
    }
    const a = arena_ptr.allocator();
    var next_id = prev.next_id;

    const prev_layout = if (prev.history_layout.len == 0) try cloneNodes(a, try flattenOwned(a, prev.layout)) else try cloneNodes(a, prev.history_layout);
    const prev_overlay = if (prev.history_overlays.len == 0) try cloneNodes(a, try flattenOwned(a, prev.overlays)) else try cloneNodes(a, prev.history_overlays);

    var fresh_nodes: std.ArrayList(Node) = .empty;
    try fresh_nodes.appendSlice(a, try flattenOwned(a, fresh.layout));
    try fresh_nodes.appendSlice(a, try flattenOwned(a, fresh.overlays));
    // deep copy titles into new arena
    for (fresh_nodes.items) |*n| n.* = try cloneNode(a, n.*);

    var overlay_mask = try a.alloc(bool, fresh_nodes.items.len);
    for (fresh_nodes.items, 0..) |node, i| {
        const pane_score = bestScore(node, prev_layout, paneMatchScore);
        const overlay_score = bestScore(node, prev_overlay, overlayMatchScore);
        overlay_mask[i] = node.index == null and (isFloatingDialog(node, fresh.cols, fresh.rows) or
            (pane_score == 0 and isDialogNode(node, prev_layout)) or
            (overlay_score > 0 and overlay_score > pane_score));
    }

    var layout_fresh: std.ArrayList(Node) = .empty;
    var overlay_fresh: std.ArrayList(Node) = .empty;
    for (fresh_nodes.items, overlay_mask) |node, ov| {
        if (ov) try overlay_fresh.append(a, node) else try layout_fresh.append(a, node);
    }

    var layout_history: std.ArrayList(Node) = .empty;
    try layout_history.appendSlice(a, prev_layout);
    for (prev_overlay) |node| if (node.index != null) try layout_history.append(a, node);

    const layout_matches = try assignGlobal(a, layout_fresh.items, layout_history.items, paneMatchScore);
    const overlay_matches = try assignGlobal(a, overlay_fresh.items, prev_overlay, overlayMatchScore);
    var layout = try assignIds(a, layout_fresh.items, layout_history.items, layout_matches, &next_id);
    const overlays = try assignIds(a, overlay_fresh.items, prev_overlay, overlay_matches, &next_id);

    if (overlays.len > 0) {
        for (layout) |*node| {
            for (overlays) |ov| {
                if (iou(node.*, ov) > 0) {
                    for (prev_layout) |previous| {
                        if (previous.id == node.id) {
                            node.body = try cloneBody(a, previous.body);
                            break;
                        }
                    }
                }
            }
        }
    }

    var history_layout = try cloneNodes(a, layout);
    if (overlays.len > 0) {
        for (prev_layout) |node| {
            var hit = false;
            for (overlays) |ov| if (iou(node, ov) > 0) {
                hit = true;
                break;
            };
            if (hit and !isFloatingDialog(node, fresh.cols, fresh.rows)) {
                var tracked = false;
                for (history_layout) |t| if (t.id == node.id) {
                    tracked = true;
                    break;
                };
                if (!tracked) {
                    const copied = try cloneNode(a, node);
                    var list: std.ArrayList(Node) = .empty;
                    try list.appendSlice(a, history_layout);
                    try list.append(a, copied);
                    history_layout = try list.toOwnedSlice(a);
                }
            }
        }
        if (history_layout.len > MAX_RECTS) history_layout = history_layout[0..MAX_RECTS];
        layout = try cloneNodes(a, history_layout);
    }
    const history_overlays = try cloneNodes(a, overlays);
    std.mem.sort(Node, layout, {}, struct {
        fn less(_: void, l: Node, r: Node) bool {
            if (l.y != r.y) return l.y < r.y;
            if (l.x != r.x) return l.x < r.x;
            return l.id < r.id;
        }
    }.less);
    std.mem.sort(Node, overlays, {}, struct {
        fn less(_: void, l: Node, r: Node) bool {
            if (l.y != r.y) return l.y < r.y;
            if (l.x != r.x) return l.x < r.x;
            return l.id < r.id;
        }
    }.less);

    return .{
        .gpa = gpa,
        .arena_ptr = arena_ptr,
        .cols = fresh.cols,
        .rows = fresh.rows,
        .layout = try nest(a, layout),
        .overlays = try nest(a, overlays),
        .history_layout = history_layout,
        .history_overlays = history_overlays,
        .next_id = next_id,
    };
}

fn assignIds(a: Allocator, fresh: []Node, previous: []const Node, matches: []const ?usize, next_id: *u64) Allocator.Error![]Node {
    const out = try a.alloc(Node, fresh.len);
    for (fresh, matches, 0..) |node, m, i| {
        var n = node;
        n.children = &.{};
        n.id = if (m) |idx| previous[idx].id else takeId(next_id);
        out[i] = n;
    }
    return out;
}

fn takeId(next_id: *u64) u64 {
    const id = next_id.*;
    next_id.* = next_id.* +| 1;
    return id;
}

fn bestScore(node: Node, previous: []const Node, scoreFn: *const fn (Node, Node) i64) i64 {
    var best: i64 = 0;
    for (previous) |cand| best = @max(best, scoreFn(node, cand));
    return best;
}

fn titlesMatch(a: Node, b: Node) bool {
    const at = a.title orelse return false;
    const bt = b.title orelse return false;
    // case insensitive ascii trim
    const aa = trimAsciiWs(at);
    const bb = trimAsciiWs(bt);
    if (aa.len != bb.len) return false;
    for (aa, bb) |ca, cb| {
        const la = if (ca >= 'A' and ca <= 'Z') ca + 32 else ca;
        const lb = if (cb >= 'A' and cb <= 'Z') cb + 32 else cb;
        if (la != lb) return false;
    }
    return true;
}

fn sizeSimilarity(a: Node, b: Node) i64 {
    const aa: u64 = areaOf(a);
    const ba: u64 = areaOf(b);
    const larger = @max(aa, ba);
    if (larger == 0) return 0;
    return @intCast(aa * 1000 / larger);
}

fn sharesEdge(a: Node, b: Node) bool {
    const a_right = a.x +| a.cols;
    const a_bottom = a.y +| a.rows;
    const b_right = b.x +| b.cols;
    const b_bottom = b.y +| b.rows;
    return (a.x == b.x and sideOverlap(a.y, a_bottom, b.y, b_bottom, a.rows, b.rows)) or
        (a_right == b_right and sideOverlap(a.y, a_bottom, b.y, b_bottom, a.rows, b.rows)) or
        (a.y == b.y and sideOverlap(a.x, a_right, b.x, b_right, a.cols, b.cols)) or
        (a_bottom == b_bottom and sideOverlap(a.x, a_right, b.x, b_right, a.cols, b.cols));
}

fn sideOverlap(a0: u16, a1: u16, b0: u16, b1: u16, a_len: u16, b_len: u16) bool {
    const start = @max(a0, b0);
    const end = @min(a1, b1);
    if (end <= start) return false;
    const shorter = @min(a_len, b_len);
    return shorter > 0 and @as(u32, end - start) * 2 >= shorter;
}

fn paneMatchScore(fresh: Node, previous: Node) i64 {
    if (fresh.index) |fi| if (previous.index) |pi| if (fi != pi) return 0;
    const overlap: i64 = @intFromFloat(@round(iou(fresh, previous) * 1000.0));
    const same_index = fresh.index != null and std.meta.eql(fresh.index, previous.index);
    const same_title = titlesMatch(fresh, previous);
    const shared = sharesEdge(fresh, previous);
    if (!same_index and overlap == 0 and !shared) return 0;
    if (!same_index and !same_title and !shared and (strictlyContains(fresh, previous) or strictlyContains(previous, fresh))) return 0;
    if (!same_index and !same_title and overlap < 500 and !shared) return 0;
    var score = overlap *| 10;
    if (same_index) score +|= 100_000;
    if (same_title) score +|= 20_000;
    if (shared) score +|= 5_000;
    return score +| sizeSimilarity(fresh, previous);
}

fn overlayMatchScore(fresh: Node, previous: Node) i64 {
    const overlap: i64 = @intFromFloat(@round(iou(fresh, previous) * 1000.0));
    const same_title = titlesMatch(fresh, previous);
    if (overlap < 250 and !same_title) return 0;
    return overlap *| 10 +| (if (same_title) @as(i64, 20_000) else 0) +| sizeSimilarity(fresh, previous);
}

fn assignGlobal(a: Allocator, fresh: []const Node, previous: []const Node, scoreFn: *const fn (Node, Node) i64) Allocator.Error![]?usize {
    const result = try a.alloc(?usize, fresh.len);
    @memset(result, null);
    if (fresh.len == 0 or previous.len == 0) return result;
    const size = @max(fresh.len, previous.len);
    const scores = try a.alloc([]i64, size);
    for (scores, 0..) |*row, r| {
        row.* = try a.alloc(i64, size);
        for (row.*, 0..) |*cell, c| {
            cell.* = if (r < fresh.len and c < previous.len) scoreFn(fresh[r], previous[c]) else 0;
        }
    }
    var max_score: i64 = 0;
    for (scores) |row| {
        for (row) |s| max_score = @max(max_score, s);
    }

    var u = try a.alloc(i64, size + 1);
    var v = try a.alloc(i64, size + 1);
    var p = try a.alloc(usize, size + 1);
    var way = try a.alloc(usize, size + 1);
    @memset(u, 0);
    @memset(v, 0);
    @memset(p, 0);
    @memset(way, 0);

    var row: usize = 1;
    while (row <= size) : (row += 1) {
        p[0] = row;
        var col0: usize = 0;
        var minv = try a.alloc(i64, size + 1);
        var used = try a.alloc(bool, size + 1);
        @memset(minv, std.math.maxInt(i64));
        @memset(used, false);
        while (true) {
            used[col0] = true;
            const row0 = p[col0];
            var delta: i64 = std.math.maxInt(i64);
            var col1: usize = 0;
            var col: usize = 1;
            while (col <= size) : (col += 1) {
                if (used[col]) continue;
                const cost = max_score -| scores[row0 - 1][col - 1] -| u[row0] -| v[col];
                if (cost < minv[col]) {
                    minv[col] = cost;
                    way[col] = col0;
                }
                if (minv[col] < delta) {
                    delta = minv[col];
                    col1 = col;
                }
            }
            col = 0;
            while (col <= size) : (col += 1) {
                if (used[col]) {
                    u[p[col]] +|= delta;
                    v[col] -|= delta;
                } else minv[col] -|= delta;
            }
            col0 = col1;
            if (p[col0] == 0) break;
        }
        while (true) {
            const col1 = way[col0];
            p[col0] = p[col1];
            col0 = col1;
            if (col0 == 0) break;
        }
    }

    var col: usize = 1;
    while (col <= size) : (col += 1) {
        const r = p[col];
        if (r == 0 or r > fresh.len or col > previous.len) continue;
        if (scores[r - 1][col - 1] > 0) result[r - 1] = col - 1;
    }
    return result;
}

fn cloneTree(gpa: Allocator, src: *const LayoutTree) Allocator.Error!LayoutTree {
    const arena_ptr = try gpa.create(std.heap.ArenaAllocator);
    arena_ptr.* = .init(gpa);
    errdefer {
        arena_ptr.deinit();
        gpa.destroy(arena_ptr);
    }
    const a = arena_ptr.allocator();
    return .{
        .gpa = gpa,
        .arena_ptr = arena_ptr,
        .cols = src.cols,
        .rows = src.rows,
        .layout = try cloneNodes(a, src.layout),
        .overlays = try cloneNodes(a, src.overlays),
        .chrome = try cloneChrome(a, src.chrome),
        .suppress = try a.dupe(CellPos, src.suppress),
        .history_layout = try cloneNodes(a, src.history_layout),
        .history_overlays = try cloneNodes(a, src.history_overlays),
        .next_id = src.next_id,
        .log_signature = try a.dupe(u8, src.log_signature),
    };
}

fn cloneNodes(a: Allocator, nodes: []const Node) Allocator.Error![]Node {
    const out = try a.alloc(Node, nodes.len);
    for (nodes, 0..) |n, i| out[i] = try cloneNode(a, n);
    return out;
}

fn cloneNode(a: Allocator, node: Node) Allocator.Error!Node {
    return .{
        .id = node.id,
        .x = node.x,
        .y = node.y,
        .cols = node.cols,
        .rows = node.rows,
        .title = if (node.title) |t| try a.dupe(u8, t) else null,
        .index = node.index,
        .scroll = node.scroll,
        .thumb = node.thumb,
        .border = node.border,
        .body = try cloneBody(a, node.body),
        .children = try cloneNodes(a, node.children),
    };
}

fn cloneBody(a: Allocator, body: PaneBody) Allocator.Error!PaneBody {
    return switch (body) {
        .empty => .empty,
        .rows => |lines| .{ .rows = try cloneLines(a, lines) },
        .text => |lines| .{ .text = try cloneLines(a, lines) },
    };
}

fn cloneLines(a: Allocator, lines: []const ContentLine) Allocator.Error![]ContentLine {
    const out = try a.alloc(ContentLine, lines.len);
    for (lines, 0..) |line, i| out[i] = .{ .selected = line.selected, .spans = try cloneSpans(a, line.spans) };
    return out;
}

fn cloneSpans(a: Allocator, spans: []const Span) Allocator.Error![]Span {
    const out = try a.alloc(Span, spans.len);
    for (spans, 0..) |sp, i| out[i] = .{ .text = try a.dupe(u8, sp.text), .fg = sp.fg, .bg = sp.bg, .style = sp.style };
    return out;
}

fn cloneChrome(a: Allocator, rows: []const ChromeRow) Allocator.Error![]ChromeRow {
    const out = try a.alloc(ChromeRow, rows.len);
    for (rows, 0..) |row, i| out[i] = .{ .y = row.y, .line = .{ .selected = row.line.selected, .spans = try cloneSpans(a, row.line.spans) } };
    return out;
}

fn appendNodes(a: Allocator, list: *std.ArrayList(u8), nodes: []const Node, prefix: []const u8, kind: []const u8) Allocator.Error!void {
    for (nodes, 0..) |node, index| {
        const last = index + 1 == nodes.len;
        const branch = if (last) "└── " else "├── ";
        try list.appendSlice(a, prefix);
        try list.appendSlice(a, branch);
        try appendNodeLine(a, list, node, kind);
        const child_prefix = if (last)
            try std.fmt.allocPrint(a, "{s}    ", .{prefix})
        else
            try std.fmt.allocPrint(a, "{s}│   ", .{prefix});
        defer a.free(child_prefix);
        try appendNodes(a, list, node.children, child_prefix, kind);
    }
}

fn appendNodeLine(a: Allocator, list: *std.ArrayList(u8), node: Node, kind: []const u8) Allocator.Error!void {
    const line = try std.fmt.allocPrint(a, "{s} {d} @{d},{d} {d}×{d}", .{ kind, node.id, node.x, node.y, node.cols, node.rows });
    defer a.free(line);
    try list.appendSlice(a, line);
    if (node.title) |t| {
        const part = try std.fmt.allocPrint(a, " \"{s}\"", .{t});
        defer a.free(part);
        try list.appendSlice(a, part);
    }
    if (node.scroll) |s| {
        const part = try std.fmt.allocPrint(a, " {d}/{d}", .{ s[0], s[1] });
        defer a.free(part);
        try list.appendSlice(a, part);
    }
    if (node.thumb) |t| {
        const part = try std.fmt.allocPrint(a, " thumb {d}-{d}", .{ t.start, t.end });
        defer a.free(part);
        try list.appendSlice(a, part);
    }
    switch (node.body) {
        .rows => |lines| {
            const part = try std.fmt.allocPrint(a, " rows {d}", .{lines.len});
            defer a.free(part);
            try list.appendSlice(a, part);
        },
        .text => |lines| {
            const part = try std.fmt.allocPrint(a, " text {d}", .{lines.len});
            defer a.free(part);
            try list.appendSlice(a, part);
        },
        .empty => {},
    }
    try list.append(a, '\n');
}

// --- tests ---

fn blankCell(x: u16, y: u16) contract.Cell {
    return .{ .x = x, .y = y, .grapheme = " ", .fg = .default, .bg = .default, .style = .{}, .occupancy = .single };
}

fn testGrid(a: Allocator, cols: u16, rows: u16, paint: *const fn (*contract.Cell) void) !contract.Frame {
    const cells = try a.alloc(contract.Cell, @as(usize, cols) * rows);
    var y: u16 = 0;
    while (y < rows) : (y += 1) {
        var x: u16 = 0;
        while (x < cols) : (x += 1) {
            var cell = blankCell(x, y);
            paint(&cell);
            cells[@as(usize, y) * cols + x] = cell;
        }
    }
    // dupe graphemes into stable memory
    for (cells) |*c| c.grapheme = try a.dupe(u8, c.grapheme);
    return .{ .cols = cols, .rows = rows, .cursor = .{ .x = 0, .y = 0, .visible = false, .wide_tail = false }, .cells = cells };
}

fn stampFamily(cell: *contract.Cell, x0: u16, y0: u16, x1: u16, y1: u16, family: CornerFamily) void {
    const x = cell.x;
    const y = cell.y;
    if (x < x0 or x > x1 or y < y0 or y > y1) return;
    const c = cornersOf(family);
    const g: []const u8 = if (x == x0 and y == y0) c[0] else if (x == x1 and y == y0) c[1] else if (x == x0 and y == y1) c[2] else if (x == x1 and y == y1) c[3] else if (y == y0 or y == y1) H else if (x == x0 or x == x1) V else return;
    cell.grapheme = g;
}

fn stamp(cell: *contract.Cell, x0: u16, y0: u16, x1: u16, y1: u16) void {
    stampFamily(cell, x0, y0, x1, y1, .square);
}

fn stampRounded(cell: *contract.Cell, x0: u16, y0: u16, x1: u16, y1: u16) void {
    stampFamily(cell, x0, y0, x1, y1, .rounded);
}

fn writeAt(cell: *contract.Cell, x: u16, y: u16, text: []const u8) void {
    if (cell.y != y or cell.x < x) return;
    var i: usize = 0;
    var cx = x;
    while (i < text.len) {
        const len = std.unicode.utf8ByteSequenceLength(text[i]) catch {
            i += 1;
            continue;
        };
        if (cell.x == cx) {
            cell.grapheme = text[i..][0..len];
            return;
        }
        cx += 1;
        i += len;
    }
}

fn writeKeycap(cell: *contract.Cell, x: u16, y: u16, label: []const u8) void {
    if (cell.y != y) return;
    const count = std.unicode.utf8CountCodepoints(label) catch label.len;
    const end = x + 1 + @as(u16, @intCast(count));
    if (cell.x == x) cell.grapheme = "┤" else if (cell.x == end) cell.grapheme = "├" else if (cell.x > x and cell.x < end) {
        var i: usize = 0;
        var idx: u16 = 0;
        while (i < label.len) {
            const len = std.unicode.utf8ByteSequenceLength(label[i]) catch break;
            if (idx == cell.x - x - 1) {
                cell.grapheme = label[i..][0..len];
                return;
            }
            idx += 1;
            i += len;
        }
    }
}

fn suppressHas(tree: *const LayoutTree, x: u16, y: u16) bool {
    for (tree.suppress) |p| if (p.x == x and p.y == y) return true;
    return false;
}

test "two panes are layout siblings" {
    const a = std.testing.allocator;
    var arena = std.heap.ArenaAllocator.init(a);
    defer arena.deinit();
    const aa = arena.allocator();
    const Paint = struct {
        fn call(cell: *contract.Cell) void {
            stamp(cell, 0, 0, 11, 11);
            stamp(cell, 12, 0, 31, 11);
            writeAt(cell, 2, 0, "Files");
        }
    };
    const frame = try testGrid(aa, 32, 12, Paint.call);
    var tree = try parse(a, frame);
    defer tree.deinit();
    const text = try tree.toString(a);
    defer a.free(text);
    try std.testing.expect(std.mem.indexOf(u8, text, "├── Layout") != null);
    try std.testing.expect(std.mem.indexOf(u8, text, "Panel 1 @0,0 12×12 \"Files\"") != null);
    try std.testing.expect(std.mem.indexOf(u8, text, "Panel 2 @12,0 20×12") != null);
}

test "bubble title on top edge is a panel" {
    const a = std.testing.allocator;
    var arena = std.heap.ArenaAllocator.init(a);
    defer arena.deinit();
    const aa = arena.allocator();
    const Paint = struct {
        fn call(cell: *contract.Cell) void {
            stampRounded(cell, 0, 0, 23, 5);
            writeKeycap(cell, 2, 0, " Processes ");
        }
    };
    const frame = try testGrid(aa, 24, 6, Paint.call);
    var tree = try parse(a, frame);
    defer tree.deinit();
    try std.testing.expectEqual(@as(usize, 1), tree.layout.len);
    try std.testing.expectEqualStrings("Processes", tree.layout[0].title.?);
    try std.testing.expect(suppressHas(&tree, 2, 0));
    try std.testing.expect(suppressHas(&tree, 4, 0));
}

test "tee junction on edge without keycap stays raw" {
    const a = std.testing.allocator;
    var arena = std.heap.ArenaAllocator.init(a);
    defer arena.deinit();
    const aa = arena.allocator();
    const Paint = struct {
        fn call(cell: *contract.Cell) void {
            stamp(cell, 0, 0, 15, 5);
            if (cell.y == 0 and cell.x == 8) cell.grapheme = "┬";
        }
    };
    const frame = try testGrid(aa, 16, 6, Paint.call);
    var tree = try parse(a, frame);
    defer tree.deinit();
    try std.testing.expectEqual(@as(u64, 0), tree.nodeCount());
}

test "ambiguous or broken frames stay raw" {
    const a = std.testing.allocator;
    var arena = std.heap.ArenaAllocator.init(a);
    defer arena.deinit();
    const aa = arena.allocator();
    const Broken = struct {
        fn call(cell: *contract.Cell) void {
            stamp(cell, 0, 0, 15, 7);
            if (cell.x == 6 and cell.y == 0) cell.grapheme = " ";
        }
    };
    const frame = try testGrid(aa, 16, 8, Broken.call);
    var tree = try parse(a, frame);
    defer tree.deinit();
    try std.testing.expectEqual(@as(u64, 0), tree.nodeCount());
    try std.testing.expectEqual(@as(usize, 0), tree.suppress.len);
}

test "dialog merges above the previous panes" {
    const a = std.testing.allocator;
    var arena = std.heap.ArenaAllocator.init(a);
    defer arena.deinit();
    const aa = arena.allocator();
    const Clean = struct {
        fn call(cell: *contract.Cell) void {
            stamp(cell, 0, 0, 11, 11);
            stamp(cell, 12, 0, 31, 11);
        }
    };
    const clean = try testGrid(aa, 32, 12, Clean.call);
    var retained: ?LayoutTree = null;
    {
        var first = try reconcile(a, clean, &retained);
        defer first.deinit();
        var left_id: u64 = 0;
        for (first.layout) |n| {
            if (n.x == 0) left_id = n.id;
        }
        try std.testing.expect(left_id != 0);
    }

    const Covered = struct {
        fn call(cell: *contract.Cell) void {
            stamp(cell, 0, 0, 11, 11);
            stamp(cell, 12, 0, 31, 11);
            stamp(cell, 8, 2, 22, 9);
            writeAt(cell, 10, 2, "Keys");
        }
    };
    const covered = try testGrid(aa, 32, 12, Covered.call);
    var tree = try reconcile(a, covered, &retained);
    defer tree.deinit();
    defer if (retained) |*t| t.deinit();
    const text = try tree.toString(a);
    defer a.free(text);
    try std.testing.expect(std.mem.indexOf(u8, text, "└── Overlay") != null);
    try std.testing.expect(std.mem.indexOf(u8, text, "Dialog") != null);
    try std.testing.expect(std.mem.indexOf(u8, text, "\"Keys\"") != null);
    try std.testing.expectEqual(@as(usize, 1), tree.overlays.len);
}

test "right edge block thumb is parsed" {
    const a = std.testing.allocator;
    var arena = std.heap.ArenaAllocator.init(a);
    defer arena.deinit();
    const aa = arena.allocator();
    const Paint = struct {
        fn call(cell: *contract.Cell) void {
            stamp(cell, 0, 0, 19, 11);
            writeAt(cell, 2, 0, "Commits");
            if (cell.x == 19 and cell.y >= 4 and cell.y <= 7) cell.grapheme = "▐";
        }
    };
    const frame = try testGrid(aa, 20, 12, Paint.call);
    var tree = try parse(a, frame);
    defer tree.deinit();
    const thumb = tree.layout[0].thumb.?;
    try std.testing.expectEqual(@as(u16, 4), thumb.start);
    try std.testing.expectEqual(@as(u16, 7), thumb.end);
}

test "suppression expands across wide cell pairs" {
    const a = std.testing.allocator;
    var arena = std.heap.ArenaAllocator.init(a);
    defer arena.deinit();
    const aa = arena.allocator();
    const Paint = struct {
        fn call(cell: *contract.Cell) void {
            if (cell.x == 1 and cell.y == 1) {
                cell.grapheme = "界";
                cell.occupancy = .wide_leading;
            }
            if (cell.x == 2 and cell.y == 1) {
                cell.grapheme = "";
                cell.occupancy = .wide_continuation;
            }
        }
    };
    const frame = try testGrid(aa, 8, 4, Paint.call);
    const arena_ptr = try a.create(std.heap.ArenaAllocator);
    arena_ptr.* = .init(a);
    var tree = LayoutTree{
        .gpa = a,
        .arena_ptr = arena_ptr,
        .cols = 8,
        .rows = 4,
        .layout = undefined,
        .next_id = 2,
    };
    defer tree.deinit();
    const ta = tree.arenaAlloc();
    const nodes = try ta.alloc(Node, 1);
    nodes[0] = .{ .id = 1, .x = 2, .y = 0, .cols = 4, .rows = 4, .title = try ta.dupe(u8, "pane") };
    tree.layout = nodes;
    const suppress = try suppress_cells(a, frame, &tree);
    defer a.free(suppress);
    var has1 = false;
    var has2 = false;
    for (suppress) |p| {
        if (p.x == 1 and p.y == 1) has1 = true;
        if (p.x == 2 and p.y == 1) has2 = true;
    }
    try std.testing.expect(has1);
    try std.testing.expect(has2);
}

test "MIN_EDGE_CELLS is three" {
    try std.testing.expectEqual(@as(u16, 3), MIN_EDGE_CELLS);
}
