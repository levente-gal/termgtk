//! Herdr snapshot parse + release gate. No process spawn.
//! Tab layout reconstruction lives in layout.zig (separate module; no cross-import).

const std = @import("std");
const contract = @import("contract");

const Allocator = std.mem.Allocator;
const Value = std.json.Value;

pub const EXPECTED_HERDR_VERSION = "0.8.0";
pub const EXPECTED_SCHEMA_SHA256 = "88ff414aa996e390c2db05a37b95d28dbe4e81b98329f6ed7f7a2cc5c6ebf51a";

pub const protocol_mismatch_message =
    "Herdr protocol mismatch (version/schema); rerun discovery against the installed binary";

pub const Error = error{
    ProtocolMismatch,
    SnapshotParse,
    SnapshotMissingFocus,
    OutOfMemory,
};

/// Release gate: Herdr 0.8.0 + pinned schema SHA-256. No process spawn.
pub fn checkProtocolGate(version: []const u8, schema_stdout: []const u8) Error!void {
    if (!std.mem.eql(u8, version, EXPECTED_HERDR_VERSION)) return error.ProtocolMismatch;
    var digest: [32]u8 = undefined;
    std.crypto.hash.sha2.Sha256.hash(schema_stdout, &digest, .{});
    const hex = std.fmt.bytesToHex(digest, .lower);
    if (!std.ascii.eqlIgnoreCase(&hex, EXPECTED_SCHEMA_SHA256)) return error.ProtocolMismatch;
}

/// Parse snapshot envelope into contract DTOs.
/// `layout` / `layout_error` are left null; callers use `layout.tabLayout` (module split).
pub fn fromSnapshotJson(allocator: Allocator, json: []const u8) Error!contract.HerdrSnapshot {
    const root = std.json.parseFromSliceLeaky(Value, allocator, json, .{}) catch return error.SnapshotParse;
    const snapshot = (blk: {
        const result = objGet(root, "result") orelse break :blk null;
        break :blk objGet(result, "snapshot");
    }) orelse return error.SnapshotParse;

    const workspaces = try parseWorkspaces(allocator, snapshot);
    const agents = try parseAgents(allocator, snapshot, workspaces);
    const focused_workspace_id = nonemptyString(objGet(snapshot, "focused_workspace_id"));
    const focused = focusedPaneFromValue(root) catch null;

    return .{
        .workspaces = workspaces,
        .agents = agents,
        .focused_workspace_id = focused_workspace_id,
        .focused = focused,
        .layout = null,
        .layout_error = null,
    };
}

pub fn focusedPaneFromSnapshotJson(allocator: Allocator, json: []const u8) Error!contract.FocusedPane {
    const root = std.json.parseFromSliceLeaky(Value, allocator, json, .{}) catch return error.SnapshotParse;
    return focusedPaneFromValue(root);
}

fn focusedPaneFromValue(root: Value) Error!contract.FocusedPane {
    const snapshot = (blk: {
        const result = objGet(root, "result") orelse break :blk null;
        break :blk objGet(result, "snapshot");
    }) orelse return error.SnapshotMissingFocus;

    const workspace_id = nonemptyString(objGet(snapshot, "focused_workspace_id")) orelse return error.SnapshotMissingFocus;
    const tab_id = nonemptyString(objGet(snapshot, "focused_tab_id")) orelse return error.SnapshotMissingFocus;
    const pane_id = nonemptyString(objGet(snapshot, "focused_pane_id")) orelse return error.SnapshotMissingFocus;

    return .{
        .workspace_id = workspace_id,
        .tab_id = tab_id,
        .pane_id = pane_id,
        .workspace_label = labelInArray(objGet(snapshot, "workspaces"), "workspace_id", workspace_id, "label"),
        .tab_label = labelInArray(objGet(snapshot, "tabs"), "tab_id", tab_id, "label"),
        .terminal_title = labelInArray(objGet(snapshot, "panes"), "pane_id", pane_id, "terminal_title"),
    };
}

fn labelInArray(arr_val: ?Value, id_key: []const u8, id: []const u8, label_key: []const u8) ?[]const u8 {
    const arr = switch (arr_val orelse return null) {
        .array => |a| a.items,
        else => return null,
    };
    for (arr) |entry| {
        const eid = nonemptyString(objGet(entry, id_key)) orelse continue;
        if (!std.mem.eql(u8, eid, id)) continue;
        return nonemptyString(objGet(entry, label_key));
    }
    return null;
}

fn parseWorkspaces(allocator: Allocator, snapshot: Value) Error![]contract.WorkspaceRow {
    const entries_val = objGet(snapshot, "workspaces") orelse return error.SnapshotParse;
    const entries = switch (entries_val) {
        .array => |a| a.items,
        else => return error.SnapshotParse,
    };
    var rows: std.ArrayListUnmanaged(contract.WorkspaceRow) = .empty;
    errdefer rows.deinit(allocator);
    for (entries) |entry| {
        const workspace_id = nonemptyString(objGet(entry, "workspace_id")) orelse continue;
        const number = asU64(objGet(entry, "number") orelse continue) orelse continue;
        const label = nonemptyString(objGet(entry, "label")) orelse workspace_id;
        const agent_status = parseAgentStatus(nonemptyString(objGet(entry, "agent_status"))) orelse .unknown;
        try rows.append(allocator, .{
            .workspace_id = workspace_id,
            .label = label,
            .number = number,
            .agent_status = agent_status,
            .branch = null,
            .git_status = null,
            .worktree_path = null,
        });
    }
    std.mem.sort(contract.WorkspaceRow, rows.items, {}, struct {
        fn less(_: void, a: contract.WorkspaceRow, b: contract.WorkspaceRow) bool {
            if (a.number != b.number) return a.number < b.number;
            return std.mem.order(u8, a.workspace_id, b.workspace_id) == .lt;
        }
    }.less);
    return try rows.toOwnedSlice(allocator);
}

const AgentSort = struct {
    row: contract.AgentRow,
    workspace_number: ?u64,
    tab_number: u64,
};

fn parseAgents(allocator: Allocator, snapshot: Value, workspaces: []const contract.WorkspaceRow) Error![]contract.AgentRow {
    const agents_val = objGet(snapshot, "agents") orelse return &[_]contract.AgentRow{};
    const entries = switch (agents_val) {
        .array => |a| a.items,
        else => return error.SnapshotParse,
    };
    var rows: std.ArrayListUnmanaged(AgentSort) = .empty;
    errdefer rows.deinit(allocator);
    for (entries) |entry| {
        const pane_id = nonemptyString(objGet(entry, "pane_id")) orelse continue;
        const agent_status = parseAgentStatus(nonemptyString(objGet(entry, "agent_status"))) orelse continue;
        const workspace_id = nonemptyString(objGet(entry, "workspace_id")) orelse "";
        var workspace_label: []const u8 = workspace_id;
        var workspace_number: ?u64 = null;
        for (workspaces) |w| {
            if (std.mem.eql(u8, w.workspace_id, workspace_id)) {
                workspace_label = w.label;
                workspace_number = w.number;
                break;
            }
        }
        const tab_id = nonemptyString(objGet(entry, "tab_id")) orelse "";
        const meta = tabMeta(snapshot, tab_id);
        const identity = nonemptyString(objGet(entry, "name")) orelse
            nonemptyString(objGet(entry, "display_agent")) orelse
            nonemptyString(objGet(entry, "agent")) orelse
            pane_id;
        try rows.append(allocator, .{
            .row = .{
                .pane_id = pane_id,
                .agent_status = agent_status,
                .workspace_id = workspace_id,
                .workspace_label = workspace_label,
                .tab_label = meta.label,
                .identity = identity,
            },
            .workspace_number = workspace_number,
            .tab_number = meta.number,
        });
    }
    std.mem.sort(AgentSort, rows.items, {}, struct {
        fn less(_: void, a: AgentSort, b: AgentSort) bool {
            const an = a.workspace_number;
            const bn = b.workspace_number;
            if (an != null and bn != null) {
                if (an.? != bn.?) return an.? < bn.?;
            } else if (an != null) {
                return true;
            } else if (bn != null) {
                return false;
            } else if (!std.mem.eql(u8, a.row.workspace_id, b.row.workspace_id)) {
                return std.mem.order(u8, a.row.workspace_id, b.row.workspace_id) == .lt;
            }
            if (a.tab_number != b.tab_number) return a.tab_number < b.tab_number;
            return std.mem.order(u8, a.row.pane_id, b.row.pane_id) == .lt;
        }
    }.less);
    var out: std.ArrayListUnmanaged(contract.AgentRow) = .empty;
    errdefer out.deinit(allocator);
    for (rows.items) |s| try out.append(allocator, s.row);
    return try out.toOwnedSlice(allocator);
}

fn tabMeta(snapshot: Value, tab_id: []const u8) struct { label: []const u8, number: u64 } {
    if (tab_id.len == 0) return .{ .label = "", .number = std.math.maxInt(u64) };
    const tabs_val = objGet(snapshot, "tabs") orelse return .{ .label = tab_id, .number = std.math.maxInt(u64) };
    const tabs = switch (tabs_val) {
        .array => |a| a.items,
        else => return .{ .label = tab_id, .number = std.math.maxInt(u64) },
    };
    for (tabs) |tab| {
        const tid = nonemptyString(objGet(tab, "tab_id")) orelse continue;
        if (!std.mem.eql(u8, tid, tab_id)) continue;
        const label = nonemptyString(objGet(tab, "label")) orelse tab_id;
        const number = asU64(objGet(tab, "number") orelse .{ .null = {} }) orelse std.math.maxInt(u64);
        return .{ .label = label, .number = number };
    }
    return .{ .label = tab_id, .number = std.math.maxInt(u64) };
}

fn parseAgentStatus(s: ?[]const u8) ?contract.AgentStatus {
    const v = s orelse return null;
    if (std.mem.eql(u8, v, "idle")) return .idle;
    if (std.mem.eql(u8, v, "working")) return .working;
    if (std.mem.eql(u8, v, "blocked")) return .blocked;
    if (std.mem.eql(u8, v, "done")) return .done;
    if (std.mem.eql(u8, v, "unknown")) return .unknown;
    return null;
}

fn objGet(v: Value, key: []const u8) ?Value {
    return switch (v) {
        .object => |o| o.get(key),
        else => null,
    };
}

fn nonemptyString(v: ?Value) ?[]const u8 {
    const val = v orelse return null;
    return switch (val) {
        .string => |s| if (s.len == 0) null else s,
        else => null,
    };
}

fn asU64(v: Value) ?u64 {
    return switch (v) {
        .integer => |i| if (i >= 0) @intCast(i) else null,
        .float => |f| blk: {
            if (!std.math.isFinite(f) or f < 0 or f != @floor(f)) break :blk null;
            break :blk @intFromFloat(f);
        },
        .null => null,
        else => null,
    };
}

fn loadText(allocator: Allocator, path: []const u8) ![]u8 {
    return try std.Io.Dir.cwd().readFileAlloc(std.testing.io, path, allocator, .limited(20 << 20));
}

test "protocol gate accepts expected version and schema hash" {
    const alloc = std.testing.allocator;
    const schema = try loadText(alloc, "herdr-native-poc/docs/contracts/herdr-api.schema.json");
    defer alloc.free(schema);
    try checkProtocolGate(EXPECTED_HERDR_VERSION, schema);
}

test "protocol gate rejects version mismatch" {
    try std.testing.expectError(error.ProtocolMismatch, checkProtocolGate("0.0.0", "{}"));
    try std.testing.expect(std.mem.indexOf(u8, protocol_mismatch_message, "rerun discovery") != null);
}

test "protocol gate rejects schema hash mismatch" {
    try std.testing.expectError(error.ProtocolMismatch, checkProtocolGate(EXPECTED_HERDR_VERSION, "not-the-schema"));
}

test "sha256 matches known empty" {
    var digest: [32]u8 = undefined;
    std.crypto.hash.sha2.Sha256.hash("", &digest, .{});
    const hex = std.fmt.bytesToHex(digest, .lower);
    try std.testing.expectEqualStrings(
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
        &hex,
    );
}

test "parses focused pane from snapshot fixture" {
    const alloc = std.testing.allocator;
    const json = try loadText(alloc, "herdr-native-poc/docs/contracts/snapshot.json");
    defer alloc.free(json);
    var arena = std.heap.ArenaAllocator.init(alloc);
    defer arena.deinit();
    const pane = try focusedPaneFromSnapshotJson(arena.allocator(), json);
    try std.testing.expectEqualStrings("w9:p2", pane.pane_id);
    try std.testing.expectEqualStrings("w9", pane.workspace_id);
    try std.testing.expectEqualStrings("w9:t1", pane.tab_id);
    try std.testing.expectEqualStrings("REDACTED", pane.workspace_label.?);
    try std.testing.expectEqualStrings("1", pane.tab_label.?);
    try std.testing.expectEqualStrings("REDACTED", pane.terminal_title.?);
}

test "parses workspaces from snapshot fixture in number order" {
    const alloc = std.testing.allocator;
    const json = try loadText(alloc, "herdr-native-poc/docs/contracts/snapshot.json");
    defer alloc.free(json);
    var arena = std.heap.ArenaAllocator.init(alloc);
    defer arena.deinit();
    const snap = try fromSnapshotJson(arena.allocator(), json);
    try std.testing.expectEqual(@as(usize, 5), snap.workspaces.len);
    try std.testing.expectEqual(@as(u64, 1), snap.workspaces[0].number);
    try std.testing.expectEqual(@as(u64, 5), snap.workspaces[4].number);
    try std.testing.expectEqualStrings("w1", snap.workspaces[0].workspace_id);
    try std.testing.expectEqualStrings("w4", snap.workspaces[1].workspace_id);
    try std.testing.expectEqualStrings("w5", snap.workspaces[2].workspace_id);
    try std.testing.expectEqualStrings("w8", snap.workspaces[3].workspace_id);
    try std.testing.expectEqualStrings("w9", snap.workspaces[4].workspace_id);
    try std.testing.expectEqualStrings("w9", snap.focused_workspace_id.?);
    try std.testing.expectEqualStrings("w9:p2", snap.focused.?.pane_id);
}

test "workspace rows sort by number and missing label uses id" {
    const json =
        \\{"result":{"snapshot":{
        \\  "focused_workspace_id":"b",
        \\  "workspaces":[
        \\    {"workspace_id":"b","number":2,"label":null},
        \\    {"workspace_id":"a","number":1,"label":""}
        \\  ]
        \\}}}
    ;
    var arena = std.heap.ArenaAllocator.init(std.testing.allocator);
    defer arena.deinit();
    const snap = try fromSnapshotJson(arena.allocator(), json);
    try std.testing.expectEqualStrings("a", snap.workspaces[0].workspace_id);
    try std.testing.expectEqualStrings("a", snap.workspaces[0].label);
    try std.testing.expectEqualStrings("b", snap.workspaces[1].workspace_id);
    try std.testing.expectEqualStrings("b", snap.workspaces[1].label);
    try std.testing.expectEqualStrings("b", snap.focused_workspace_id.?);
    try std.testing.expect(snap.focused == null);
}

test "parses agents from snapshot fixture in workspace order" {
    const alloc = std.testing.allocator;
    const json = try loadText(alloc, "herdr-native-poc/docs/contracts/snapshot.json");
    defer alloc.free(json);
    var arena = std.heap.ArenaAllocator.init(alloc);
    defer arena.deinit();
    const snap = try fromSnapshotJson(arena.allocator(), json);
    try std.testing.expectEqual(@as(usize, 4), snap.agents.len);
    try std.testing.expectEqualStrings("w4:p1", snap.agents[0].pane_id);
    try std.testing.expectEqualStrings("w5:p1", snap.agents[1].pane_id);
    try std.testing.expectEqualStrings("w8:p1", snap.agents[2].pane_id);
    try std.testing.expectEqualStrings("w9:p2", snap.agents[3].pane_id);
    for (snap.agents) |row| {
        try std.testing.expectEqualStrings("opencode", row.identity);
        try std.testing.expect(row.agent_status == .idle);
        try std.testing.expectEqualStrings("1", row.tab_label);
        try std.testing.expectEqualStrings("REDACTED", row.workspace_label);
    }
}

test "agent rows sort by workspace then tab and fall back identity" {
    const json =
        \\{"result":{"snapshot":{
        \\  "workspaces":[
        \\    {"workspace_id":"b","number":2,"label":"beta"},
        \\    {"workspace_id":"a","number":1,"label":""}
        \\  ],
        \\  "tabs":[
        \\    {"tab_id":"a:t2","number":2,"label":"second"},
        \\    {"tab_id":"a:t1","number":1,"label":null},
        \\    {"tab_id":"b:t1","number":1,"label":"only"}
        \\  ],
        \\  "agents":[
        \\    {"pane_id":"b:p1","agent_status":"working","workspace_id":"b","tab_id":"b:t1","agent":"codex","name":"named","cwd":"/tmp/secret","terminal_title":"title"},
        \\    {"pane_id":"a:p2","agent_status":"idle","workspace_id":"a","tab_id":"a:t2","agent":"claude"},
        \\    {"pane_id":"z:p1","agent_status":"blocked","workspace_id":"missing","tab_id":"missing:t1","display_agent":"shown"},
        \\    {"pane_id":"a:p1","agent_status":"done","workspace_id":"a","tab_id":"a:t1"},
        \\    {"pane_id":"skip","workspace_id":"a"},
        \\    {"agent_status":"idle","workspace_id":"a","agent":"no-pane"},
        \\    {"pane_id":"bad","agent_status":"asleep","workspace_id":"a","agent":"nope"}
        \\  ]
        \\}}}
    ;
    var arena = std.heap.ArenaAllocator.init(std.testing.allocator);
    defer arena.deinit();
    const snap = try fromSnapshotJson(arena.allocator(), json);
    try std.testing.expectEqual(@as(usize, 4), snap.agents.len);
    try std.testing.expectEqualStrings("a:p1", snap.agents[0].pane_id);
    try std.testing.expectEqualStrings("a:p1", snap.agents[0].identity);
    try std.testing.expectEqualStrings("a", snap.agents[0].workspace_label);
    try std.testing.expectEqualStrings("a:t1", snap.agents[0].tab_label);
    try std.testing.expect(snap.agents[0].agent_status == .done);
    try std.testing.expectEqualStrings("claude", snap.agents[1].identity);
    try std.testing.expectEqualStrings("second", snap.agents[1].tab_label);
    try std.testing.expectEqualStrings("named", snap.agents[2].identity);
    try std.testing.expectEqualStrings("beta", snap.agents[2].workspace_label);
    try std.testing.expectEqualStrings("shown", snap.agents[3].identity);
    try std.testing.expectEqualStrings("missing", snap.agents[3].workspace_label);
    try std.testing.expectEqualStrings("missing:t1", snap.agents[3].tab_label);
}
