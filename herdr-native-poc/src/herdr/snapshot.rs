//! Focused-pane discovery against the Herdr CLI (protocol 19 / Herdr 0.8.0).

use std::process::{Command, Stdio};

use serde_json::Value;
use thiserror::Error;

/// Expected Herdr version string for this POC contract.
pub const EXPECTED_HERDR_VERSION: &str = "0.8.0";

/// SHA-256 of `herdr api schema --json` stdout for the contracted schema.
pub const EXPECTED_SCHEMA_SHA256: &str =
    "88ff414aa996e390c2db05a37b95d28dbe4e81b98329f6ed7f7a2cc5c6ebf51a";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FocusedPane {
    pub workspace_id: String,
    pub tab_id: String,
    pub pane_id: String,
    pub workspace_label: Option<String>,
    pub tab_label: Option<String>,
    pub terminal_title: Option<String>,
}

/// One sidebar row. Label text is the snapshot label, or `workspace_id` when
/// that label is missing. Cwd and terminal titles are not kept.
///
/// `branch` and `git_status` are filled after the snapshot parse. `worktree_path`
/// is the checkout used to read git status and is not shown.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorkspaceRow {
    pub workspace_id: String,
    pub label: String,
    pub number: u64,
    pub agent_status: AgentStatus,
    pub branch: Option<String>,
    pub git_status: Option<String>,
    pub worktree_path: Option<String>,
}

/// `agents[].agent_status` from the Herdr snapshot schema.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AgentStatus {
    Idle,
    Working,
    Blocked,
    Done,
    Unknown,
}

impl AgentStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Working => "working",
            Self::Blocked => "blocked",
            Self::Done => "done",
            Self::Unknown => "unknown",
        }
    }

    fn parse(value: &str) -> Option<Self> {
        match value {
            "idle" => Some(Self::Idle),
            "working" => Some(Self::Working),
            "blocked" => Some(Self::Blocked),
            "done" => Some(Self::Done),
            "unknown" => Some(Self::Unknown),
            _ => None,
        }
    }

    /// CSS class for the status dot. Paired with `herdr-status` in the stylesheet.
    pub fn mark_class(self) -> &'static str {
        match self {
            Self::Idle => "herdr-status-idle",
            Self::Working => "herdr-status-working",
            Self::Blocked => "herdr-status-blocked",
            Self::Done => "herdr-status-done",
            Self::Unknown => "herdr-status-unknown",
        }
    }
}

/// One agent-panel row. Identity is `name`, else `display_agent`, else `agent`,
/// else `pane_id`. Cwd and terminal titles are not kept.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AgentRow {
    pub pane_id: String,
    pub agent_status: AgentStatus,
    pub workspace_id: String,
    pub workspace_label: String,
    pub tab_label: String,
    pub identity: String,
}

/// Cell rectangle from a pane-layout snapshot. Used only to rebuild the split tree.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct CellRect {
    x: u16,
    y: u16,
    width: u16,
    height: u16,
}

/// `splits[].direction`. Herdr only reports `right` and `down`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SplitDirection {
    Right,
    Down,
}

/// Fraction of a split given to the first child (left or top). Never NaN.
#[derive(Clone, Copy, Debug)]
pub struct SplitRatio(f64);

impl PartialEq for SplitRatio {
    fn eq(&self, other: &Self) -> bool {
        self.0.to_bits() == other.0.to_bits()
    }
}

impl Eq for SplitRatio {}

impl SplitRatio {
    pub fn get(self) -> f64 {
        self.0
    }
}

/// One node of the focused tab after containment is resolved.
///
/// Pane rects are not kept. They only decide which child is on which side.
/// `ratio` is what allocates pixels inside the card.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LayoutNode {
    Pane {
        pane_id: String,
    },
    Split {
        id: String,
        direction: SplitDirection,
        ratio: SplitRatio,
        first: Box<LayoutNode>,
        second: Box<LayoutNode>,
    },
}

/// One step of `herdr pane focus --direction`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PaneDirection {
    Left,
    Right,
    Up,
    Down,
}

impl PaneDirection {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Left => "left",
            Self::Right => "right",
            Self::Up => "up",
            Self::Down => "down",
        }
    }
}

/// A pane's cell rect in the focused tab. Used to choose a focus direction.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlacedPane {
    pub pane_id: String,
    pub x: u16,
    pub y: u16,
    pub width: u16,
    pub height: u16,
}

/// Zoom-filtered layout of the focused tab.
///
/// When `zoomed` is set, `root` is only `focused_pane_id`. A live snapshot on
/// this machine had `zoomed: false` on every layout, so the flag is not given
/// any other meaning.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TabLayout {
    pub workspace_id: String,
    pub tab_id: String,
    pub zoomed: bool,
    pub focused_pane_id: String,
    pub root: LayoutNode,
    pub panes: Vec<PlacedPane>,
}

impl TabLayout {
    /// Panes that should be observed and painted, in tree order.
    pub fn visible_panes(&self) -> Vec<String> {
        fn walk(node: &LayoutNode, out: &mut Vec<String>) {
            match node {
                LayoutNode::Pane { pane_id } => out.push(pane_id.clone()),
                LayoutNode::Split { first, second, .. } => {
                    walk(first, out);
                    walk(second, out);
                }
            }
        }
        let mut out = Vec::new();
        walk(&self.root, &mut out);
        out
    }

    /// Direction of one `herdr pane focus --direction` step from `from` toward `to`.
    ///
    /// The installed CLI has no by-id focus. A click sends this one step; a pane
    /// that is not the immediate neighbor is reached by another click.
    pub fn direction_toward(&self, from: &str, to: &str) -> Option<PaneDirection> {
        if from == to {
            return None;
        }
        let from = self.panes.iter().find(|pane| pane.pane_id == from)?;
        let to = self.panes.iter().find(|pane| pane.pane_id == to)?;
        let dx = i32::from(to.center_x()) - i32::from(from.center_x());
        let dy = i32::from(to.center_y()) - i32::from(from.center_y());
        if dx == 0 && dy == 0 {
            return None;
        }
        if dx.abs() >= dy.abs() {
            Some(if dx > 0 {
                PaneDirection::Right
            } else {
                PaneDirection::Left
            })
        } else {
            Some(if dy > 0 {
                PaneDirection::Down
            } else {
                PaneDirection::Up
            })
        }
    }
}

impl PlacedPane {
    fn center_x(&self) -> u16 {
        self.x.saturating_add(self.width / 2)
    }

    fn center_y(&self) -> u16 {
        self.y.saturating_add(self.height / 2)
    }
}

/// Workspaces, agents, the focused pane, and the focused tab's layout.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HerdrSnapshot {
    pub workspaces: Vec<WorkspaceRow>,
    pub agents: Vec<AgentRow>,
    pub focused_workspace_id: Option<String>,
    pub focused: Option<FocusedPane>,
    /// Layout of the focused tab. `None` when that tab was not selected.
    pub layout: Option<TabLayout>,
    /// Set when the focused tab's layout was rejected. The previous split must stay.
    pub layout_error: Option<String>,
}

#[derive(Debug, Error)]
pub enum HerdrError {
    #[error("Herdr server is not running; start it with `herdr server`")]
    ServerNotRunning,

    #[error(
        "Herdr protocol mismatch (version/schema); rerun discovery against the installed binary"
    )]
    ProtocolMismatch,

    #[error("Herdr snapshot JSON is missing focused pane fields")]
    SnapshotMissingFocus,

    #[error("Herdr snapshot parse failed: {0}")]
    SnapshotParse(String),

    #[error("Herdr tab layout rejected: {0}")]
    LayoutRejected(String),

    #[error("invalid Herdr NDJSON record")]
    InvalidNdjson,

    #[error("terminal.frame is missing bytes")]
    MissingFrameBytes,

    #[error("terminal.frame bytes are not valid standard base64")]
    InvalidFrameBase64,

    #[error("terminal.frame encoding must be ansi")]
    UnexpectedFrameEncoding,

    #[error("Herdr stream sequence went backwards; resync by restarting observe")]
    SequenceResyncRequired,

    #[error("Herdr stream ended unexpectedly (EOF)")]
    StreamEof,

    #[error("Herdr process exited with status {status}: {stderr}")]
    ProcessExit { status: i32, stderr: String },

    #[error("timed out waiting for Herdr stream event")]
    Timeout,

    #[error("Herdr stderr: {0}")]
    Stderr(String),

    #[error("Herdr control input rejected: {0}")]
    ControlInputRejected(String),

    #[error("Herdr I/O error: {0}")]
    Io(String),

    #[error("Herdr command failed: {0}")]
    Command(String),

    #[error("invalid terminal.resize: cols and rows must be greater than 0")]
    InvalidResize,

    #[error("Herdr backend is not implemented yet")]
    Unimplemented,
}

impl FocusedPane {
    /// Parse a full `herdr api snapshot` JSON envelope into the focused pane.
    pub fn from_snapshot_json(json: &str) -> Result<Self, HerdrError> {
        let root: Value =
            serde_json::from_str(json).map_err(|e| HerdrError::SnapshotParse(e.to_string()))?;
        Self::from_snapshot_value(&root)
    }

    fn from_snapshot_value(root: &Value) -> Result<Self, HerdrError> {
        let snapshot = root
            .pointer("/result/snapshot")
            .ok_or(HerdrError::SnapshotMissingFocus)?;

        let workspace_id = snapshot
            .get("focused_workspace_id")
            .and_then(Value::as_str)
            .ok_or(HerdrError::SnapshotMissingFocus)?
            .to_string();
        let tab_id = snapshot
            .get("focused_tab_id")
            .and_then(Value::as_str)
            .ok_or(HerdrError::SnapshotMissingFocus)?
            .to_string();
        let pane_id = snapshot
            .get("focused_pane_id")
            .and_then(Value::as_str)
            .ok_or(HerdrError::SnapshotMissingFocus)?
            .to_string();

        let workspace_label = snapshot
            .get("workspaces")
            .and_then(Value::as_array)
            .and_then(|arr| {
                arr.iter().find_map(|w| {
                    (w.get("workspace_id").and_then(Value::as_str) == Some(workspace_id.as_str()))
                        .then(|| w.get("label").and_then(Value::as_str).map(str::to_string))
                        .flatten()
                })
            });

        let tab_label = snapshot
            .get("tabs")
            .and_then(Value::as_array)
            .and_then(|arr| {
                arr.iter().find_map(|t| {
                    (t.get("tab_id").and_then(Value::as_str) == Some(tab_id.as_str()))
                        .then(|| t.get("label").and_then(Value::as_str).map(str::to_string))
                        .flatten()
                })
            });

        let terminal_title = snapshot
            .get("panes")
            .and_then(Value::as_array)
            .and_then(|arr| {
                arr.iter().find_map(|p| {
                    (p.get("pane_id").and_then(Value::as_str) == Some(pane_id.as_str()))
                        .then(|| {
                            p.get("terminal_title")
                                .and_then(Value::as_str)
                                .map(str::to_string)
                        })
                        .flatten()
                })
            });

        Ok(Self {
            workspace_id,
            tab_id,
            pane_id,
            workspace_label,
            tab_label,
            terminal_title,
        })
    }
}

impl HerdrSnapshot {
    /// Parse workspaces and the optional focused pane from one snapshot envelope.
    pub fn from_snapshot_json(json: &str) -> Result<Self, HerdrError> {
        let root: Value =
            serde_json::from_str(json).map_err(|e| HerdrError::SnapshotParse(e.to_string()))?;
        let snapshot = root
            .pointer("/result/snapshot")
            .ok_or_else(|| HerdrError::SnapshotParse("missing result.snapshot".to_string()))?;
        let workspaces = parse_workspaces(snapshot)?;
        let agents = parse_agents(snapshot, &workspaces)?;
        let focused_workspace_id = nonempty_string(snapshot.get("focused_workspace_id"));
        let focused_tab_id = nonempty_string(snapshot.get("focused_tab_id"));
        let focused = FocusedPane::from_snapshot_value(&root).ok();
        let (layout, layout_error) = match (&focused_workspace_id, &focused_tab_id) {
            (Some(workspace_id), Some(tab_id)) => {
                match tab_layout(snapshot, workspace_id, tab_id) {
                    Ok(layout) => (Some(layout), None),
                    Err(err) => (None, Some(err.to_string())),
                }
            }
            _ => (None, None),
        };
        Ok(Self {
            workspaces,
            agents,
            focused_workspace_id,
            focused,
            layout,
            layout_error,
        })
    }
}

/// Layout whose `workspace_id` and `tab_id` match the focused tab.
///
/// Other workspaces' layouts are ignored. A layout with no unique root,
/// ambiguous containment, or children that do not occupy both sides is
/// rejected. Nothing is invented in its place.
pub fn tab_layout(
    snapshot: &Value,
    workspace_id: &str,
    tab_id: &str,
) -> Result<TabLayout, HerdrError> {
    let layouts = snapshot
        .get("layouts")
        .and_then(Value::as_array)
        .ok_or_else(|| HerdrError::LayoutRejected("missing layouts".into()))?;
    let mut matches = layouts.iter().filter(|layout| {
        layout.get("workspace_id").and_then(Value::as_str) == Some(workspace_id)
            && layout.get("tab_id").and_then(Value::as_str) == Some(tab_id)
    });
    let layout = matches.next().ok_or_else(|| {
        HerdrError::LayoutRejected(format!("no layout for {workspace_id} {tab_id}"))
    })?;
    if matches.next().is_some() {
        return Err(HerdrError::LayoutRejected(format!(
            "ambiguous layout for {workspace_id} {tab_id}"
        )));
    }
    reconstruct_layout(layout, workspace_id, tab_id)
}

fn reconstruct_layout(
    layout: &Value,
    workspace_id: &str,
    tab_id: &str,
) -> Result<TabLayout, HerdrError> {
    let focused_pane_id = nonempty_string(layout.get("focused_pane_id"))
        .ok_or_else(|| HerdrError::LayoutRejected("layout is missing focused_pane_id".into()))?;
    let zoomed = layout
        .get("zoomed")
        .and_then(Value::as_bool)
        .ok_or_else(|| HerdrError::LayoutRejected("layout is missing zoomed".into()))?;
    let area = parse_cell_rect(
        layout
            .get("area")
            .ok_or_else(|| HerdrError::LayoutRejected("layout is missing area".into()))?,
    )?;
    let panes = parse_layout_panes(layout)?;
    if !panes.iter().any(|pane| pane.pane_id == focused_pane_id) {
        return Err(HerdrError::LayoutRejected(
            "focused pane is not in the layout".into(),
        ));
    }
    let root = if zoomed {
        LayoutNode::Pane {
            pane_id: focused_pane_id.clone(),
        }
    } else {
        let splits = parse_layout_splits(layout)?;
        if splits.is_empty() {
            if panes.len() != 1 {
                return Err(HerdrError::LayoutRejected(
                    "a tab without splits must contain one pane".into(),
                ));
            }
            LayoutNode::Pane {
                pane_id: panes[0].pane_id.clone(),
            }
        } else {
            build_split_tree(&panes, &splits, area)?
        }
    };
    let placed = panes
        .iter()
        .map(|pane| PlacedPane {
            pane_id: pane.pane_id.clone(),
            x: pane.rect.x,
            y: pane.rect.y,
            width: pane.rect.width,
            height: pane.rect.height,
        })
        .collect();
    Ok(TabLayout {
        workspace_id: workspace_id.to_string(),
        tab_id: tab_id.to_string(),
        zoomed,
        focused_pane_id,
        root,
        panes: placed,
    })
}

struct RawPane {
    pane_id: String,
    rect: CellRect,
}

struct RawSplit {
    id: String,
    direction: SplitDirection,
    ratio: SplitRatio,
    rect: CellRect,
}

#[derive(Clone, Copy)]
enum ChildRef {
    Pane(usize),
    Split(usize),
}

fn parse_layout_panes(layout: &Value) -> Result<Vec<RawPane>, HerdrError> {
    let entries = layout
        .get("panes")
        .and_then(Value::as_array)
        .ok_or_else(|| HerdrError::LayoutRejected("layout is missing panes".into()))?;
    let mut panes = Vec::with_capacity(entries.len());
    for entry in entries {
        let pane_id = nonempty_string(entry.get("pane_id"))
            .ok_or_else(|| HerdrError::LayoutRejected("pane is missing pane_id".into()))?;
        if panes.iter().any(|pane: &RawPane| pane.pane_id == pane_id) {
            return Err(HerdrError::LayoutRejected(format!(
                "duplicate pane {pane_id}"
            )));
        }
        let rect = parse_cell_rect(entry.get("rect").ok_or_else(|| {
            HerdrError::LayoutRejected(format!("pane {pane_id} is missing rect"))
        })?)?;
        panes.push(RawPane { pane_id, rect });
    }
    Ok(panes)
}

fn parse_layout_splits(layout: &Value) -> Result<Vec<RawSplit>, HerdrError> {
    let entries = layout
        .get("splits")
        .and_then(Value::as_array)
        .ok_or_else(|| HerdrError::LayoutRejected("layout is missing splits".into()))?;
    let mut splits = Vec::with_capacity(entries.len());
    for entry in entries {
        let id = nonempty_string(entry.get("id"))
            .ok_or_else(|| HerdrError::LayoutRejected("split is missing id".into()))?;
        if splits.iter().any(|split: &RawSplit| split.id == id) {
            return Err(HerdrError::LayoutRejected(format!("duplicate split {id}")));
        }
        let direction = match entry.get("direction").and_then(Value::as_str) {
            Some("right") => SplitDirection::Right,
            Some("down") => SplitDirection::Down,
            _ => {
                return Err(HerdrError::LayoutRejected(format!(
                    "split {id} has an unknown direction"
                )))
            }
        };
        let ratio = entry
            .get("ratio")
            .and_then(Value::as_f64)
            .filter(|ratio| ratio.is_finite() && *ratio > 0.0 && *ratio < 1.0)
            .ok_or_else(|| {
                HerdrError::LayoutRejected(format!("split {id} ratio must be between 0 and 1"))
            })?;
        let rect =
            parse_cell_rect(entry.get("rect").ok_or_else(|| {
                HerdrError::LayoutRejected(format!("split {id} is missing rect"))
            })?)?;
        splits.push(RawSplit {
            id,
            direction,
            ratio: SplitRatio(ratio),
            rect,
        });
    }
    Ok(splits)
}

fn parse_cell_rect(value: &Value) -> Result<CellRect, HerdrError> {
    let field = |name: &str| -> Result<u16, HerdrError> {
        let number = value
            .get(name)
            .and_then(Value::as_u64)
            .ok_or_else(|| HerdrError::LayoutRejected(format!("rect {name} is missing")))?;
        u16::try_from(number)
            .map_err(|_| HerdrError::LayoutRejected(format!("rect {name} is out of range")))
    };
    Ok(CellRect {
        x: field("x")?,
        y: field("y")?,
        width: field("width")?,
        height: field("height")?,
    })
}

fn rect_contains(outer: CellRect, inner: CellRect) -> bool {
    let outer_right = u32::from(outer.x) + u32::from(outer.width);
    let outer_bottom = u32::from(outer.y) + u32::from(outer.height);
    let inner_right = u32::from(inner.x) + u32::from(inner.width);
    let inner_bottom = u32::from(inner.y) + u32::from(inner.height);
    inner.x >= outer.x
        && inner.y >= outer.y
        && inner_right <= outer_right
        && inner_bottom <= outer_bottom
}

fn rect_area(rect: CellRect) -> u32 {
    u32::from(rect.width) * u32::from(rect.height)
}

fn smallest_parent(splits: &[RawSplit], rect: CellRect) -> Result<usize, HerdrError> {
    let mut best: Option<(u32, usize, bool)> = None;
    for (index, split) in splits.iter().enumerate() {
        if !rect_contains(split.rect, rect) || split.rect == rect {
            continue;
        }
        let area = rect_area(split.rect);
        best = Some(match best {
            Some((best_area, _, _)) if area < best_area => (area, index, false),
            Some((best_area, best_index, _)) if area == best_area => (best_area, best_index, true),
            Some(kept) => kept,
            None => (area, index, false),
        });
    }
    match best {
        Some((_, _, true)) => Err(HerdrError::LayoutRejected(
            "ambiguous split containment".into(),
        )),
        Some((_, index, false)) => Ok(index),
        None => Err(HerdrError::LayoutRejected(
            "layout node is outside every split".into(),
        )),
    }
}

fn build_split_tree(
    panes: &[RawPane],
    splits: &[RawSplit],
    area: CellRect,
) -> Result<LayoutNode, HerdrError> {
    let mut roots = Vec::new();
    for (index, split) in splits.iter().enumerate() {
        if split.rect == area {
            roots.push(index);
        }
    }
    if roots.len() != 1 {
        return Err(HerdrError::LayoutRejected(
            "layout has no unique root split".into(),
        ));
    }
    let root = roots[0];
    let mut children: std::collections::HashMap<usize, Vec<ChildRef>> =
        std::collections::HashMap::new();
    for (index, pane) in panes.iter().enumerate() {
        let parent = smallest_parent(splits, pane.rect)?;
        children
            .entry(parent)
            .or_default()
            .push(ChildRef::Pane(index));
    }
    for (index, split) in splits.iter().enumerate() {
        if index == root {
            continue;
        }
        let parent = smallest_parent(splits, split.rect)?;
        children
            .entry(parent)
            .or_default()
            .push(ChildRef::Split(index));
    }
    let node = build_split(root, splits, panes, &children)?;
    if node_pane_count(&node) != panes.len() {
        return Err(HerdrError::LayoutRejected(
            "layout does not place every pane".into(),
        ));
    }
    Ok(node)
}

fn build_split(
    index: usize,
    splits: &[RawSplit],
    panes: &[RawPane],
    children: &std::collections::HashMap<usize, Vec<ChildRef>>,
) -> Result<LayoutNode, HerdrError> {
    let split = &splits[index];
    let kids = children.get(&index).map(Vec::as_slice).unwrap_or(&[]);
    if kids.len() != 2 {
        return Err(HerdrError::LayoutRejected(format!(
            "split {} does not have two children",
            split.id
        )));
    }
    let side = |child: ChildRef| -> u16 {
        let rect = match child {
            ChildRef::Pane(index) => panes[index].rect,
            ChildRef::Split(index) => splits[index].rect,
        };
        match split.direction {
            SplitDirection::Right => rect.x,
            SplitDirection::Down => rect.y,
        }
    };
    if side(kids[0]) == side(kids[1]) {
        return Err(HerdrError::LayoutRejected(format!(
            "split {} children do not occupy both sides",
            split.id
        )));
    }
    let (first_ref, second_ref) = if side(kids[0]) < side(kids[1]) {
        (kids[0], kids[1])
    } else {
        (kids[1], kids[0])
    };
    let first = build_child(first_ref, splits, panes, children)?;
    let second = build_child(second_ref, splits, panes, children)?;
    Ok(LayoutNode::Split {
        id: split.id.clone(),
        direction: split.direction,
        ratio: split.ratio,
        first: Box::new(first),
        second: Box::new(second),
    })
}

fn build_child(
    child: ChildRef,
    splits: &[RawSplit],
    panes: &[RawPane],
    children: &std::collections::HashMap<usize, Vec<ChildRef>>,
) -> Result<LayoutNode, HerdrError> {
    match child {
        ChildRef::Pane(index) => Ok(LayoutNode::Pane {
            pane_id: panes[index].pane_id.clone(),
        }),
        ChildRef::Split(index) => build_split(index, splits, panes, children),
    }
}

fn node_pane_count(node: &LayoutNode) -> usize {
    match node {
        LayoutNode::Pane { .. } => 1,
        LayoutNode::Split { first, second, .. } => node_pane_count(first) + node_pane_count(second),
    }
}

fn parse_workspaces(snapshot: &Value) -> Result<Vec<WorkspaceRow>, HerdrError> {
    let entries = snapshot
        .get("workspaces")
        .and_then(Value::as_array)
        .ok_or_else(|| HerdrError::SnapshotParse("missing workspaces".to_string()))?;
    let mut rows = Vec::with_capacity(entries.len());
    for entry in entries {
        let Some(workspace_id) = nonempty_string(entry.get("workspace_id")) else {
            continue;
        };
        let Some(number) = entry.get("number").and_then(Value::as_u64) else {
            continue;
        };
        let label = match nonempty_string(entry.get("label")) {
            Some(label) => label,
            None => workspace_id.clone(),
        };
        let agent_status = entry
            .get("agent_status")
            .and_then(Value::as_str)
            .and_then(AgentStatus::parse)
            .unwrap_or(AgentStatus::Unknown);
        rows.push(WorkspaceRow {
            workspace_id,
            label,
            number,
            agent_status,
            branch: None,
            git_status: None,
            worktree_path: None,
        });
    }
    rows.sort_by(|a, b| {
        a.number
            .cmp(&b.number)
            .then_with(|| a.workspace_id.cmp(&b.workspace_id))
    });
    Ok(rows)
}

struct AgentSort {
    row: AgentRow,
    workspace_number: Option<u64>,
    tab_number: u64,
}

fn parse_agents(
    snapshot: &Value,
    workspaces: &[WorkspaceRow],
) -> Result<Vec<AgentRow>, HerdrError> {
    let Some(value) = snapshot.get("agents") else {
        return Ok(Vec::new());
    };
    let entries = value
        .as_array()
        .ok_or_else(|| HerdrError::SnapshotParse("agents is not an array".to_string()))?;
    let mut rows = Vec::with_capacity(entries.len());
    for entry in entries {
        let Some(pane_id) = nonempty_string(entry.get("pane_id")) else {
            continue;
        };
        let Some(agent_status) = entry
            .get("agent_status")
            .and_then(Value::as_str)
            .and_then(AgentStatus::parse)
        else {
            continue;
        };
        let workspace_id = nonempty_string(entry.get("workspace_id")).unwrap_or_default();
        let (workspace_label, workspace_number) = match workspaces
            .iter()
            .find(|row| row.workspace_id == workspace_id)
        {
            Some(workspace) => (workspace.label.clone(), Some(workspace.number)),
            None => (workspace_id.clone(), None),
        };
        let tab_id = nonempty_string(entry.get("tab_id")).unwrap_or_default();
        let (tab_label, tab_number) = tab_meta(snapshot, &tab_id);
        let identity = nonempty_string(entry.get("name"))
            .or_else(|| nonempty_string(entry.get("display_agent")))
            .or_else(|| nonempty_string(entry.get("agent")))
            .unwrap_or_else(|| pane_id.clone());
        rows.push(AgentSort {
            row: AgentRow {
                pane_id,
                agent_status,
                workspace_id,
                workspace_label,
                tab_label,
                identity,
            },
            workspace_number,
            tab_number,
        });
    }
    rows.sort_by(|a, b| {
        match (a.workspace_number, b.workspace_number) {
            (Some(left), Some(right)) => left.cmp(&right),
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => a.row.workspace_id.cmp(&b.row.workspace_id),
        }
        .then_with(|| a.tab_number.cmp(&b.tab_number))
        .then_with(|| a.row.pane_id.cmp(&b.row.pane_id))
    });
    Ok(rows.into_iter().map(|sorted| sorted.row).collect())
}

fn tab_meta(snapshot: &Value, tab_id: &str) -> (String, u64) {
    if tab_id.is_empty() {
        return (String::new(), u64::MAX);
    }
    let Some(tabs) = snapshot.get("tabs").and_then(Value::as_array) else {
        return (tab_id.to_string(), u64::MAX);
    };
    let Some(tab) = tabs
        .iter()
        .find(|tab| tab.get("tab_id").and_then(Value::as_str) == Some(tab_id))
    else {
        return (tab_id.to_string(), u64::MAX);
    };
    let label = nonempty_string(tab.get("label")).unwrap_or_else(|| tab_id.to_string());
    let number = tab
        .get("number")
        .and_then(Value::as_u64)
        .unwrap_or(u64::MAX);
    (label, number)
}

fn nonempty_string(value: Option<&Value>) -> Option<String> {
    value
        .and_then(Value::as_str)
        .filter(|text| !text.is_empty())
        .map(str::to_string)
}

/// Argv for `herdr workspace focus <workspace_id>`. Four arguments, no shell.
pub fn focus_workspace_argv(workspace_id: &str) -> [String; 4] {
    [
        "herdr".to_string(),
        "workspace".to_string(),
        "focus".to_string(),
        workspace_id.to_string(),
    ]
}

/// Run `herdr workspace focus` without a shell. Stdout is discarded.
pub fn focus_workspace(workspace_id: &str) -> Result<(), HerdrError> {
    run_focus(&focus_workspace_argv(workspace_id))
}

/// Argv for `herdr agent focus <pane_id>`. Four arguments, no shell.
pub fn focus_agent_argv(pane_id: &str) -> [String; 4] {
    [
        "herdr".to_string(),
        "agent".to_string(),
        "focus".to_string(),
        pane_id.to_string(),
    ]
}

/// Run `herdr agent focus` without a shell. Stdout is discarded.
pub fn focus_agent(pane_id: &str) -> Result<(), HerdrError> {
    run_focus(&focus_agent_argv(pane_id))
}

/// Argv for `herdr pane focus --direction <direction> --pane <pane_id>`.
pub fn focus_pane_direction_argv(direction: PaneDirection, pane_id: &str) -> [String; 7] {
    [
        "herdr".to_string(),
        "pane".to_string(),
        "focus".to_string(),
        "--direction".to_string(),
        direction.as_str().to_string(),
        "--pane".to_string(),
        pane_id.to_string(),
    ]
}

/// Run `herdr pane focus --direction` without a shell.
pub fn focus_pane_direction(direction: PaneDirection, pane_id: &str) -> Result<(), HerdrError> {
    run_focus(&focus_pane_direction_argv(direction, pane_id))
}

fn run_focus(argv: &[String]) -> Result<(), HerdrError> {
    let output = Command::new(&argv[0])
        .args(&argv[1..])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .output()
        .map_err(|e| HerdrError::Command(e.to_string()))?;
    if output.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    let stderr = stderr.trim();
    let command = argv.join(" ");
    if stderr.is_empty() {
        Err(HerdrError::Command(format!("{command} failed")))
    } else {
        Err(HerdrError::Command(format!("{command} failed: {stderr}")))
    }
}

/// Schema / version gate tests can call with fixture strings (no live Herdr).
///
/// `version` is the Herdr version string (e.g. `"0.8.0"`).
/// `schema_stdout` is the raw stdout bytes of `herdr api schema --json`.
pub fn check_protocol_gate(version: &str, schema_stdout: &[u8]) -> Result<(), HerdrError> {
    if version != EXPECTED_HERDR_VERSION {
        return Err(HerdrError::ProtocolMismatch);
    }
    let digest = sha256_hex(schema_stdout);
    if !digest.eq_ignore_ascii_case(EXPECTED_SCHEMA_SHA256) {
        return Err(HerdrError::ProtocolMismatch);
    }
    Ok(())
}

pub fn discover_snapshot() -> Result<HerdrSnapshot, HerdrError> {
    ensure_server_running()?;
    let version = read_herdr_version()?;
    let schema = run_herdr_capture(&["api", "schema", "--json"])?;
    check_protocol_gate(&version, &schema)?;
    fetch_snapshot()
}

pub fn discover_focused_pane() -> Result<FocusedPane, HerdrError> {
    discover_snapshot()?
        .focused
        .ok_or(HerdrError::SnapshotMissingFocus)
}

/// `herdr api snapshot`, then each workspace's branch and git status.
/// Timer refreshes do not repeat the protocol gate.
pub fn fetch_snapshot() -> Result<HerdrSnapshot, HerdrError> {
    let snapshot = run_herdr_capture(&["api", "snapshot"])?;
    let text = String::from_utf8(snapshot).map_err(|e| HerdrError::SnapshotParse(e.to_string()))?;
    let mut snap = HerdrSnapshot::from_snapshot_json(&text)?;
    attach_workspace_git(&mut snap.workspaces);
    Ok(snap)
}

/// Join `herdr worktree list` onto workspace rows, then read a short git status
/// from each checkout. A missing list or a non-repo path leaves that half empty.
fn attach_workspace_git(rows: &mut [WorkspaceRow]) {
    if let Ok(bytes) = run_herdr_capture(&["worktree", "list"]) {
        if let Ok(text) = String::from_utf8(bytes) {
            let _ = apply_worktree_list(rows, &text);
        }
    }
    for row in rows {
        let Some(path) = row.worktree_path.clone() else {
            continue;
        };
        row.git_status = git_status_summary(&path);
    }
}

/// Copy `branch` and checkout `path` from a worktree-list JSON document.
///
/// Accepts the CLI envelope (`result.worktrees`) or a bare `worktrees` array.
/// `is_detached` replaces the branch name. Rows with no open worktree are left
/// unchanged.
pub(crate) fn apply_worktree_list(rows: &mut [WorkspaceRow], json: &str) -> Result<(), HerdrError> {
    let value: Value =
        serde_json::from_str(json).map_err(|err| HerdrError::SnapshotParse(err.to_string()))?;
    let worktrees = value
        .pointer("/result/worktrees")
        .or_else(|| value.get("worktrees"))
        .and_then(Value::as_array)
        .ok_or_else(|| HerdrError::SnapshotParse("worktree list is missing worktrees".into()))?;
    for entry in worktrees {
        let Some(workspace_id) = nonempty_string(entry.get("open_workspace_id")) else {
            continue;
        };
        let Some(row) = rows.iter_mut().find(|row| row.workspace_id == workspace_id) else {
            continue;
        };
        let detached = entry
            .get("is_detached")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        row.branch = if detached {
            Some("detached".to_string())
        } else {
            nonempty_string(entry.get("branch"))
        };
        row.worktree_path = nonempty_string(entry.get("path"));
    }
    Ok(())
}

/// Ahead, behind, and a dirty mark from `git status --porcelain=v2 --branch`.
///
/// A clean repository with no ahead/behind counts returns `None`.
pub(crate) fn format_git_porcelain(text: &str) -> Option<String> {
    let mut ahead = 0i32;
    let mut behind = 0i32;
    let mut dirty = false;
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("# branch.ab ") {
            let mut parts = rest.split_whitespace();
            if let Some(value) = parts.next().and_then(|part| part.parse::<i32>().ok()) {
                ahead = value.max(0);
            }
            if let Some(value) = parts.next().and_then(|part| part.parse::<i32>().ok()) {
                behind = value.abs();
            }
            continue;
        }
        if line.starts_with("1 ")
            || line.starts_with("2 ")
            || line.starts_with("u ")
            || line.starts_with("? ")
        {
            dirty = true;
        }
    }
    let mut bits = Vec::new();
    if ahead > 0 {
        bits.push(format!("↑{ahead}"));
    }
    if behind > 0 {
        bits.push(format!("↓{behind}"));
    }
    if dirty {
        bits.push("*".to_string());
    }
    if bits.is_empty() {
        None
    } else {
        Some(bits.join(" "))
    }
}

fn git_status_summary(path: &str) -> Option<String> {
    let output = Command::new("git")
        .args(["-C", path, "status", "--porcelain=v2", "--branch"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8(output.stdout).ok()?;
    format_git_porcelain(&text)
}

fn ensure_server_running() -> Result<(), HerdrError> {
    let output = Command::new("herdr")
        .args(["status", "server"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .map_err(|e| HerdrError::Command(e.to_string()))?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    if stdout.lines().any(|l| l.trim() == "status: running") || stdout.contains("status: running") {
        return Ok(());
    }
    Err(HerdrError::ServerNotRunning)
}

fn read_herdr_version() -> Result<String, HerdrError> {
    let output = Command::new("herdr")
        .arg("--version")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .map_err(|e| HerdrError::Command(e.to_string()))?;
    if !output.status.success() {
        return Err(HerdrError::Command(format!(
            "herdr --version failed: {}",
            String::from_utf8_lossy(&output.stderr)
        )));
    }
    let text = String::from_utf8_lossy(&output.stdout);
    // "herdr 0.8.0"
    let version = text
        .split_whitespace()
        .nth(1)
        .ok_or_else(|| HerdrError::Command("could not parse herdr --version".into()))?
        .trim()
        .to_string();
    Ok(version)
}

fn run_herdr_capture(args: &[&str]) -> Result<Vec<u8>, HerdrError> {
    let output = Command::new("herdr")
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .map_err(|e| HerdrError::Command(e.to_string()))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        if stderr.contains("server_not_running")
            || stderr.contains("Is herdr server running?")
            || stderr.contains("no herdr server is running")
        {
            return Err(HerdrError::ServerNotRunning);
        }
        return Err(HerdrError::Command(format!(
            "herdr {} failed: {stderr}",
            args.join(" ")
        )));
    }
    Ok(output.stdout)
}

fn sha256_hex(data: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    format!("{:x}", Sha256::digest(data))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;

    fn contracts_dir() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("docs/contracts")
    }

    fn snapshot_object(json: &str) -> Value {
        let root: Value = serde_json::from_str(json).unwrap();
        root.pointer("/result/snapshot").unwrap().clone()
    }

    #[test]
    fn focused_tab_layout_ignores_other_workspaces() {
        let json = fs::read_to_string(contracts_dir().join("snapshot.json")).unwrap();
        let snap = HerdrSnapshot::from_snapshot_json(&json).unwrap();
        let layout = snap.layout.expect("focused tab layout");
        assert!(snap.layout_error.is_none());
        assert_eq!(layout.workspace_id, "w9");
        assert_eq!(layout.tab_id, "w9:t1");
        assert!(!layout.zoomed);
        assert_eq!(layout.visible_panes(), ["w9:p1", "w9:p2"]);
    }

    #[test]
    fn w4_t1_maps_to_two_panes_split_by_split_0_root() {
        let json = fs::read_to_string(contracts_dir().join("snapshot.json")).unwrap();
        let layout = tab_layout(&snapshot_object(&json), "w4", "w4:t1").unwrap();
        assert!(!layout.zoomed);
        assert_eq!(layout.focused_pane_id, "w4:p1");
        match layout.root {
            LayoutNode::Split {
                id,
                direction,
                ratio,
                first,
                second,
            } => {
                assert_eq!(id, "split_0_root");
                assert_eq!(direction, SplitDirection::Right);
                assert!((ratio.get() - 0.48795182).abs() < 1e-8);
                assert!(
                    matches!(first.as_ref(), LayoutNode::Pane { pane_id } if pane_id == "w4:p1")
                );
                assert!(
                    matches!(second.as_ref(), LayoutNode::Pane { pane_id } if pane_id == "w4:p2")
                );
            }
            other => panic!("expected a root split, got {other:?}"),
        }
    }

    #[test]
    fn w1_t1_maps_to_one_pane() {
        let json = fs::read_to_string(contracts_dir().join("snapshot.json")).unwrap();
        let layout = tab_layout(&snapshot_object(&json), "w1", "w1:t1").unwrap();
        assert!(!layout.zoomed);
        assert!(layout.visible_panes() == ["w1:p1"]);
        assert!(matches!(layout.root, LayoutNode::Pane { pane_id } if pane_id == "w1:p1"));
    }

    #[test]
    fn nested_fixture_reconstructs_parent_and_child_splits() {
        let json = fs::read_to_string(contracts_dir().join("fixtures/layout-nested.json")).unwrap();
        let layout = tab_layout(&snapshot_object(&json), "w4", "w4:t1").unwrap();
        assert_eq!(layout.visible_panes(), ["w4:p1", "w4:p2", "w4:p3"]);
        match layout.root {
            LayoutNode::Split {
                id,
                direction,
                first,
                second,
                ..
            } => {
                assert_eq!(id, "split_0_root");
                assert_eq!(direction, SplitDirection::Right);
                assert!(
                    matches!(first.as_ref(), LayoutNode::Pane { pane_id } if pane_id == "w4:p1")
                );
                match second.as_ref() {
                    LayoutNode::Split {
                        id,
                        direction,
                        first,
                        second,
                        ..
                    } => {
                        assert_eq!(id, "split_1");
                        assert_eq!(*direction, SplitDirection::Down);
                        assert!(
                            matches!(first.as_ref(), LayoutNode::Pane { pane_id } if pane_id == "w4:p2")
                        );
                        assert!(
                            matches!(second.as_ref(), LayoutNode::Pane { pane_id } if pane_id == "w4:p3")
                        );
                    }
                    other => panic!("expected nested split, got {other:?}"),
                }
            }
            other => panic!("expected root split, got {other:?}"),
        }
    }

    #[test]
    fn zoomed_fixture_maps_to_the_focused_pane_only() {
        let json = fs::read_to_string(contracts_dir().join("fixtures/layout-zoomed.json")).unwrap();
        let layout = tab_layout(&snapshot_object(&json), "w4", "w4:t1").unwrap();
        assert!(layout.zoomed);
        assert_eq!(layout.focused_pane_id, "w4:p1");
        assert_eq!(layout.visible_panes(), ["w4:p1"]);
        assert!(matches!(layout.root, LayoutNode::Pane { pane_id } if pane_id == "w4:p1"));
    }

    #[test]
    fn layout_without_a_unique_root_is_rejected() {
        let json = r#"{
            "area": {"x": 0, "y": 0, "width": 10, "height": 10},
            "focused_pane_id": "p1",
            "zoomed": false,
            "panes": [
                {"pane_id": "p1", "focused": true, "rect": {"x": 0, "y": 0, "width": 10, "height": 10}}
            ],
            "splits": [
                {"id": "a", "direction": "right", "ratio": 0.5, "rect": {"x": 0, "y": 0, "width": 9, "height": 10}},
                {"id": "b", "direction": "right", "ratio": 0.5, "rect": {"x": 0, "y": 0, "width": 8, "height": 10}}
            ],
            "workspace_id": "w",
            "tab_id": "t"
        }"#;
        let err = reconstruct_layout(&serde_json::from_str(json).unwrap(), "w", "t").unwrap_err();
        assert!(err.to_string().contains("no unique root"), "{err}");
    }

    #[test]
    fn parses_focused_pane_from_snapshot_fixture() {
        let json = fs::read_to_string(contracts_dir().join("snapshot.json")).unwrap();
        let pane = FocusedPane::from_snapshot_json(&json).unwrap();
        assert_eq!(pane.pane_id, "w9:p2");
        assert_eq!(pane.workspace_id, "w9");
        assert_eq!(pane.tab_id, "w9:t1");
        // Labels / title may be present or absent; fixture has redacted values.
        assert_eq!(pane.workspace_label.as_deref(), Some("REDACTED"));
        assert_eq!(pane.tab_label.as_deref(), Some("1"));
        assert_eq!(pane.terminal_title.as_deref(), Some("REDACTED"));
    }

    #[test]
    fn parses_workspaces_from_snapshot_fixture_in_number_order() {
        let json = fs::read_to_string(contracts_dir().join("snapshot.json")).unwrap();
        let snap = HerdrSnapshot::from_snapshot_json(&json).unwrap();
        assert_eq!(snap.workspaces.len(), 5);
        let numbers: Vec<u64> = snap.workspaces.iter().map(|row| row.number).collect();
        assert_eq!(numbers, vec![1, 2, 3, 4, 5]);
        let ids: Vec<&str> = snap
            .workspaces
            .iter()
            .map(|row| row.workspace_id.as_str())
            .collect();
        assert_eq!(ids, ["w1", "w4", "w5", "w8", "w9"]);
        assert_eq!(snap.focused_workspace_id.as_deref(), Some("w9"));
        assert!(snap.workspaces.iter().all(|row| row.label == "REDACTED"));
        assert!(snap
            .workspaces
            .iter()
            .all(|row| !row.label.contains("idle") && !row.label.contains("unknown")));
        assert_eq!(
            snap.focused.as_ref().map(|pane| pane.pane_id.as_str()),
            Some("w9:p2")
        );
    }

    #[test]
    fn workspace_rows_sort_by_number_and_missing_label_uses_id() {
        let json = r#"{
            "result": {"snapshot": {
                "focused_workspace_id": "b",
                "workspaces": [
                    {"workspace_id": "b", "number": 2, "label": null},
                    {"workspace_id": "a", "number": 1, "label": ""}
                ]
            }}
        }"#;
        let snap = HerdrSnapshot::from_snapshot_json(json).unwrap();
        assert_eq!(snap.workspaces[0].workspace_id, "a");
        assert_eq!(snap.workspaces[0].label, "a");
        assert_eq!(snap.workspaces[1].workspace_id, "b");
        assert_eq!(snap.workspaces[1].label, "b");
        assert_eq!(snap.focused_workspace_id.as_deref(), Some("b"));
        assert!(snap.focused.is_none());
    }

    #[test]
    fn click_direction_follows_pane_centers() {
        let nested =
            fs::read_to_string(contracts_dir().join("fixtures/layout-nested.json")).unwrap();
        let layout = tab_layout(&snapshot_object(&nested), "w4", "w4:t1").unwrap();
        assert_eq!(
            layout.direction_toward("w4:p1", "w4:p2"),
            Some(PaneDirection::Right)
        );
        assert_eq!(
            layout.direction_toward("w4:p2", "w4:p3"),
            Some(PaneDirection::Down)
        );
        assert_eq!(
            layout.direction_toward("w4:p3", "w4:p2"),
            Some(PaneDirection::Up)
        );
        assert_eq!(
            layout.direction_toward("w4:p2", "w4:p1"),
            Some(PaneDirection::Left)
        );
        assert_eq!(layout.direction_toward("w4:p1", "w4:p1"), None);

        let json = fs::read_to_string(contracts_dir().join("snapshot.json")).unwrap();
        let w4 = tab_layout(&snapshot_object(&json), "w4", "w4:t1").unwrap();
        assert_eq!(
            w4.direction_toward("w4:p1", "w4:p2"),
            Some(PaneDirection::Right)
        );
    }

    #[test]
    fn focus_pane_direction_argv_names_the_neighbor_step() {
        let argv = focus_pane_direction_argv(PaneDirection::Right, "w4:p1");
        assert_eq!(
            argv,
            [
                "herdr".to_string(),
                "pane".to_string(),
                "focus".to_string(),
                "--direction".to_string(),
                "right".to_string(),
                "--pane".to_string(),
                "w4:p1".to_string(),
            ]
        );
    }

    #[test]
    fn focus_workspace_argv_is_four_arguments_without_a_shell() {
        let argv = focus_workspace_argv("w4");
        assert_eq!(
            argv,
            [
                "herdr".to_string(),
                "workspace".to_string(),
                "focus".to_string(),
                "w4".to_string(),
            ]
        );
        assert!(argv
            .iter()
            .all(|arg| arg != "sh" && arg != "-c" && arg != "bash"));
    }

    #[test]
    fn parses_agents_from_snapshot_fixture_in_workspace_order() {
        let json = fs::read_to_string(contracts_dir().join("snapshot.json")).unwrap();
        let snap = HerdrSnapshot::from_snapshot_json(&json).unwrap();
        let panes: Vec<&str> = snap.agents.iter().map(|row| row.pane_id.as_str()).collect();
        assert_eq!(panes, ["w4:p1", "w5:p1", "w8:p1", "w9:p2"]);
        assert!(snap.agents.iter().all(|row| row.identity == "opencode"));
        assert!(snap
            .agents
            .iter()
            .all(|row| row.agent_status == AgentStatus::Idle));
        assert!(snap.agents.iter().all(|row| row.tab_label == "1"));
        assert!(snap
            .agents
            .iter()
            .all(|row| row.workspace_label == "REDACTED"));
        assert!(snap.agents.iter().all(|row| {
            !row.identity.contains('/')
                && row.identity != "REDACTED"
                && !row.tab_label.contains('/')
        }));
        assert_eq!(
            snap.focused.as_ref().map(|pane| pane.pane_id.as_str()),
            Some("w9:p2")
        );
    }

    #[test]
    fn agent_rows_sort_by_workspace_then_tab_and_fall_back_identity() {
        let json = r#"{
            "result": {"snapshot": {
                "workspaces": [
                    {"workspace_id": "b", "number": 2, "label": "beta"},
                    {"workspace_id": "a", "number": 1, "label": ""}
                ],
                "tabs": [
                    {"tab_id": "a:t2", "number": 2, "label": "second"},
                    {"tab_id": "a:t1", "number": 1, "label": null},
                    {"tab_id": "b:t1", "number": 1, "label": "only"}
                ],
                "agents": [
                    {"pane_id": "b:p1", "agent_status": "working", "workspace_id": "b", "tab_id": "b:t1", "agent": "codex", "name": "named", "cwd": "/tmp/secret", "terminal_title": "title"},
                    {"pane_id": "a:p2", "agent_status": "idle", "workspace_id": "a", "tab_id": "a:t2", "agent": "claude"},
                    {"pane_id": "z:p1", "agent_status": "blocked", "workspace_id": "missing", "tab_id": "missing:t1", "display_agent": "shown"},
                    {"pane_id": "a:p1", "agent_status": "done", "workspace_id": "a", "tab_id": "a:t1"},
                    {"pane_id": "skip", "workspace_id": "a"},
                    {"agent_status": "idle", "workspace_id": "a", "agent": "no-pane"},
                    {"pane_id": "bad", "agent_status": "asleep", "workspace_id": "a", "agent": "nope"}
                ]
            }}
        }"#;
        let snap = HerdrSnapshot::from_snapshot_json(json).unwrap();
        let panes: Vec<&str> = snap.agents.iter().map(|row| row.pane_id.as_str()).collect();
        assert_eq!(panes, ["a:p1", "a:p2", "b:p1", "z:p1"]);
        assert_eq!(snap.agents[0].identity, "a:p1");
        assert_eq!(snap.agents[0].workspace_label, "a");
        assert_eq!(snap.agents[0].tab_label, "a:t1");
        assert_eq!(snap.agents[0].agent_status, AgentStatus::Done);
        assert_eq!(snap.agents[1].identity, "claude");
        assert_eq!(snap.agents[1].tab_label, "second");
        assert_eq!(snap.agents[2].identity, "named");
        assert_eq!(snap.agents[2].workspace_label, "beta");
        assert_eq!(snap.agents[3].identity, "shown");
        assert_eq!(snap.agents[3].workspace_label, "missing");
        assert_eq!(snap.agents[3].tab_label, "missing:t1");
        assert!(snap.agents.iter().all(|row| row.identity != "title"));
        assert!(snap.agents.iter().all(|row| !row.identity.contains("/tmp")));
    }

    #[test]
    fn workspace_rows_keep_status_and_a_joined_branch() {
        let json = fs::read_to_string(contracts_dir().join("snapshot.json")).unwrap();
        let mut snap = HerdrSnapshot::from_snapshot_json(&json).unwrap();
        let status = |id: &str| {
            snap.workspaces
                .iter()
                .find(|row| row.workspace_id == id)
                .map(|row| row.agent_status)
        };
        assert_eq!(status("w1"), Some(AgentStatus::Unknown));
        assert_eq!(status("w4"), Some(AgentStatus::Idle));
        assert_eq!(status("w9"), Some(AgentStatus::Idle));
        assert!(snap.workspaces.iter().all(|row| row.branch.is_none()));

        apply_worktree_list(
            &mut snap.workspaces,
            r#"{"result":{"worktrees":[
                {"branch":"dev","is_detached":false,"open_workspace_id":"w9","path":"/tmp/repo"},
                {"branch":"other","is_detached":true,"open_workspace_id":"w4","path":"/tmp/other"}
            ]}}"#,
        )
        .unwrap();
        let w9 = snap
            .workspaces
            .iter()
            .find(|row| row.workspace_id == "w9")
            .unwrap();
        assert_eq!(w9.branch.as_deref(), Some("dev"));
        assert_eq!(w9.worktree_path.as_deref(), Some("/tmp/repo"));
        let w4 = snap
            .workspaces
            .iter()
            .find(|row| row.workspace_id == "w4")
            .unwrap();
        assert_eq!(w4.branch.as_deref(), Some("detached"));
        assert!(snap
            .workspaces
            .iter()
            .filter(|row| row.workspace_id != "w9" && row.workspace_id != "w4")
            .all(|row| row.branch.is_none()));

        assert_eq!(
            format_git_porcelain("# branch.ab +2 -1\n1 .M N... 100644 100644 0 0 file\n"),
            Some("↑2 ↓1 *".to_string())
        );
        assert_eq!(format_git_porcelain("# branch.ab +0 -0\n"), None);
        assert_eq!(
            format_git_porcelain("# branch.head main\n? untracked\n"),
            Some("*".to_string())
        );
    }

    #[test]
    fn focus_agent_argv_is_four_arguments_without_a_shell() {
        let argv = focus_agent_argv("w9:p2");
        assert_eq!(
            argv,
            [
                "herdr".to_string(),
                "agent".to_string(),
                "focus".to_string(),
                "w9:p2".to_string(),
            ]
        );
        assert!(argv
            .iter()
            .all(|arg| arg != "sh" && arg != "-c" && arg != "bash"));
    }

    #[test]
    fn protocol_gate_accepts_expected_version_and_schema_hash() {
        let schema = fs::read(contracts_dir().join("herdr-api.schema.json")).unwrap();
        check_protocol_gate(EXPECTED_HERDR_VERSION, &schema).unwrap();
    }

    #[test]
    fn protocol_gate_rejects_version_mismatch_mentioning_rediscovery() {
        let err = check_protocol_gate("0.0.0", b"{}").unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("rerun discovery") || msg.contains("rediscovery"),
            "error should mention rediscovery, got: {msg}"
        );
    }

    #[test]
    fn protocol_gate_rejects_schema_hash_mismatch() {
        let err = check_protocol_gate(EXPECTED_HERDR_VERSION, b"not-the-schema").unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("rerun discovery") || msg.contains("rediscovery"),
            "error should mention rediscovery, got: {msg}"
        );
    }

    #[test]
    fn fixtures_are_redacted_of_home_and_username() {
        let root = contracts_dir();
        let mut paths = vec![root.join("snapshot.json"), root.join("observe.ndjson")];
        let fixtures = root.join("fixtures");
        for entry in fs::read_dir(&fixtures).unwrap() {
            paths.push(entry.unwrap().path());
        }
        for path in paths {
            let text = fs::read_to_string(&path).unwrap_or_default();
            assert!(
                !text.contains("/home/lgal"),
                "{} contains /home/lgal",
                path.display()
            );
            // Exact username token — avoid matching REDACTED-style placeholders.
            for token in text.split(|c: char| !c.is_ascii_alphanumeric() && c != '_') {
                assert_ne!(token, "lgal", "{} contains username lgal", path.display());
            }
        }
    }

    #[test]
    fn server_not_running_message_mentions_herdr_server() {
        let msg = HerdrError::ServerNotRunning.to_string();
        assert!(msg.contains("herdr server"), "{msg}");
    }

    #[test]
    fn sha256_matches_known_empty() {
        // SHA-256 of empty string
        assert_eq!(
            sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }
}
