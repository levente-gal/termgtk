//! Herdr CLI and NDJSON. Nothing outside this module starts Herdr commands.

pub mod input;
pub mod snapshot;
pub mod terminal_stream;

pub use input::{encode_key, Key, KeyEvent, KeyInput};
pub use snapshot::{
    check_protocol_gate, discover_focused_pane, discover_snapshot, fetch_snapshot, focus_agent,
    focus_agent_argv, focus_pane_direction, focus_pane_direction_argv, focus_workspace,
    focus_workspace_argv, tab_layout, AgentRow, AgentStatus, FocusedPane, HerdrError,
    HerdrSnapshot, LayoutNode, PaneDirection, PlacedPane, SplitDirection, SplitRatio, TabLayout,
    WorkspaceRow, EXPECTED_HERDR_VERSION, EXPECTED_SCHEMA_SHA256,
};
pub use terminal_stream::{ControlSession, ObserveSession, ScrollDirection, ScrollSource};
