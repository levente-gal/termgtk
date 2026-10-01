//! Layout tree. A frame is parsed into panels, then merged into the previous tree.
//! Paint reads that tree. Debug builds print grid/previous/new/merged diagnostics on changes.

use std::collections::HashSet;
use std::fmt;

use crate::terminal::{Cell, CellOccupancy, Color, Style, TerminalSnapshot};

const H: &str = "─";
const V: &str = "│";
const MAX_RECTS: usize = 256;
const MIN_EDGE_CELLS: u16 = 3;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CornerFamily {
    Square,
    Rounded,
}

impl CornerFamily {
    fn corners(self) -> (&'static str, &'static str, &'static str, &'static str) {
        match self {
            Self::Square => ("┌", "┐", "└", "┘"),
            Self::Rounded => ("╭", "╮", "╰", "╯"),
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct Node {
    id: u64,
    x: u16,
    y: u16,
    cols: u16,
    rows: u16,
    title: Option<String>,
    index: Option<u16>,
    scroll: Option<(u32, u32)>,
    /// Right-edge TUI thumb, when the painted slider does not cover the whole side.
    thumb: Option<Thumb>,
    border: Color,
    body: PaneBody,
    children: Vec<Node>,
}

/// Inclusive cell run of a pane's right-edge scrollbar.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct Thumb {
    pub start: u16,
    pub end: u16,
    pub track_top: u16,
    pub track_bottom: u16,
}

#[allow(dead_code)]
impl Thumb {
    pub(crate) fn track_len(self) -> u32 {
        u32::from(self.track_bottom.saturating_sub(self.track_top))
            .saturating_add(1)
            .max(1)
    }

    pub(crate) fn thumb_len(self) -> u32 {
        u32::from(self.end.saturating_sub(self.start))
            .saturating_add(1)
            .max(1)
            .min(self.track_len())
    }

    pub(crate) fn offset(self) -> u32 {
        u32::from(self.start.saturating_sub(self.track_top))
            .min(self.track_len().saturating_sub(self.thumb_len()))
    }

    /// 0 at the top of the track, 1 at the bottom.
    pub(crate) fn travel_t(self) -> f64 {
        let travel = self.track_len().saturating_sub(self.thumb_len());
        if travel == 0 {
            0.0
        } else {
            f64::from(self.offset()) / f64::from(travel)
        }
    }
}

/// One styled run of text copied out of a pane interior.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct Span {
    pub text: String,
    pub fg: Color,
    pub bg: Color,
    pub style: Style,
}

/// One row of pane text. `selected` is set when the row is reverse video.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct ContentLine {
    pub spans: Vec<Span>,
    pub selected: bool,
}

/// Interior of a pane, classified for the widget layer.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) enum PaneBody {
    Empty,
    Rows(Vec<ContentLine>),
    Text(Vec<ContentLine>),
}

/// A row that is not inside any pane, such as the status line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ChromeRow {
    pub y: u16,
    pub line: ContentLine,
}

/// Retained layout. `layout` is the tiled panes. `overlays` are dialogs above them.
#[derive(Clone, Debug)]
pub struct LayoutTree {
    cols: u16,
    rows: u16,
    layout: Vec<Node>,
    overlays: Vec<Node>,
    chrome: Vec<ChromeRow>,
    suppress: HashSet<(u16, u16)>,
    // Tracking state never paints. Layout history survives only while a
    // confirmed overlay explains why underlying borders are absent.
    history_layout: Vec<Node>,
    history_overlays: Vec<Node>,
    next_id: u64,
    log_signature: String,
}

impl LayoutTree {
    pub fn node_count(&self) -> u64 {
        count_nodes(&self.layout) + count_nodes(&self.overlays)
    }

    pub fn suppress(&self) -> &HashSet<(u16, u16)> {
        &self.suppress
    }

    pub(crate) fn layout(&self) -> &[Node] {
        &self.layout
    }

    pub(crate) fn overlays(&self) -> &[Node] {
        &self.overlays
    }

    pub(crate) fn chrome(&self) -> &[ChromeRow] {
        &self.chrome
    }

    pub(crate) fn screen(&self) -> (u16, u16) {
        (self.cols, self.rows)
    }
}

impl fmt::Display for LayoutTree {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "Screen {}×{}", self.cols, self.rows)?;
        writeln!(f, "├── Layout")?;
        write_nodes(f, &self.layout, "│   ", "Panel")?;
        writeln!(f, "└── Overlay")?;
        write_nodes(f, &self.overlays, "    ", "Dialog")?;
        Ok(())
    }
}

/// Parse one frame into a tree. Every closed box is a layout node.
/// Dialogs are decided when this tree is merged with the previous one.
pub fn parse(snapshot: &TerminalSnapshot) -> LayoutTree {
    let mut nodes: Vec<Node> = find_rects(snapshot)
        .into_iter()
        .enumerate()
        .map(|(index, rect)| rect_node(snapshot, rect, index as u64 + 1))
        .collect();
    nodes.sort_by_key(|node| (node.y, node.x, node.cols, node.rows));
    let next_id = nodes.len() as u64 + 1;
    let mut layout = nest(nodes);
    fill_bodies(snapshot, &mut layout, &[]);
    let history_layout = flatten(&layout);
    let mut tree = LayoutTree {
        cols: snapshot.cols(),
        rows: snapshot.rows(),
        layout,
        overlays: Vec::new(),
        chrome: Vec::new(),
        suppress: HashSet::new(),
        history_layout,
        history_overlays: Vec::new(),
        next_id,
        log_signature: String::new(),
    };
    tree.chrome = chrome_rows(snapshot, &tree);
    tree.suppress = suppress_for(snapshot, &tree);
    tree
}

/// Parse `snapshot` and merge it into `retained`. Debug builds print grid plus
/// previous/new/merged trees on structural changes. `HERDR_OVERLAY_LOG=0|1` overrides.
pub fn reconcile(snapshot: &TerminalSnapshot, retained: &mut Option<LayoutTree>) -> LayoutTree {
    if retained
        .as_ref()
        .is_some_and(|tree| tree.cols != snapshot.cols() || tree.rows != snapshot.rows())
    {
        *retained = None;
    }
    let log = overlay_log_enabled();
    let previous_text = if log {
        retained
            .as_ref()
            .map(ToString::to_string)
            .unwrap_or_default()
    } else {
        String::new()
    };
    let fresh = parse(snapshot);
    let fresh_text = if log {
        fresh.to_string()
    } else {
        String::new()
    };
    let mut merged = match retained.as_ref() {
        Some(prev) => merge(prev, &fresh),
        None => classify_initial(fresh),
    };
    merged.chrome = chrome_rows(snapshot, &merged);
    merged.suppress = suppress_for(snapshot, &merged);
    if log {
        let merged_text = merged.to_string();
        let signature = format!("{previous_text}{fresh_text}{merged_text}");
        if retained.as_ref().map(|tree| tree.log_signature.as_str()) != Some(signature.as_str()) {
            eprint!(
                "\n{}",
                frame_log(snapshot, &previous_text, &fresh_text, &merged_text)
            );
        }
        merged.log_signature = signature;
    } else {
        merged.log_signature.clear();
    }
    *retained = Some(merged.clone());
    merged
}

fn classify_initial(mut tree: LayoutTree) -> LayoutTree {
    let nodes = flatten(&tree.layout);
    let (overlays, layout): (Vec<_>, Vec<_>) = nodes
        .into_iter()
        .partition(|node| is_floating_dialog(node, tree.cols, tree.rows));
    if overlays.is_empty() {
        return tree;
    }
    tree.layout = nest(layout.clone());
    tree.overlays = nest(overlays.clone());
    tree.history_layout = layout;
    tree.history_overlays = overlays;
    tree.chrome.clear();
    tree.suppress.clear();
    tree
}

fn overlay_log_enabled() -> bool {
    match std::env::var("HERDR_OVERLAY_LOG").ok().as_deref() {
        Some("1") => true,
        Some("0") => false,
        _ => cfg!(debug_assertions),
    }
}

fn frame_log(snapshot: &TerminalSnapshot, previous: &str, fresh: &str, merged: &str) -> String {
    let mut out = format!("terminal {}×{}\n", snapshot.cols(), snapshot.rows());
    write_grid(&mut out, snapshot);
    out.push_str("previous\n");
    out.push_str(previous);
    out.push_str("new\n");
    out.push_str(fresh);
    out.push_str("merged\n");
    out.push_str(merged);
    out
}

fn write_grid(out: &mut String, snapshot: &TerminalSnapshot) {
    let cols = snapshot.cols();
    let prefix = "    |";
    let mut ruler = String::from(prefix);
    let mut ones = String::from(prefix);
    let mut x = 0u16;
    while x < cols {
        if x % 10 == 0 {
            let label = x.to_string();
            let width = u16::try_from(label.len()).unwrap_or(1);
            if x + width > cols {
                ruler.push_str(&label[..usize::from(cols - x)]);
                x = cols;
            } else {
                ruler.push_str(&label);
                x += width;
            }
        } else {
            ruler.push(' ');
            x += 1;
        }
    }
    for col in 0..cols {
        ones.push(char::from_digit(u32::from(col % 10), 10).unwrap_or(' '));
    }
    out.push_str(&ruler);
    out.push('\n');
    out.push_str(&ones);
    out.push('\n');
    for y in 0..snapshot.rows() {
        out.push_str(&format!("{y:>4}|"));
        for x in 0..cols {
            out.push_str(&cell_text(snapshot, x, y));
        }
        out.push('\n');
    }
}

fn cell_text(snapshot: &TerminalSnapshot, x: u16, y: u16) -> String {
    let Some(cell) = snapshot.cell(x, y) else {
        return " ".to_string();
    };
    if cell.occupancy == CellOccupancy::WideContinuation
        || cell.grapheme.is_empty()
        || cell.grapheme.chars().any(char::is_control)
    {
        return " ".to_string();
    }
    cell.grapheme.clone()
}

fn merge(prev: &LayoutTree, fresh: &LayoutTree) -> LayoutTree {
    let mut next_id = prev.next_id;
    let prev_layout = if prev.history_layout.is_empty() {
        flatten(&prev.layout)
    } else {
        prev.history_layout.clone()
    };
    let prev_overlay = if prev.history_overlays.is_empty() {
        flatten(&prev.overlays)
    } else {
        prev.history_overlays.clone()
    };
    let fresh_nodes = {
        let mut nodes = flatten(&fresh.layout);
        nodes.extend(flatten(&fresh.overlays));
        nodes
    };

    let overlay_mask: Vec<bool> = fresh_nodes
        .iter()
        .map(|node| {
            let pane_score = best_score(node, &prev_layout, pane_match_score);
            let overlay_score = best_score(node, &prev_overlay, overlay_match_score);
            node.index.is_none()
                && (is_floating_dialog(node, fresh.cols, fresh.rows)
                    || (pane_score == 0 && is_dialog(node, &prev_layout))
                    || (overlay_score > 0 && overlay_score > pane_score))
        })
        .collect();

    let layout_fresh: Vec<Node> = fresh_nodes
        .iter()
        .zip(&overlay_mask)
        .filter_map(|(node, overlay)| (!overlay).then_some(node.clone()))
        .collect();
    let overlay_fresh: Vec<Node> = fresh_nodes
        .iter()
        .zip(&overlay_mask)
        .filter_map(|(node, overlay)| overlay.then_some(node.clone()))
        .collect();

    // A numbered pane can recover from an earlier bad layer classification.
    let mut layout_history = prev_layout.clone();
    layout_history.extend(
        prev_overlay
            .iter()
            .filter(|node| node.index.is_some())
            .cloned(),
    );

    let layout_matches = assign_global(&layout_fresh, &layout_history, pane_match_score);
    let overlay_matches = assign_global(&overlay_fresh, &prev_overlay, overlay_match_score);
    let mut layout = assign_ids(layout_fresh, &layout_history, layout_matches, &mut next_id);
    let mut overlays = assign_ids(overlay_fresh, &prev_overlay, overlay_matches, &mut next_id);

    if !overlays.is_empty() {
        for node in &mut layout {
            if overlays.iter().any(|overlay| iou(node, overlay) > 0.0) {
                if let Some(previous) = prev_layout.iter().find(|previous| previous.id == node.id) {
                    node.body = previous.body.clone();
                }
            }
        }
    }

    let mut history_layout = layout.clone();
    if !overlays.is_empty() {
        for node in &prev_layout {
            if overlays.iter().any(|overlay| iou(node, overlay) > 0.0)
                && !is_floating_dialog(node, fresh.cols, fresh.rows)
                && !history_layout.iter().any(|tracked| tracked.id == node.id)
            {
                history_layout.push(node.clone());
            }
        }
        history_layout.truncate(MAX_RECTS);
        layout = history_layout.clone();
    }
    let history_overlays = overlays.clone();

    layout.sort_by_key(|node| (node.y, node.x, node.id));
    overlays.sort_by_key(|node| (node.y, node.x, node.id));
    LayoutTree {
        cols: fresh.cols,
        rows: fresh.rows,
        layout: nest(layout),
        overlays: nest(overlays),
        chrome: Vec::new(),
        suppress: HashSet::new(),
        history_layout,
        history_overlays,
        next_id,
        log_signature: String::new(),
    }
}

fn assign_ids(
    fresh: Vec<Node>,
    previous: &[Node],
    matches: Vec<Option<usize>>,
    next_id: &mut u64,
) -> Vec<Node> {
    fresh
        .into_iter()
        .zip(matches)
        .map(|(mut node, previous_index)| {
            node.children.clear();
            node.id = previous_index
                .map(|index| previous[index].id)
                .unwrap_or_else(|| take_id(next_id));
            node
        })
        .collect()
}

fn is_dialog(node: &Node, panes: &[Node]) -> bool {
    if node.index.is_some()
        || panes.is_empty()
        || panes.iter().any(|pane| strictly_contains(node, pane))
    {
        return false;
    }
    panes.iter().any(|pane| interiors_overlap(pane, node))
}

fn is_floating_dialog(node: &Node, cols: u16, rows: u16) -> bool {
    node.index.is_none()
        && node.x > 0
        && node.y > 0
        && node.x.saturating_add(node.cols) < cols
        && node.y.saturating_add(node.rows) < rows
}

fn take_id(next_id: &mut u64) -> u64 {
    let id = *next_id;
    *next_id = next_id.saturating_add(1);
    id
}

type MatchScore = fn(&Node, &Node) -> i64;

fn best_score(node: &Node, previous: &[Node], score: MatchScore) -> i64 {
    previous
        .iter()
        .map(|candidate| score(node, candidate))
        .max()
        .unwrap_or(0)
}

fn pane_match_score(fresh: &Node, previous: &Node) -> i64 {
    if matches!((fresh.index, previous.index), (Some(a), Some(b)) if a != b) {
        return 0;
    }
    let overlap = i64::from((iou(fresh, previous) * 1000.0).round() as i32);
    let same_index = fresh.index.is_some() && fresh.index == previous.index;
    let same_title = titles_match(fresh, previous);
    let shared_edge = shares_edge(fresh, previous);
    if !same_index && overlap == 0 && !shared_edge {
        return 0;
    }
    if !same_index
        && !same_title
        && !shared_edge
        && (strictly_contains(fresh, previous) || strictly_contains(previous, fresh))
    {
        return 0;
    }
    if !same_index && !same_title && overlap < 500 && !shared_edge {
        return 0;
    }
    let mut score = overlap.saturating_mul(10);
    if same_index {
        score = score.saturating_add(100_000);
    }
    if same_title {
        score = score.saturating_add(20_000);
    }
    if shared_edge {
        score = score.saturating_add(5_000);
    }
    score.saturating_add(size_similarity(fresh, previous))
}

fn overlay_match_score(fresh: &Node, previous: &Node) -> i64 {
    let overlap = i64::from((iou(fresh, previous) * 1000.0).round() as i32);
    let same_title = titles_match(fresh, previous);
    if overlap < 250 && !same_title {
        return 0;
    }
    overlap
        .saturating_mul(10)
        .saturating_add(if same_title { 20_000 } else { 0 })
        .saturating_add(size_similarity(fresh, previous))
}

fn titles_match(a: &Node, b: &Node) -> bool {
    match (&a.title, &b.title) {
        (Some(a), Some(b)) => a.trim().eq_ignore_ascii_case(b.trim()),
        _ => false,
    }
}

fn size_similarity(a: &Node, b: &Node) -> i64 {
    let a_area = u64::from(area(a));
    let b_area = u64::from(area(b));
    let larger = a_area.max(b_area);
    if larger == 0 {
        return 0;
    }
    i64::try_from(a_area.min(b_area).saturating_mul(1000) / larger).unwrap_or(1000)
}

/// Deterministic maximum-weight one-to-one assignment. Zero means no valid match.
fn assign_global(fresh: &[Node], previous: &[Node], score: MatchScore) -> Vec<Option<usize>> {
    if fresh.is_empty() || previous.is_empty() {
        return vec![None; fresh.len()];
    }
    let size = fresh.len().max(previous.len());
    let scores: Vec<Vec<i64>> = (0..size)
        .map(|row| {
            (0..size)
                .map(|col| {
                    if row < fresh.len() && col < previous.len() {
                        score(&fresh[row], &previous[col])
                    } else {
                        0
                    }
                })
                .collect()
        })
        .collect();
    let max_score = scores.iter().flatten().copied().max().unwrap_or(0);
    let mut u = vec![0i64; size + 1];
    let mut v = vec![0i64; size + 1];
    let mut p = vec![0usize; size + 1];
    let mut way = vec![0usize; size + 1];
    for row in 1..=size {
        p[0] = row;
        let mut col0 = 0usize;
        let mut minv = vec![i64::MAX; size + 1];
        let mut used = vec![false; size + 1];
        loop {
            used[col0] = true;
            let row0 = p[col0];
            let mut delta = i64::MAX;
            let mut col1 = 0usize;
            for col in 1..=size {
                if used[col] {
                    continue;
                }
                let cost = max_score
                    .saturating_sub(scores[row0 - 1][col - 1])
                    .saturating_sub(u[row0])
                    .saturating_sub(v[col]);
                if cost < minv[col] {
                    minv[col] = cost;
                    way[col] = col0;
                }
                if minv[col] < delta {
                    delta = minv[col];
                    col1 = col;
                }
            }
            for col in 0..=size {
                if used[col] {
                    u[p[col]] = u[p[col]].saturating_add(delta);
                    v[col] = v[col].saturating_sub(delta);
                } else {
                    minv[col] = minv[col].saturating_sub(delta);
                }
            }
            col0 = col1;
            if p[col0] == 0 {
                break;
            }
        }
        loop {
            let col1 = way[col0];
            p[col0] = p[col1];
            col0 = col1;
            if col0 == 0 {
                break;
            }
        }
    }

    let mut result = vec![None; fresh.len()];
    for col in 1..=size {
        let row = p[col];
        if row == 0 || row > fresh.len() || col > previous.len() {
            continue;
        }
        if scores[row - 1][col - 1] > 0 {
            result[row - 1] = Some(col - 1);
        }
    }
    result
}

fn shares_edge(a: &Node, b: &Node) -> bool {
    let a_right = a.x.saturating_add(a.cols);
    let a_bottom = a.y.saturating_add(a.rows);
    let b_right = b.x.saturating_add(b.cols);
    let b_bottom = b.y.saturating_add(b.rows);
    (a.x == b.x && side_overlap(a.y, a_bottom, b.y, b_bottom, a.rows, b.rows))
        || (a_right == b_right && side_overlap(a.y, a_bottom, b.y, b_bottom, a.rows, b.rows))
        || (a.y == b.y && side_overlap(a.x, a_right, b.x, b_right, a.cols, b.cols))
        || (a_bottom == b_bottom && side_overlap(a.x, a_right, b.x, b_right, a.cols, b.cols))
}

fn side_overlap(a0: u16, a1: u16, b0: u16, b1: u16, a_len: u16, b_len: u16) -> bool {
    let start = a0.max(b0);
    let end = a1.min(b1);
    if end <= start {
        return false;
    }
    let shorter = a_len.min(b_len);
    shorter > 0 && u32::from(end - start).saturating_mul(2) >= u32::from(shorter)
}

#[derive(Clone, Copy)]
struct Rect {
    x: u16,
    y: u16,
    cols: u16,
    rows: u16,
}

fn find_rects(snapshot: &TerminalSnapshot) -> Vec<Rect> {
    let mut rects = Vec::new();
    let cols = snapshot.cols();
    let rows = snapshot.rows();
    let work_limit = usize::from(cols)
        .saturating_mul(usize::from(rows))
        .saturating_mul(32)
        .max(1024);
    let mut work = 0usize;
    for y in 0..rows {
        for x in 0..cols {
            let Some(family) = corner_family(glyph(snapshot, x, y)) else {
                continue;
            };
            let (_, top_right, bottom_left, bottom_right) = family.corners();
            for right in x.saturating_add(MIN_EDGE_CELLS - 1)..cols {
                work = work.saturating_add(1);
                if work > work_limit {
                    return Vec::new();
                }
                if glyph(snapshot, right, y) != top_right {
                    continue;
                }
                work = work.saturating_add(usize::from(right.saturating_sub(x)));
                if work > work_limit {
                    return Vec::new();
                }
                if !row_is_edge(snapshot, x + 1, right, y) {
                    continue;
                }
                for bottom in y.saturating_add(MIN_EDGE_CELLS - 1)..rows {
                    work = work.saturating_add(1);
                    if work > work_limit {
                        return Vec::new();
                    }
                    let left = glyph(snapshot, x, bottom);
                    if left == V {
                        continue;
                    }
                    if left != bottom_left {
                        break;
                    }
                    if glyph(snapshot, right, bottom) != bottom_right {
                        continue;
                    }
                    work = work
                        .saturating_add(usize::from(right.saturating_sub(x)))
                        .saturating_add(usize::from(bottom.saturating_sub(y)).saturating_mul(2));
                    if work > work_limit {
                        return Vec::new();
                    }
                    if !row_is_edge(snapshot, x + 1, right, bottom) {
                        continue;
                    }
                    if !col_is_edge(snapshot, y + 1, bottom, x, false)
                        || !col_is_edge(snapshot, y + 1, bottom, right, true)
                    {
                        continue;
                    }
                    let cols = right - x + 1;
                    let rows = bottom - y + 1;
                    rects.push(Rect { x, y, cols, rows });
                    if rects.len() > MAX_RECTS {
                        return Vec::new();
                    }
                }
            }
        }
    }
    rects.sort_by_key(|rect| (rect.y, rect.x, rect.cols, rect.rows));
    rects.dedup_by(|a, b| a.x == b.x && a.y == b.y && a.cols == b.cols && a.rows == b.rows);
    reject_crossing_rects(rects)
}

fn corner_family(text: &str) -> Option<CornerFamily> {
    match text {
        "┌" => Some(CornerFamily::Square),
        "╭" => Some(CornerFamily::Rounded),
        _ => None,
    }
}

fn reject_crossing_rects(rects: Vec<Rect>) -> Vec<Rect> {
    let mut rejected = vec![false; rects.len()];
    for left in 0..rects.len() {
        for right in left + 1..rects.len() {
            if rects_cross(rects[left], rects[right]) {
                rejected[left] = true;
                rejected[right] = true;
            }
        }
    }
    rects
        .into_iter()
        .zip(rejected)
        .filter_map(|(rect, rejected)| (!rejected).then_some(rect))
        .collect()
}

fn rects_cross(a: Rect, b: Rect) -> bool {
    let overlap_x0 = a.x.max(b.x);
    let overlap_y0 = a.y.max(b.y);
    let overlap_x1 = a.x.saturating_add(a.cols).min(b.x.saturating_add(b.cols));
    let overlap_y1 = a.y.saturating_add(a.rows).min(b.y.saturating_add(b.rows));
    if overlap_x1 <= overlap_x0 || overlap_y1 <= overlap_y0 {
        return false;
    }
    !rect_strictly_contains(a, b) && !rect_strictly_contains(b, a)
}

fn rect_strictly_contains(outer: Rect, inner: Rect) -> bool {
    inner.x > outer.x
        && inner.y > outer.y
        && inner.x.saturating_add(inner.cols) < outer.x.saturating_add(outer.cols)
        && inner.y.saturating_add(inner.rows) < outer.y.saturating_add(outer.rows)
}

fn rect_node(snapshot: &TerminalSnapshot, rect: Rect, id: u64) -> Node {
    let title = read_title(snapshot, &rect);
    let index = title.as_deref().and_then(panel_index);
    let scroll = read_scroll(snapshot, &rect);
    let thumb = read_thumb(snapshot, &rect);
    Node {
        id,
        x: rect.x,
        y: rect.y,
        cols: rect.cols,
        rows: rect.rows,
        title,
        index,
        scroll,
        thumb,
        border: snapshot
            .cell(rect.x, rect.y)
            .map(|cell| cell.fg)
            .unwrap_or(Color::Default),
        body: PaneBody::Empty,
        children: Vec::new(),
    }
}

fn read_title(snapshot: &TerminalSnapshot, rect: &Rect) -> Option<String> {
    if rect.cols < 3 {
        return None;
    }
    let y = rect.y;
    let left = rect.x + 1;
    let right = rect.x + rect.cols - 1;
    let mut first = None;
    let mut last = None;
    for x in left..right {
        if is_frame_glyph(glyph(snapshot, x, y)) {
            continue;
        }
        if first.is_none() {
            first = Some(x);
        }
        last = Some(x);
    }
    let (Some(mut start), Some(mut end)) = (first, last) else {
        return None;
    };
    while start < end && glyph(snapshot, start, y).chars().all(char::is_whitespace) {
        start += 1;
    }
    while end > start && glyph(snapshot, end, y).chars().all(char::is_whitespace) {
        end -= 1;
    }
    let mut title = String::new();
    for x in start..=end {
        title.push_str(glyph(snapshot, x, y));
    }
    if !title.chars().any(|ch| ch.is_alphanumeric()) {
        return None;
    }
    Some(title)
}

fn panel_index(title: &str) -> Option<u16> {
    let rest = title.strip_prefix('[')?;
    let (num, after) = rest.split_once(']')?;
    if num.len() != 1 || !num.chars().all(|ch| ch.is_ascii_digit()) {
        return None;
    }
    let name = after.trim_start_matches([' ', '─']);
    if !(name.is_empty() || name.starts_with(|ch: char| ch.is_ascii_alphabetic())) {
        return None;
    }
    let index = num.parse::<u16>().ok()?;
    (index <= 5).then_some(index)
}

fn read_scroll(snapshot: &TerminalSnapshot, rect: &Rect) -> Option<(u32, u32)> {
    if rect.rows < 2 {
        return None;
    }
    let bottom = rect.y + rect.rows - 1;
    let rows = [bottom, bottom - 1];
    for y in rows {
        if let Some((current, total, _)) = find_n_of_m(snapshot, rect.x, rect.x + rect.cols, y) {
            return Some((current, total));
        }
    }
    None
}

fn find_n_of_m(
    snapshot: &TerminalSnapshot,
    x0: u16,
    x1: u16,
    y: u16,
) -> Option<(u32, u32, Vec<(u16, u16)>)> {
    let mut text = String::new();
    let mut xs = Vec::new();
    for x in x0..x1 {
        let grapheme = glyph(snapshot, x, y);
        let mut chars = grapheme.chars();
        let Some(ch) = chars.next() else {
            if let Some(hit) = match_n_of_m(&text, &xs, y) {
                return Some(hit);
            }
            text.clear();
            xs.clear();
            continue;
        };
        if chars.next().is_some() || !(ch.is_ascii_digit() || ch == ' ' || ch == 'o' || ch == 'f') {
            if let Some(hit) = match_n_of_m(&text, &xs, y) {
                return Some(hit);
            }
            text.clear();
            xs.clear();
            continue;
        }
        text.push(ch);
        xs.push(x);
    }
    match_n_of_m(&text, &xs, y)
}

fn read_thumb(snapshot: &TerminalSnapshot, rect: &Rect) -> Option<Thumb> {
    if rect.cols < 2 || rect.rows < 4 {
        return None;
    }
    let right = rect.x + rect.cols - 1;
    let track_top = rect.y + 1;
    let track_bottom = rect.y + rect.rows - 2;
    if track_bottom < track_top {
        return None;
    }
    let corner_fg = snapshot
        .cell(right, rect.y)
        .or_else(|| snapshot.cell(rect.x, rect.y))
        .map(|cell| cell.fg)
        .unwrap_or(Color::Default);
    let mut marked = Vec::new();
    for y in track_top..=track_bottom {
        marked.push((y, is_thumb_cell(snapshot, right, y, corner_fg)));
    }
    if marked.iter().all(|(_, hit)| *hit) || !marked.iter().any(|(_, hit)| *hit) {
        return None;
    }
    let mut best: Option<(u16, u16, u32)> = None;
    let mut run_start = None;
    let close = |best: &mut Option<(u16, u16, u32)>, start: u16, end: u16| {
        let len = u32::from(end.saturating_sub(start)).saturating_add(1);
        if best.map(|(_, _, prev)| prev).unwrap_or(0) < len {
            *best = Some((start, end, len));
        }
    };
    for &(y, hit) in &marked {
        if hit {
            if run_start.is_none() {
                run_start = Some(y);
            }
        } else if let Some(start) = run_start.take() {
            close(&mut best, start, y.saturating_sub(1));
        }
    }
    if let Some(start) = run_start {
        close(&mut best, start, track_bottom);
    }
    let (start, end, _) = best?;
    Some(Thumb {
        start,
        end,
        track_top,
        track_bottom,
    })
}

fn is_thumb_cell(snapshot: &TerminalSnapshot, x: u16, y: u16, corner_fg: Color) -> bool {
    let Some(cell) = snapshot.cell(x, y) else {
        return false;
    };
    if is_thumb(&cell.grapheme) {
        return true;
    }
    cell.grapheme == V && cell.fg != corner_fg
}

fn match_n_of_m(text: &str, xs: &[u16], y: u16) -> Option<(u32, u32, Vec<(u16, u16)>)> {
    let bytes = text.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if !bytes[index].is_ascii_digit() {
            index += 1;
            continue;
        }
        let n_start = index;
        while index < bytes.len() && bytes[index].is_ascii_digit() {
            index += 1;
        }
        if index + 4 > bytes.len() || &text[index..index + 4] != " of " {
            continue;
        }
        let m_start = index + 4;
        let mut m_end = m_start;
        while m_end < bytes.len() && bytes[m_end].is_ascii_digit() {
            m_end += 1;
        }
        if m_end == m_start {
            index = m_start;
            continue;
        }
        let current: u32 = text[n_start..index].parse().ok()?;
        let total: u32 = text[m_start..m_end].parse().ok()?;
        if current == 0 || total == 0 || current > total {
            index = m_end;
            continue;
        }
        let cells = xs[n_start..m_end].iter().map(|x| (*x, y)).collect();
        return Some((current, total, cells));
    }
    None
}

fn is_frame_glyph(text: &str) -> bool {
    text.is_empty() || is_box(text)
}

fn row_is_edge(snapshot: &TerminalSnapshot, start: u16, end: u16, y: u16) -> bool {
    enum Seg {
        Text(String),
        Keycap(String),
    }
    let mut segs = Vec::new();
    let mut text_run = String::new();
    let mut line_cells = 0u16;
    let mut x = start;
    let flush_text = |run: &mut String, segs: &mut Vec<Seg>| {
        if !run.is_empty() {
            segs.push(Seg::Text(std::mem::take(run)));
        }
    };
    while x < end {
        let text = glyph(snapshot, x, y);
        if snapshot
            .cell(x, y)
            .is_some_and(|cell| cell.occupancy == CellOccupancy::WideContinuation)
        {
            x = x.saturating_add(1);
            continue;
        }
        if text == H {
            flush_text(&mut text_run, &mut segs);
            line_cells = line_cells.saturating_add(1);
            x = x.saturating_add(1);
            continue;
        }
        // Bubble title / keycap footer: ┤ label ├ amid ─ runs.
        if text == "┤" {
            flush_text(&mut text_run, &mut segs);
            x = x.saturating_add(1);
            let mut label = String::new();
            while x < end {
                let part = glyph(snapshot, x, y);
                if snapshot
                    .cell(x, y)
                    .is_some_and(|cell| cell.occupancy == CellOccupancy::WideContinuation)
                {
                    x = x.saturating_add(1);
                    continue;
                }
                if part == "├" {
                    break;
                }
                if part == H
                    || part.is_empty()
                    || part.chars().any(char::is_control)
                    || is_box(part)
                {
                    return false;
                }
                label.push_str(part);
                x = x.saturating_add(1);
            }
            if x >= end || glyph(snapshot, x, y) != "├" {
                return false;
            }
            if label.is_empty() || label.chars().all(char::is_whitespace) {
                return false;
            }
            segs.push(Seg::Keycap(label));
            x = x.saturating_add(1);
            continue;
        }
        if text.is_empty() || text.chars().any(char::is_control) || is_box(text) {
            return false;
        }
        text_run.push_str(text);
        x = x.saturating_add(1);
    }
    flush_text(&mut text_run, &mut segs);
    if line_cells == 0
        || segs.iter().any(|seg| match seg {
            Seg::Text(text) | Seg::Keycap(text) => text.chars().all(char::is_whitespace),
        })
    {
        return false;
    }
    match segs.as_slice() {
        [] => true,
        [Seg::Text(_)] => true,
        [Seg::Text(index), Seg::Text(title)] => {
            panel_index_prefix(index) && title.starts_with(char::is_alphabetic)
        }
        keycaps if keycaps.iter().all(|seg| matches!(seg, Seg::Keycap(_))) => true,
        _ => false,
    }
}

fn col_is_edge(
    snapshot: &TerminalSnapshot,
    start: u16,
    end: u16,
    x: u16,
    allow_thumb: bool,
) -> bool {
    if !allow_thumb {
        return (start..end).all(|y| glyph(snapshot, x, y) == V);
    }
    let corner_fg = snapshot
        .cell(x, start.saturating_sub(1))
        .map(|cell| cell.fg)
        .unwrap_or(Color::Default);
    let mut marked = 0usize;
    let mut runs = 0usize;
    let mut in_run = false;
    for y in start..end {
        let text = glyph(snapshot, x, y);
        if text != V && !is_thumb(text) {
            return false;
        }
        let hit = is_thumb_cell(snapshot, x, y, corner_fg);
        if hit {
            marked += 1;
            if !in_run {
                runs += 1;
                in_run = true;
            }
        } else {
            in_run = false;
        }
    }
    let len = usize::from(end.saturating_sub(start));
    runs <= 1 && (marked == 0 || marked < len)
}

fn panel_index_prefix(text: &str) -> bool {
    let Some(number) = text
        .strip_prefix('[')
        .and_then(|text| text.strip_suffix(']'))
    else {
        return false;
    };
    !number.is_empty() && number.chars().all(|ch| ch.is_ascii_digit())
}

fn is_box(text: &str) -> bool {
    matches!(
        text,
        "─" | "│"
            | "┌"
            | "┐"
            | "└"
            | "┘"
            | "╭"
            | "╮"
            | "╰"
            | "╯"
            | "├"
            | "┤"
            | "┬"
            | "┴"
            | "┼"
            | "┏"
            | "┓"
            | "┗"
            | "┛"
            | "━"
            | "┃"
            | "╔"
            | "╗"
            | "╚"
            | "╝"
            | "═"
            | "║"
            | "█"
            | "▐"
            | "▌"
            | "▓"
    )
}

fn is_thumb(text: &str) -> bool {
    matches!(text, "┃" | "█" | "▐" | "▌" | "▓")
}

fn glyph<'a>(snapshot: &'a TerminalSnapshot, x: u16, y: u16) -> &'a str {
    snapshot
        .cell(x, y)
        .map(|cell| cell.grapheme.as_str())
        .unwrap_or("")
}

fn nest(nodes: Vec<Node>) -> Vec<Node> {
    let mut parent = vec![None; nodes.len()];
    for index in 0..nodes.len() {
        let mut best: Option<usize> = None;
        for other in 0..nodes.len() {
            if index == other || !strictly_contains(&nodes[other], &nodes[index]) {
                continue;
            }
            let tighter = best.is_none_or(|current| area(&nodes[other]) < area(&nodes[current]));
            if tighter {
                best = Some(other);
            }
        }
        parent[index] = best;
    }
    let mut child_indexes = vec![Vec::new(); nodes.len()];
    let mut roots = Vec::new();
    for index in 0..nodes.len() {
        if let Some(parent_index) = parent[index] {
            child_indexes[parent_index].push(index);
        } else {
            roots.push(index);
        }
    }
    fn build(index: usize, nodes: &[Node], child_indexes: &[Vec<usize>]) -> Node {
        let mut node = nodes[index].clone();
        node.children = child_indexes[index]
            .iter()
            .copied()
            .map(|child| build(child, nodes, child_indexes))
            .collect();
        node.children
            .sort_by_key(|child| (child.y, child.x, child.id));
        node
    }
    let mut rooted: Vec<Node> = roots
        .into_iter()
        .map(|index| build(index, &nodes, &child_indexes))
        .collect();
    rooted.sort_by_key(|node| (node.y, node.x, node.id));
    rooted
}

fn flatten(nodes: &[Node]) -> Vec<Node> {
    let mut out = Vec::new();
    for node in nodes {
        let mut copy = node.clone();
        copy.children.clear();
        out.push(copy);
        out.extend(flatten(&node.children));
    }
    out
}

fn count_nodes(nodes: &[Node]) -> u64 {
    nodes
        .iter()
        .map(|node| 1 + count_nodes(&node.children))
        .sum()
}

fn suppress_for(snapshot: &TerminalSnapshot, tree: &LayoutTree) -> HashSet<(u16, u16)> {
    let mut suppress = HashSet::new();
    let mut nodes = flatten(&tree.layout);
    nodes.extend(flatten(&tree.overlays));
    for node in &nodes {
        let right = node.x.saturating_add(node.cols).min(tree.cols);
        let bottom = node.y.saturating_add(node.rows).min(tree.rows);
        for y in node.y..bottom {
            for x in node.x..right {
                suppress.insert((x, y));
            }
        }
    }
    for row in &tree.chrome {
        for x in 0..tree.cols {
            suppress.insert((x, row.y));
        }
    }
    for (x, y) in suppress.iter().copied().collect::<Vec<_>>() {
        match snapshot.cell(x, y).map(|cell| cell.occupancy) {
            Some(CellOccupancy::WideLeading) if x + 1 < tree.cols => {
                suppress.insert((x + 1, y));
            }
            Some(CellOccupancy::WideContinuation) if x > 0 => {
                suppress.insert((x - 1, y));
            }
            _ => {}
        }
    }
    suppress
}

fn strictly_contains(outer: &Node, inner: &Node) -> bool {
    inner.x > outer.x
        && inner.y > outer.y
        && inner.x + inner.cols < outer.x + outer.cols
        && inner.y + inner.rows < outer.y + outer.rows
}

fn interiors_overlap(a: &Node, b: &Node) -> bool {
    let Some(a) = interior(a) else {
        return false;
    };
    let Some(b) = interior(b) else {
        return false;
    };
    a.0 <= b.2 && b.0 <= a.2 && a.1 <= b.3 && b.1 <= a.3
}

fn interior(node: &Node) -> Option<(u16, u16, u16, u16)> {
    if node.cols < 3 || node.rows < 3 {
        return None;
    }
    Some((
        node.x + 1,
        node.y + 1,
        node.x + node.cols - 2,
        node.y + node.rows - 2,
    ))
}

fn node_rects(nodes: &[Node]) -> Vec<Rect> {
    flatten(nodes)
        .into_iter()
        .map(|node| Rect {
            x: node.x,
            y: node.y,
            cols: node.cols,
            rows: node.rows,
        })
        .collect()
}

fn fill_bodies(snapshot: &TerminalSnapshot, nodes: &mut [Node], holes: &[Rect]) {
    for node in nodes {
        let mut covered = holes.to_vec();
        for child in &node.children {
            covered.push(Rect {
                x: child.x,
                y: child.y,
                cols: child.cols,
                rows: child.rows,
            });
        }
        node.body = read_body(snapshot, node, &covered);
        fill_bodies(snapshot, &mut node.children, holes);
    }
}

fn read_body(snapshot: &TerminalSnapshot, node: &Node, holes: &[Rect]) -> PaneBody {
    let Some((x0, y0, x1, y1)) = interior(node) else {
        return PaneBody::Empty;
    };
    let mut lines = Vec::new();
    for y in y0..=y1 {
        let xs = (x0..=x1).filter(|x| !holes.iter().any(|hole| rect_contains(hole, *x, y)));
        if let Some(line) = collect_line(snapshot, y, xs) {
            lines.push(line);
        }
    }
    classify(lines)
}

fn chrome_rows(snapshot: &TerminalSnapshot, tree: &LayoutTree) -> Vec<ChromeRow> {
    if tree.layout.is_empty() && tree.overlays.is_empty() {
        return Vec::new();
    }
    let mut rects = node_rects(&tree.layout);
    rects.extend(node_rects(&tree.overlays));
    rects.extend(tree.history_layout.iter().map(|node| Rect {
        x: node.x,
        y: node.y,
        cols: node.cols,
        rows: node.rows,
    }));
    let mut rows = Vec::new();
    for y in 0..snapshot.rows() {
        if rects
            .iter()
            .any(|rect| y >= rect.y && y < rect.y.saturating_add(rect.rows))
        {
            continue;
        }
        if let Some(line) = collect_line(snapshot, y, 0..snapshot.cols()) {
            rows.push(ChromeRow { y, line });
        }
    }
    rows
}

fn collect_line(
    snapshot: &TerminalSnapshot,
    y: u16,
    xs: impl Iterator<Item = u16>,
) -> Option<ContentLine> {
    let mut spans = Vec::new();
    let mut selected = false;
    for x in xs {
        let Some(cell) = snapshot.cell(x, y) else {
            continue;
        };
        if cell.occupancy == CellOccupancy::WideContinuation {
            continue;
        }
        if cell.style.reverse {
            selected = true;
        }
        push_span(&mut spans, cell);
    }
    if !spans
        .iter()
        .any(|span| span.text.chars().any(|ch| !ch.is_whitespace()))
    {
        return None;
    }
    Some(ContentLine { spans, selected })
}

fn push_span(spans: &mut Vec<Span>, cell: &Cell) {
    let text = if cell.grapheme.is_empty() || cell.grapheme.chars().any(char::is_control) {
        " ".to_string()
    } else {
        cell.grapheme.clone()
    };
    if let Some(span) = spans.last_mut() {
        if span.fg == cell.fg && span.bg == cell.bg && span.style == cell.style {
            span.text.push_str(&text);
            return;
        }
    }
    spans.push(Span {
        text,
        fg: cell.fg,
        bg: cell.bg,
        style: cell.style,
    });
}

fn classify(lines: Vec<ContentLine>) -> PaneBody {
    if lines.is_empty() {
        return PaneBody::Empty;
    }
    let diffs = lines.iter().filter(|line| is_diff_line(line)).count();
    if diffs >= 3 {
        PaneBody::Text(lines)
    } else {
        PaneBody::Rows(lines)
    }
}

fn is_diff_line(line: &ContentLine) -> bool {
    let text = line_text(line);
    let trimmed = text.trim_start();
    trimmed.starts_with('+') || trimmed.starts_with('-') || text.contains("@@")
}

pub(crate) fn line_text(line: &ContentLine) -> String {
    line.spans.iter().map(|span| span.text.as_str()).collect()
}

fn rect_contains(rect: &Rect, x: u16, y: u16) -> bool {
    x >= rect.x
        && y >= rect.y
        && x < rect.x.saturating_add(rect.cols)
        && y < rect.y.saturating_add(rect.rows)
}

fn area(node: &Node) -> u32 {
    u32::from(node.cols) * u32::from(node.rows)
}

fn iou(a: &Node, b: &Node) -> f32 {
    let ax1 = u32::from(a.x) + u32::from(a.cols);
    let ay1 = u32::from(a.y) + u32::from(a.rows);
    let bx1 = u32::from(b.x) + u32::from(b.cols);
    let by1 = u32::from(b.y) + u32::from(b.rows);
    let ix0 = u32::from(a.x).max(u32::from(b.x));
    let iy0 = u32::from(a.y).max(u32::from(b.y));
    let ix1 = ax1.min(bx1);
    let iy1 = ay1.min(by1);
    if ix1 <= ix0 || iy1 <= iy0 {
        return 0.0;
    }
    let inter = (ix1 - ix0) as f32 * (iy1 - iy0) as f32;
    let union = area(a) as f32 + area(b) as f32 - inter;
    if union <= 0.0 {
        0.0
    } else {
        inter / union
    }
}

fn write_nodes(
    f: &mut fmt::Formatter<'_>,
    nodes: &[Node],
    prefix: &str,
    kind: &str,
) -> fmt::Result {
    for (index, node) in nodes.iter().enumerate() {
        let last = index + 1 == nodes.len();
        let branch = if last { "└── " } else { "├── " };
        write!(f, "{prefix}{branch}")?;
        write_node_line(f, node, kind)?;
        let child_prefix = if last {
            format!("{prefix}    ")
        } else {
            format!("{prefix}│   ")
        };
        write_nodes(f, &node.children, &child_prefix, kind)?;
    }
    Ok(())
}

fn write_node_line(f: &mut fmt::Formatter<'_>, node: &Node, kind: &str) -> fmt::Result {
    write!(
        f,
        "{kind} {} @{},{} {}×{}",
        node.id, node.x, node.y, node.cols, node.rows
    )?;
    if let Some(title) = &node.title {
        write!(f, " \"{title}\"")?;
    }
    if let Some((current, total)) = node.scroll {
        write!(f, " {current}/{total}")?;
    }
    if let Some(thumb) = node.thumb {
        write!(f, " thumb {}-{}", thumb.start, thumb.end)?;
    }
    match &node.body {
        PaneBody::Rows(lines) => write!(f, " rows {}", lines.len())?,
        PaneBody::Text(lines) => write!(f, " text {}", lines.len())?,
        PaneBody::Empty => {}
    }
    writeln!(f)
}

pub(crate) fn node_rect(node: &Node) -> (u16, u16, u16, u16, Color) {
    (node.x, node.y, node.cols, node.rows, node.border)
}

pub(crate) fn node_children(node: &Node) -> &[Node] {
    &node.children
}

pub(crate) fn node_scroll(node: &Node) -> Option<(u32, u32)> {
    node.scroll
}

pub(crate) fn node_thumb(node: &Node) -> Option<Thumb> {
    node.thumb
}

pub(crate) fn node_id(node: &Node) -> u64 {
    node.id
}

pub(crate) fn node_title(node: &Node) -> Option<&str> {
    node.title.as_deref()
}

pub(crate) fn node_body(node: &Node) -> &PaneBody {
    &node.body
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::overlay::test_grid::test_grid;

    fn stamp(cell: &mut crate::terminal::Cell, x0: u16, y0: u16, x1: u16, y1: u16) {
        stamp_family(cell, x0, y0, x1, y1, CornerFamily::Square);
    }

    fn stamp_rounded(cell: &mut crate::terminal::Cell, x0: u16, y0: u16, x1: u16, y1: u16) {
        stamp_family(cell, x0, y0, x1, y1, CornerFamily::Rounded);
    }

    fn stamp_family(
        cell: &mut crate::terminal::Cell,
        x0: u16,
        y0: u16,
        x1: u16,
        y1: u16,
        family: CornerFamily,
    ) {
        let x = cell.x;
        let y = cell.y;
        if x < x0 || x > x1 || y < y0 || y > y1 {
            return;
        }
        let (tl, tr, bl, br) = family.corners();
        let glyph = if x == x0 && y == y0 {
            tl
        } else if x == x1 && y == y0 {
            tr
        } else if x == x0 && y == y1 {
            bl
        } else if x == x1 && y == y1 {
            br
        } else if y == y0 || y == y1 {
            "─"
        } else if x == x0 || x == x1 {
            "│"
        } else {
            return;
        };
        cell.grapheme = glyph.to_string();
    }

    /// Write `┤label├` starting at `x` on row `y` (superfile-style bubble/keycap).
    fn write_keycap(cell: &mut crate::terminal::Cell, x: u16, y: u16, label: &str) {
        if cell.y != y {
            return;
        }
        let end = x + 1 + label.chars().count() as u16;
        if cell.x == x {
            cell.grapheme = "┤".to_string();
        } else if cell.x == end {
            cell.grapheme = "├".to_string();
        } else if cell.x > x && cell.x < end {
            let index = (cell.x - x - 1) as usize;
            if let Some(ch) = label.chars().nth(index) {
                cell.grapheme = ch.to_string();
            }
        }
    }

    fn ids(nodes: &[Node]) -> Vec<(u64, u16, u16, u16, u16)> {
        let mut out = Vec::new();
        for node in nodes {
            out.push((node.id, node.x, node.y, node.cols, node.rows));
            out.extend(ids(&node.children));
        }
        out
    }

    #[test]
    fn two_panes_are_layout_siblings() {
        let snap = test_grid(32, 12, |cell| {
            stamp(cell, 0, 0, 11, 11);
            stamp(cell, 12, 0, 31, 11);
            if cell.y == 0 && (2..7).contains(&cell.x) {
                cell.grapheme = "Files"
                    .chars()
                    .nth((cell.x - 2) as usize)
                    .unwrap()
                    .to_string();
            }
        });
        let tree = parse(&snap);
        let text = tree.to_string();
        assert!(text.contains("├── Layout"), "{text}");
        assert!(text.contains("Panel 1 @0,0 12×12 \"Files\""), "{text}");
        assert!(text.contains("Panel 2 @12,0 20×12"), "{text}");
        assert!(!text.contains("Overlay\n    └──"), "{text}");
    }

    #[test]
    fn multi_codepoint_and_wide_titles_are_valid_edges() {
        let snapshot = test_grid(16, 8, |cell| {
            stamp(cell, 0, 0, 15, 7);
            if cell.y == 0 && cell.x == 2 {
                cell.grapheme = "e\u{301}".to_string();
            }
            if cell.y == 0 && cell.x == 3 {
                cell.grapheme = "界".to_string();
                cell.occupancy = CellOccupancy::WideLeading;
            }
            if cell.y == 0 && cell.x == 4 {
                cell.grapheme.clear();
                cell.occupancy = CellOccupancy::WideContinuation;
            }
        });
        let tree = parse(&snapshot);
        assert_eq!(tree.layout().len(), 1, "{tree}");
        assert_eq!(tree.layout()[0].title.as_deref(), Some("e\u{301}界"));
    }

    #[test]
    fn bubble_title_on_top_edge_is_a_panel() {
        let snap = test_grid(24, 6, |cell| {
            stamp_rounded(cell, 0, 0, 23, 5);
            write_keycap(cell, 2, 0, " Processes ");
        });
        let tree = parse(&snap);
        assert_eq!(tree.layout().len(), 1, "{tree}");
        assert_eq!(tree.layout()[0].title.as_deref(), Some("Processes"));
        assert!(tree.suppress().contains(&(2, 0)), "┤ suppressed");
        assert!(tree.suppress().contains(&(4, 0)), "title suppressed");
    }

    #[test]
    fn keycap_footer_on_bottom_edge_is_a_panel() {
        let snap = test_grid(40, 6, |cell| {
            stamp_rounded(cell, 0, 0, 39, 5);
            write_keycap(cell, 10, 5, " Name ");
            write_keycap(cell, 18, 5, " Browser ");
            write_keycap(cell, 30, 5, "0/0");
        });
        let tree = parse(&snap);
        assert_eq!(tree.layout().len(), 1, "{tree}");
        assert_eq!(tree.layout()[0].cols, 40, "{tree}");
        assert!(tree.suppress().contains(&(10, 5)), "footer ┤ suppressed");
        assert!(tree.suppress().contains(&(31, 5)), "0/0 suppressed");
    }

    #[test]
    fn tee_junction_on_edge_without_keycap_stays_raw() {
        let snap = test_grid(16, 6, |cell| {
            stamp(cell, 0, 0, 15, 5);
            if cell.y == 0 && cell.x == 8 {
                cell.grapheme = "┬".to_string();
            }
        });
        let tree = parse(&snap);
        assert_eq!(tree.node_count(), 0, "bare ┬ must stay raw:\n{tree}");
    }

    #[test]
    fn superfile_like_bubble_panels_are_detected() {
        let snap = test_grid(48, 14, |cell| {
            stamp_rounded(cell, 0, 0, 11, 8);
            write_at(cell, 2, 1, "superfile");
            stamp_rounded(cell, 12, 0, 47, 8);
            write_at(cell, 14, 1, "/home");
            write_keycap(cell, 28, 8, " Name ");
            write_keycap(cell, 36, 8, "0/0");
            stamp_rounded(cell, 0, 9, 15, 13);
            write_keycap(cell, 2, 9, " Processes ");
            stamp_rounded(cell, 16, 9, 31, 13);
            write_keycap(cell, 18, 9, " Metadata ");
            stamp_rounded(cell, 32, 9, 47, 13);
            write_keycap(cell, 34, 9, " Clipboard ");
        });
        let tree = parse(&snap);
        let text = tree.to_string();
        assert_eq!(tree.node_count(), 5, "{text}");
        assert!(text.contains("Panel 1 @0,0 12×9"), "{text}");
        assert!(text.contains("@12,0 36×9"), "{text}");
        assert!(text.contains("\"Processes\""), "{text}");
        assert!(text.contains("\"Metadata\""), "{text}");
        assert!(text.contains("\"Clipboard\""), "{text}");
    }

    #[test]
    fn ambiguous_or_broken_frames_stay_raw() {
        let broken = test_grid(16, 8, |cell| {
            stamp(cell, 0, 0, 15, 7);
            if cell.x == 6 && cell.y == 0 {
                cell.grapheme = " ".to_string();
            }
        });
        let mixed = test_grid(16, 8, |cell| {
            stamp(cell, 0, 0, 15, 7);
            if cell.x == 15 && cell.y == 0 {
                cell.grapheme = "╮".to_string();
            }
        });
        let two_titles = test_grid(20, 8, |cell| {
            stamp(cell, 0, 0, 19, 7);
            write_at(cell, 2, 0, "One");
            write_at(cell, 10, 0, "Two");
        });
        let junctions = test_grid(20, 8, |cell| {
            cell.grapheme = "┼".to_string();
        });

        for snapshot in [&broken, &mixed, &two_titles, &junctions] {
            let tree = parse(snapshot);
            assert_eq!(tree.node_count(), 0, "ambiguous frame enhanced:\n{tree}");
            assert!(tree.suppress().is_empty());
            assert!(tree.chrome().is_empty());
        }
    }

    #[test]
    fn malformed_sibling_content_does_not_become_chrome() {
        let snapshot = test_grid(32, 12, |cell| {
            stamp(cell, 0, 0, 15, 10);
            stamp(cell, 16, 0, 31, 10);
            write_at(cell, 18, 2, "raw sibling");
            if cell.x == 24 && cell.y == 0 {
                cell.grapheme = " ".to_string();
            }
            write_at(cell, 0, 11, "ready");
        });
        let tree = parse(&snapshot);
        assert_eq!(tree.layout().len(), 1, "{tree}");
        assert!(tree
            .chrome()
            .iter()
            .all(|row| !line_text(&row.line).contains("raw sibling")));
        assert!(tree
            .chrome()
            .iter()
            .any(|row| line_text(&row.line).contains("ready")));
    }

    #[test]
    fn global_assignment_avoids_greedy_identity_swap() {
        fn score(fresh: &Node, previous: &Node) -> i64 {
            match (fresh.id, previous.id) {
                (1, 10) => 9,
                (1, 11) => 8,
                (2, 10) => 8,
                _ => 0,
            }
        }
        let fresh = vec![
            blank_untitled(1, 0, 0, 3, 3, "first"),
            blank_untitled(2, 4, 0, 3, 3, "second"),
        ];
        let previous = vec![
            blank_untitled(10, 0, 0, 3, 3, "left"),
            blank_untitled(11, 4, 0, 3, 3, "right"),
        ];
        assert_eq!(
            assign_global(&fresh, &previous, score),
            vec![Some(1), Some(0)]
        );
    }

    #[test]
    fn equal_score_assignment_is_deterministic() {
        fn tied(_: &Node, _: &Node) -> i64 {
            10
        }
        let fresh = vec![blank_untitled(1, 0, 0, 3, 3, "fresh")];
        let previous = vec![
            blank_untitled(10, 0, 0, 3, 3, "one"),
            blank_untitled(11, 0, 0, 3, 3, "two"),
        ];
        let first = assign_global(&fresh, &previous, tied);
        assert_eq!(first, vec![Some(0)]);
        assert_eq!(assign_global(&fresh, &previous, tied), first);
    }

    #[test]
    fn parser_is_deterministic_and_bounded_on_corner_noise() {
        let valid = test_grid(32, 12, |cell| {
            stamp(cell, 0, 0, 15, 11);
            stamp(cell, 16, 0, 31, 11);
        });
        let first = parse(&valid);
        let second = parse(&valid);
        assert_eq!(first.to_string(), second.to_string());
        assert_eq!(first.suppress(), second.suppress());

        let noise = test_grid(120, 60, |cell| {
            cell.grapheme = "┌".to_string();
        });
        let tree = parse(&noise);
        assert_eq!(
            tree.node_count(),
            0,
            "budget exhaustion must fall back to raw"
        );
        assert!(tree.suppress().is_empty());
    }

    #[test]
    fn realistic_popup_fixture_detects_only_confirmed_frames() {
        let lines: Vec<Vec<char>> = include_str!("testdata_lg_popup.txt")
            .lines()
            .map(|line| line.chars().collect())
            .collect();
        let cols = lines.iter().map(Vec::len).max().unwrap_or(1) as u16;
        let rows = lines.len() as u16;
        let snapshot = test_grid(cols, rows, |cell| {
            if let Some(ch) = lines
                .get(usize::from(cell.y))
                .and_then(|line| line.get(usize::from(cell.x)))
            {
                cell.grapheme = ch.to_string();
            }
        });
        let tree = parse(&snapshot);
        let nodes = flatten(tree.layout());
        assert!(
            nodes.iter().any(|node| node
                .title
                .as_deref()
                .is_some_and(|title| title.contains("Keybindings"))),
            "popup frame missing:\n{tree}"
        );
        assert!(nodes.iter().all(|node| node.x < cols && node.y < rows));
    }

    #[test]
    fn invalid_scroll_count_is_not_semantic_metadata() {
        let snapshot = test_grid(20, 8, |cell| {
            stamp(cell, 0, 0, 19, 7);
            write_at(cell, 10, 7, "9 of 2");
        });
        let tree = parse(&snapshot);
        assert_eq!(tree.layout().len(), 1, "{tree}");
        assert_eq!(tree.layout()[0].scroll, None);
    }

    #[test]
    fn dialog_merges_above_the_previous_panes() {
        let clean = test_grid(32, 12, |cell| {
            stamp(cell, 0, 0, 11, 11);
            stamp(cell, 12, 0, 31, 11);
        });
        let mut retained = None;
        let first = reconcile(&clean, &mut retained);
        let left_id = ids(first.layout())
            .iter()
            .find(|item| item.1 == 0)
            .unwrap()
            .0;

        let covered = test_grid(32, 12, |cell| {
            stamp(cell, 0, 0, 11, 11);
            stamp(cell, 12, 0, 31, 11);
            stamp(cell, 8, 2, 22, 9);
            if cell.y == 2 && (10..14).contains(&cell.x) {
                cell.grapheme = "Keys"
                    .chars()
                    .nth((cell.x - 10) as usize)
                    .unwrap()
                    .to_string();
            }
        });
        let tree = reconcile(&covered, &mut retained);
        let text = tree.to_string();
        assert!(
            tree.history_layout.iter().any(|node| node.id == left_id),
            "left pane identity was not retained behind the dialog:\n{text}"
        );
        assert!(
            ids(tree.layout()).iter().any(|item| item.0 == left_id),
            "occluded pane card disappeared:\n{text}"
        );
        assert!(text.contains("└── Overlay"), "{text}");
        assert!(text.contains("Dialog 3 @8,2 15×8 \"Keys\""), "{text}");
        assert!(
            tree.overlays().len() == 1,
            "dialog was nested into a pane:\n{text}"
        );
    }

    #[test]
    fn a_damaged_dialog_falls_back_to_raw() {
        let clean = test_grid(32, 12, |cell| {
            stamp(cell, 0, 0, 11, 11);
            stamp(cell, 12, 0, 31, 11);
        });
        let mut retained = None;
        reconcile(&clean, &mut retained);
        let covered = test_grid(32, 12, |cell| {
            stamp(cell, 0, 0, 11, 11);
            stamp(cell, 12, 0, 31, 11);
            stamp(cell, 8, 2, 22, 9);
        });
        reconcile(&covered, &mut retained);
        let damaged = test_grid(32, 12, |cell| {
            stamp(cell, 0, 0, 11, 11);
            stamp(cell, 12, 0, 31, 11);
            stamp(cell, 8, 2, 22, 9);
            if cell.x == 22 && (2..10).contains(&cell.y) {
                cell.grapheme = "x".to_string();
            }
        });
        let tree = reconcile(&damaged, &mut retained);
        assert!(tree.layout().is_empty(), "{tree}");
        assert!(tree.overlays().is_empty(), "{tree}");
        assert!(tree.suppress().is_empty(), "damaged frame hid raw cells");
        assert!(tree.chrome().is_empty(), "damaged frame enhanced chrome");
    }

    #[test]
    fn closing_the_dialog_removes_the_overlay() {
        let clean = test_grid(32, 12, |cell| {
            stamp(cell, 0, 0, 11, 11);
            stamp(cell, 12, 0, 31, 11);
        });
        let mut retained = None;
        reconcile(&clean, &mut retained);
        let covered = test_grid(32, 12, |cell| {
            stamp(cell, 0, 0, 11, 11);
            stamp(cell, 12, 0, 31, 11);
            stamp(cell, 8, 2, 22, 9);
        });
        reconcile(&covered, &mut retained);
        let tree = reconcile(&clean, &mut retained);
        assert!(tree.overlays().is_empty(), "{}", tree);
        assert_eq!(tree.layout().len(), 2, "{}", tree);
    }

    #[test]
    fn keybindings_stay_above_the_diff_pane() {
        let clean = test_grid(168, 41, |cell| {
            stamp(cell, 0, 0, 55, 2);
            stamp(cell, 56, 0, 167, 39);
        });
        let mut retained = None;
        let first = reconcile(&clean, &mut retained);
        let diff_id = ids(first.layout())
            .iter()
            .find(|item| item.1 == 56)
            .unwrap()
            .0;

        let covered = test_grid(168, 41, |cell| {
            stamp(cell, 0, 0, 55, 2);
            stamp(cell, 56, 0, 167, 39);
            stamp(cell, 39, 6, 128, 40);
            write_at(cell, 41, 6, "Keybindings");
        });
        let tree = reconcile(&covered, &mut retained);
        let text = tree.to_string();
        assert!(
            ids(&tree.history_layout)
                .iter()
                .any(|item| item.0 == diff_id && *item == (diff_id, 56, 0, 112, 40)),
            "diff pane identity was not retained behind the dialog:\n{text}"
        );
        assert_eq!(tree.layout().len(), 2, "pane cards disappeared:\n{text}");
        assert!(
            text.contains("Dialog") && text.contains("@39,6 90×35 \"Keybindings\""),
            "dialog missing:\n{text}"
        );
        assert!(
            tree.overlays().len() == 1,
            "dialog joined the layout:\n{text}"
        );
    }

    #[test]
    fn command_log_and_keybindings_keep_all_pane_cards() {
        fn base(cell: &mut crate::terminal::Cell) {
            titled(cell, 0, 0, 55, 2, "[1]─Status");
            titled(cell, 56, 0, 167, 39, "[0]─Log");
            titled(cell, 56, 40, 167, 49, "Command log");
            titled(cell, 0, 3, 55, 17, "[2]─Files");
            titled(cell, 0, 18, 55, 32, "[3]─Branches");
            titled(cell, 0, 33, 55, 46, "[4]─Commits");
            titled(cell, 0, 47, 55, 49, "[5]─Stash");
        }

        let clean = test_grid(168, 51, base);
        let mut retained = None;
        let first = reconcile(&clean, &mut retained);
        let pane_ids: HashSet<u64> = ids(first.layout()).into_iter().map(|item| item.0).collect();
        assert_eq!(pane_ids.len(), 7, "{first}");

        for snapshot in [
            test_grid(168, 51, |cell| {
                base(cell);
                titled(cell, 39, 21, 128, 25, "Command log");
            }),
            test_grid(168, 51, |cell| {
                base(cell);
                titled(cell, 39, 6, 128, 40, "Keybindings");
            }),
        ] {
            let mut retained = Some(first.clone());
            let tree = reconcile(&snapshot, &mut retained);
            let visible_ids: HashSet<u64> =
                ids(tree.layout()).into_iter().map(|item| item.0).collect();
            assert_eq!(visible_ids, pane_ids, "pane cards changed:\n{tree}");
            assert_eq!(tree.overlays().len(), 1, "popup missing:\n{tree}");
        }
    }

    #[test]
    fn first_frame_classifies_floating_command_log() {
        let snapshot = test_grid(168, 51, |cell| {
            titled(cell, 0, 0, 55, 2, "[1]─Status");
            titled(cell, 56, 40, 167, 49, "Command log");
            titled(cell, 39, 21, 128, 25, "Command log");
        });
        let mut retained = None;
        let tree = reconcile(&snapshot, &mut retained);
        assert_eq!(tree.overlays().len(), 1, "{tree}");
        assert_eq!(tree.overlays()[0].x, 39, "{tree}");
        assert!(tree.layout().iter().all(|node| node.x != 39), "{tree}");
    }

    #[test]
    fn contained_popup_keeps_previous_pane_body() {
        let clean = test_grid(30, 16, |cell| {
            titled(cell, 0, 0, 29, 15, "Status");
            write_at(cell, 6, 6, "covered old");
        });
        let mut retained = None;
        reconcile(&clean, &mut retained);

        let covered = test_grid(30, 16, |cell| {
            titled(cell, 0, 0, 29, 15, "Status");
            titled(cell, 4, 3, 25, 12, "Keybindings");
        });
        let tree = reconcile(&covered, &mut retained);
        assert_eq!(tree.layout().len(), 1, "{tree}");
        assert_eq!(tree.overlays().len(), 1, "{tree}");
        assert!(
            body_string(&tree.layout()[0].body).contains("covered old"),
            "covered pane body was replaced:\n{tree}"
        );
    }

    #[test]
    fn popup_does_not_retain_unrelated_removed_pane() {
        let clean = test_grid(32, 16, |cell| {
            titled(cell, 0, 0, 15, 15, "Status");
            titled(cell, 16, 0, 31, 15, "Other");
        });
        let mut retained = None;
        reconcile(&clean, &mut retained);

        let covered = test_grid(32, 16, |cell| {
            titled(cell, 0, 0, 15, 15, "Status");
            titled(cell, 1, 2, 14, 12, "Command log");
        });
        let tree = reconcile(&covered, &mut retained);
        assert_eq!(tree.layout().len(), 1, "removed pane retained:\n{tree}");
        assert_eq!(tree.overlays().len(), 1, "{tree}");
    }

    #[test]
    fn suppression_expands_across_wide_cell_pairs() {
        let snapshot = test_grid(8, 4, |cell| {
            if cell.x == 1 && cell.y == 1 {
                cell.grapheme = "界".to_string();
                cell.occupancy = CellOccupancy::WideLeading;
            }
            if cell.x == 2 && cell.y == 1 {
                cell.grapheme.clear();
                cell.occupancy = CellOccupancy::WideContinuation;
            }
        });
        let tree = LayoutTree {
            cols: 8,
            rows: 4,
            layout: vec![blank_untitled(1, 2, 0, 4, 4, "pane")],
            overlays: Vec::new(),
            chrome: Vec::new(),
            suppress: HashSet::new(),
            history_layout: Vec::new(),
            history_overlays: Vec::new(),
            next_id: 2,
            log_signature: String::new(),
        };
        let suppress = suppress_for(&snapshot, &tree);
        assert!(suppress.contains(&(1, 1)));
        assert!(suppress.contains(&(2, 1)));
    }

    #[test]
    fn a_split_drag_keeps_the_pane() {
        let before = test_grid(32, 12, |cell| {
            stamp(cell, 0, 0, 11, 11);
            stamp(cell, 12, 0, 31, 11);
        });
        let mut retained = None;
        let first = reconcile(&before, &mut retained);
        let right_id = ids(first.layout())
            .iter()
            .find(|item| item.1 == 12)
            .unwrap()
            .0;
        let after = test_grid(32, 12, |cell| {
            stamp(cell, 0, 0, 15, 11);
            stamp(cell, 16, 0, 31, 11);
        });
        let tree = reconcile(&after, &mut retained);
        assert!(
            ids(tree.layout())
                .iter()
                .any(|item| item.0 == right_id && item.1 == 16 && item.3 == 16),
            "{}",
            tree
        );
        assert!(tree.overlays().is_empty(), "{}", tree);
    }

    #[test]
    fn a_grown_stash_keeps_commits_in_the_layout() {
        let before = test_grid(56, 50, |cell| {
            titled(cell, 0, 34, 55, 46, "[4]─Commits");
            titled(cell, 0, 47, 55, 49, "[5]─Stash");
        });
        let mut retained = None;
        let first = reconcile(&before, &mut retained);
        let commits_id = ids(first.layout())
            .into_iter()
            .find(|item| item.2 == 34)
            .unwrap()
            .0;
        let stash_id = ids(first.layout())
            .into_iter()
            .find(|item| item.2 == 47)
            .unwrap()
            .0;
        assert_eq!(
            first
                .layout()
                .iter()
                .find(|node| node.id == commits_id)
                .unwrap()
                .index,
            Some(4),
            "border dash hid the panel number:\n{first}"
        );

        let after = test_grid(56, 50, |cell| {
            titled(cell, 0, 27, 55, 38, "[4]─Commits");
            titled(cell, 0, 39, 55, 49, "[5]─Stash");
        });
        let tree = reconcile(&after, &mut retained);
        let text = tree.to_string();
        assert!(
            tree.overlays().is_empty(),
            "commits became a dialog:\n{text}"
        );
        assert_eq!(tree.layout().len(), 2, "{text}");
        assert!(
            ids(tree.layout())
                .iter()
                .any(|item| { item.0 == commits_id && *item == (commits_id, 0, 27, 56, 12) }),
            "commits lost its pane:\n{text}"
        );
        assert!(
            ids(tree.layout())
                .iter()
                .any(|item| item.0 == stash_id && *item == (stash_id, 0, 39, 56, 11)),
            "stash lost its pane:\n{text}"
        );
    }

    #[test]
    fn a_latched_commits_dialog_returns_to_the_layout() {
        let grown = test_grid(56, 50, |cell| {
            titled(cell, 0, 27, 55, 38, "[4]─Commits");
            titled(cell, 0, 39, 55, 49, "[5]─Stash");
        });
        let mut retained = Some(LayoutTree {
            cols: 56,
            rows: 50,
            layout: vec![
                blank_node(12, 0, 39, 56, 11, "[5]Stash", 5),
                blank_node(14, 0, 47, 56, 3, "[5]Stash", 5),
            ],
            overlays: vec![blank_node(19, 0, 27, 56, 12, "[4]Commits", 4)],
            chrome: Vec::new(),
            suppress: HashSet::new(),
            history_layout: Vec::new(),
            history_overlays: Vec::new(),
            next_id: 20,
            log_signature: String::new(),
        });
        let tree = reconcile(&grown, &mut retained);
        let text = tree.to_string();
        assert!(
            tree.overlays().is_empty(),
            "commits stayed a dialog:\n{text}"
        );
        assert!(
            ids(tree.layout())
                .iter()
                .any(|item| *item == (19, 0, 27, 56, 12)),
            "commits dropped its id:\n{text}"
        );
        assert!(
            ids(tree.layout())
                .iter()
                .any(|item| *item == (12, 0, 39, 56, 11)),
            "live stash dropped:\n{text}"
        );
        assert!(!text.contains("56×3"), "short stash was kept:\n{text}");
    }

    #[test]
    fn focusing_command_log_drops_the_popup() {
        let focused = test_grid(168, 50, |cell| {
            titled(cell, 0, 0, 55, 2, "[1]─Status");
            titled(cell, 56, 0, 167, 48, "Command log");
            titled(cell, 0, 3, 55, 17, "[2]─Files");
            titled(cell, 0, 18, 55, 31, "[3]─Branches");
            titled(cell, 0, 32, 55, 45, "[4]─Commits");
            titled(cell, 0, 46, 55, 48, "[5]─Stash");
        });
        let mut retained = Some(LayoutTree {
            cols: 168,
            rows: 50,
            layout: vec![
                blank_node(1, 0, 0, 56, 3, "[1]Status", 1),
                blank_untitled(2, 56, 0, 112, 49, "Command log"),
                blank_node(3, 0, 3, 56, 15, "[2]Files", 2),
                blank_node(4, 0, 18, 56, 14, "[3]Branches", 3),
                blank_node(5, 0, 32, 56, 14, "[4]Commits", 4),
                blank_untitled(6, 56, 39, 112, 10, "Command log"),
                blank_node(7, 0, 46, 56, 3, "[5]Stash", 5),
            ],
            overlays: vec![blank_untitled(9, 39, 21, 90, 5, "Command log")],
            chrome: Vec::new(),
            suppress: HashSet::new(),
            history_layout: Vec::new(),
            history_overlays: Vec::new(),
            next_id: 10,
            log_signature: String::new(),
        });
        let tree = reconcile(&focused, &mut retained);
        let text = tree.to_string();
        assert!(
            tree.overlays().is_empty(),
            "command log popup stayed:\n{text}"
        );
        assert!(
            !text.contains("@56,39"),
            "short command log ghost stayed:\n{text}"
        );
        assert!(
            ids(tree.layout())
                .iter()
                .any(|item| item.0 == 2 && *item == (2, 56, 0, 112, 49)),
            "focused command log lost its id:\n{text}"
        );
        assert_eq!(
            tree.layout()
                .iter()
                .filter(|node| node.title.as_deref() == Some("Command log"))
                .count(),
            1,
            "duplicate command log panes:\n{text}"
        );
    }

    fn titled(cell: &mut crate::terminal::Cell, x0: u16, y0: u16, x1: u16, y1: u16, title: &str) {
        stamp(cell, x0, y0, x1, y1);
        write_at(cell, x0 + 1, y0, title);
    }

    fn blank_node(id: u64, x: u16, y: u16, cols: u16, rows: u16, title: &str, index: u16) -> Node {
        Node {
            id,
            x,
            y,
            cols,
            rows,
            title: Some(title.to_string()),
            index: Some(index),
            scroll: None,
            thumb: None,
            border: Color::Default,
            body: PaneBody::Empty,
            children: Vec::new(),
        }
    }

    fn blank_untitled(id: u64, x: u16, y: u16, cols: u16, rows: u16, title: &str) -> Node {
        let mut node = blank_node(id, x, y, cols, rows, title, 0);
        node.index = None;
        node
    }

    #[test]
    fn header_suppresses_raw_title_and_scroll_label() {
        let title = "[2] Files - Worktrees - Submodules";
        let snap = test_grid(40, 10, |cell| {
            stamp(cell, 0, 0, 39, 9);
            write_at(cell, 1, 0, title);
            write_at(cell, 30, 9, "9 of 24");
        });
        let tree = parse(&snap);
        let text = tree.to_string();
        assert!(
            text.contains("\"[2] Files - Worktrees - Submodules\" 9/24"),
            "{text}"
        );
        let node = &tree.layout()[0];
        assert_eq!(node.index, Some(2));
        assert!(tree.suppress().contains(&(5, 0)), "raw title kept");
        assert!(tree.suppress().contains(&(5, 5)), "raw body kept");
        assert!(tree.suppress().contains(&(36, 0)), "border dash kept");
        assert!(tree.suppress().contains(&(30, 9)), "scroll label kept");
        assert!(tree.suppress().contains(&(36, 9)), "scroll label kept");
    }

    #[test]
    fn right_edge_block_thumb_is_parsed() {
        let snap = test_grid(20, 12, |cell| {
            stamp(cell, 0, 0, 19, 11);
            write_at(cell, 2, 0, "Commits");
            if cell.x == 19 && (4..=7).contains(&cell.y) {
                cell.grapheme = "▐".to_string();
            }
        });
        let tree = parse(&snap);
        let thumb = node_thumb(&tree.layout()[0]).expect("thumb");
        assert_eq!(thumb.start, 4, "{tree}");
        assert_eq!(thumb.end, 7, "{tree}");
        assert_eq!(thumb.track_top, 1);
        assert_eq!(thumb.track_bottom, 10);
        assert!(tree.to_string().contains("thumb 4-7"), "{tree}");
    }

    #[test]
    fn full_edge_block_stays_raw() {
        let snap = test_grid(12, 8, |cell| {
            stamp(cell, 0, 0, 11, 7);
            if cell.x == 11 && (1..=6).contains(&cell.y) {
                cell.grapheme = "▐".to_string();
            }
        });
        let tree = parse(&snap);
        assert_eq!(tree.node_count(), 0, "{tree}");
        assert!(tree.suppress().is_empty());
    }

    #[test]
    fn recolored_bar_run_is_a_thumb() {
        let snap = test_grid(16, 10, |cell| {
            stamp(cell, 0, 0, 15, 9);
            if cell.x == 15 && (3..=5).contains(&cell.y) {
                cell.fg = Color::Indexed(15);
            }
        });
        let tree = parse(&snap);
        let thumb = node_thumb(&tree.layout()[0]).expect("thumb");
        assert_eq!(thumb.start, 3, "{tree}");
        assert_eq!(thumb.end, 5, "{tree}");
    }

    #[test]
    fn full_edge_recolor_stays_raw() {
        let snap = test_grid(12, 8, |cell| {
            stamp(cell, 0, 0, 11, 7);
            if cell.x == 11 && (1..=6).contains(&cell.y) {
                cell.fg = Color::Indexed(15);
            }
        });
        let tree = parse(&snap);
        assert_eq!(tree.node_count(), 0, "{tree}");
        assert!(tree.suppress().is_empty());
    }

    #[test]
    fn log_shows_the_grid_and_all_three_trees() {
        let snap = test_grid(8, 4, |cell| {
            stamp(cell, 0, 0, 7, 3);
            write_at(cell, 2, 0, "Files");
        });
        let tree = parse(&snap);
        let text = tree.to_string();
        let previous = "Screen 8×4\nprevious-tree\n";
        let log = frame_log(&snap, previous, &text, &text);
        assert!(log.contains("terminal 8×4\n"), "{log}");
        assert!(log.contains(&format!("previous\n{previous}")), "{log}");
        assert!(log.contains(&format!("new\n{text}")), "{log}");
        assert!(log.contains(&format!("merged\n{text}")), "{log}");
        assert!(log.contains("   0|┌─Files"), "{log}");
    }

    #[test]
    fn reconcile_logs_by_default() {
        let snap = test_grid(8, 4, |cell| {
            stamp(cell, 0, 0, 7, 3);
            write_at(cell, 2, 0, "Files");
        });
        let mut retained = None;
        let first = reconcile(&snap, &mut retained);
        let second = reconcile(&snap, &mut retained);
        assert_eq!(first.layout().len(), 1, "{first}");
        assert_eq!(second.layout().len(), 1, "{second}");
    }

    #[test]
    fn pane_body_is_the_interior_not_the_border() {
        let snap = test_grid(16, 8, |cell| {
            stamp(cell, 0, 0, 15, 6);
            write_at(cell, 2, 0, "Files");
            write_at(cell, 2, 2, "main.rs");
            write_at(cell, 2, 3, "lib.rs");
            if cell.y == 3 && (2..8).contains(&cell.x) {
                cell.style.reverse = true;
            }
            write_at(cell, 0, 7, "ready");
        });
        let tree = parse(&snap);
        let node = &tree.layout()[0];
        let PaneBody::Rows(lines) = &node.body else {
            panic!("expected rows: {node:?}");
        };
        let text = lines
            .iter()
            .map(|line| line_text(line))
            .collect::<Vec<_>>()
            .join("\n");
        assert!(text.contains("main.rs"), "{text}");
        assert!(text.contains("lib.rs"), "{text}");
        assert!(!text.contains("Files"), "{text}");
        assert!(!text.contains('┌') && !text.contains('─'), "{text}");
        assert!(lines
            .iter()
            .any(|line| line.selected && line_text(line).contains("lib.rs")));
        assert!(lines
            .iter()
            .any(|line| !line.selected && line_text(line).contains("main.rs")));
        assert!(tree
            .chrome()
            .iter()
            .any(|row| line_text(&row.line).contains("ready")));
        assert!(tree.suppress().contains(&(0, 7)));
        assert!(tree.suppress().contains(&(15, 7)));
        assert!(!text.contains("ready"), "{text}");
        assert!(tree.to_string().contains("rows 2"), "{}", tree);
    }

    #[test]
    fn a_parent_does_not_repeat_its_child() {
        let snap = test_grid(30, 16, |cell| {
            stamp(cell, 0, 0, 29, 15);
            stamp(cell, 4, 3, 20, 12);
            write_at(cell, 1, 1, "OUTER");
            write_at(cell, 6, 5, "INNER");
        });
        let tree = parse(&snap);
        assert_eq!(tree.layout().len(), 1, "{}", tree);
        let parent = &tree.layout()[0];
        let child = &parent.children[0];
        let parent_text = body_string(&parent.body);
        let child_text = body_string(&child.body);
        assert!(parent_text.contains("OUTER"), "{parent_text}");
        assert!(!parent_text.contains("INNER"), "{parent_text}");
        assert!(child_text.contains("INNER"), "{child_text}");
        assert!(!child_text.contains("OUTER"), "{child_text}");
    }

    #[test]
    fn a_dialog_is_not_copied_into_the_pane() {
        let clean = test_grid(32, 12, |cell| {
            stamp(cell, 0, 0, 15, 11);
            stamp(cell, 16, 0, 31, 11);
            write_at(cell, 2, 2, "pane line");
        });
        let mut retained = None;
        reconcile(&clean, &mut retained);
        let covered = test_grid(32, 12, |cell| {
            stamp(cell, 0, 0, 15, 11);
            stamp(cell, 16, 0, 31, 11);
            write_at(cell, 2, 2, "pane line");
            stamp(cell, 8, 3, 24, 9);
            write_at(cell, 10, 5, "popup");
        });
        let tree = reconcile(&covered, &mut retained);
        let dialog = body_string(&tree.overlays()[0].body);
        assert!(
            tree.layout().len() == 2,
            "occluded pane cards disappeared:\n{tree}"
        );
        assert!(dialog.contains("popup"), "{dialog}\n{tree}");
        assert!(
            tree.layout()
                .iter()
                .any(|node| body_string(&node.body).contains("pane line")),
            "retained pane body was discarded"
        );
        assert!(
            tree.chrome()
                .iter()
                .all(|row| !line_text(&row.line).contains("pane line")),
            "underlying pane text became chrome"
        );
        assert!(
            tree.suppress().contains(&(2, 2)),
            "underlying pane text was not replaced"
        );
    }

    #[test]
    fn a_diff_pane_is_text() {
        let snap = test_grid(24, 10, |cell| {
            stamp(cell, 0, 0, 23, 9);
            write_at(cell, 1, 1, "@@ -1,2 +1,2 @@");
            write_at(cell, 1, 2, "-old");
            write_at(cell, 1, 3, "+new");
            write_at(cell, 1, 4, " context");
        });
        let tree = parse(&snap);
        assert!(
            matches!(tree.layout()[0].body, PaneBody::Text(_)),
            "{}",
            tree
        );
        assert!(tree.to_string().contains(" text "), "{}", tree);
    }

    fn body_string(body: &PaneBody) -> String {
        let lines = match body {
            PaneBody::Empty => return String::new(),
            PaneBody::Rows(lines) | PaneBody::Text(lines) => lines,
        };
        lines
            .iter()
            .map(|line| line_text(line))
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn write_at(cell: &mut crate::terminal::Cell, x: u16, y: u16, text: &str) {
        if cell.y != y || cell.x < x {
            return;
        }
        if let Some(ch) = text.chars().nth((cell.x - x) as usize) {
            cell.grapheme = ch.to_string();
        }
    }
}
