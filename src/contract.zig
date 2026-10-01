//! Frozen Herdr↔native contract. Fields only. No I/O, Ghostty, or GTK.

pub const Frame = struct {
    cols: u16,
    rows: u16,
    cursor: Cursor,
    cells: []Cell,
};

pub const Cursor = struct {
    x: u16,
    y: u16,
    visible: bool,
    wide_tail: bool,
};

pub const Cell = struct {
    x: u16,
    y: u16,
    grapheme: []const u8,
    fg: Color,
    bg: Color,
    style: Style,
    occupancy: Occupancy,
};

pub const Style = struct {
    bold: bool = false,
    italic: bool = false,
    underline: bool = false,
    reverse: bool = false,
};

pub const Color = union(enum) {
    default,
    indexed: u8,
    rgb: struct { r: u8, g: u8, b: u8 },
};

pub const Occupancy = enum {
    single,
    wide_leading,
    wide_continuation,
};

pub const FrameBytes = struct {
    bytes: []const u8,
    full: bool,
    seq: u64,
    cols: u16,
    rows: u16,
};

pub const TerminalEvent = union(enum) {
    data: FrameBytes,
    closed: CloseReason,
};

pub const CloseReason = struct {
    reason: []const u8,
};

pub const FocusedPane = struct {
    workspace_id: []const u8,
    tab_id: []const u8,
    pane_id: []const u8,
    workspace_label: ?[]const u8,
    tab_label: ?[]const u8,
    terminal_title: ?[]const u8,
};

pub const WorkspaceRow = struct {
    workspace_id: []const u8,
    label: []const u8,
    number: u64,
    agent_status: AgentStatus,
    branch: ?[]const u8,
    git_status: ?[]const u8,
    worktree_path: ?[]const u8,
};

pub const AgentStatus = enum {
    idle,
    working,
    blocked,
    done,
    unknown,
};

pub const AgentRow = struct {
    pane_id: []const u8,
    agent_status: AgentStatus,
    workspace_id: []const u8,
    workspace_label: []const u8,
    tab_label: []const u8,
    identity: []const u8,
};

pub const SplitDirection = enum {
    right,
    down,
};

pub const SplitRatio = struct {
    value: f64,
};

pub const LayoutNode = union(enum) {
    pane: struct { pane_id: []const u8 },
    split: struct {
        id: []const u8,
        direction: SplitDirection,
        ratio: SplitRatio,
        first: *LayoutNode,
        second: *LayoutNode,
    },
};

pub const PaneDirection = enum {
    left,
    right,
    up,
    down,
};

pub const PlacedPane = struct {
    pane_id: []const u8,
    x: u16,
    y: u16,
    width: u16,
    height: u16,
};

pub const TabLayout = struct {
    workspace_id: []const u8,
    tab_id: []const u8,
    zoomed: bool,
    focused_pane_id: []const u8,
    root: LayoutNode,
    panes: []PlacedPane,
};

pub const HerdrSnapshot = struct {
    workspaces: []WorkspaceRow,
    agents: []AgentRow,
    focused_workspace_id: ?[]const u8,
    focused: ?FocusedPane,
    layout: ?TabLayout,
    layout_error: ?[]const u8,
};

// --- app.rs / terminal_stream.rs (types only; logic lives in rules.zig) ---

pub const GridSize = struct {
    cols: u16,
    rows: u16,
    cell_w: u32,
    cell_h: u32,
};

pub const FrameFit = enum {
    use,
    acknowledged,
    ignore,
};

/// One in-flight Herdr resize and at most one newer target.
pub const ResizeSync = struct {
    confirmed: GridSize,
    inflight: ?GridSize = null,
    pending: ?GridSize = null,
};

pub const ObserveFit = enum {
    draw,
    skip,
    disconnect,
};

pub const ScrollDirection = enum {
    up,
    down,
};

pub const ScrollSource = enum {
    wheel,
    page_key,
};

pub const ScrollCommand = struct {
    direction: ScrollDirection,
    lines: u16,
    source: ScrollSource,
    column: ?u16 = null,
    row: ?u16 = null,
};

// --- semantic names (real fields live in semantic/*.zig; report deviations to parent) ---

pub const LayoutTree = opaque {};
pub const Node = opaque {};
pub const PaneBody = opaque {};
pub const ContentLine = opaque {};
pub const Span = opaque {};
pub const FrameRect = opaque {};
pub const CardSlot = opaque {};
