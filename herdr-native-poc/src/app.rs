//! Native application shell. Integrators replace placeholder labels and status.

use std::collections::HashMap;
use std::sync::Mutex;

use anyhow::Result;

use crate::terminal::{Cell, Color, Cursor, TerminalSnapshot};

/// Session presentation state supplied by the integrator.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SessionStatus {
    Live,
    /// Connected mirror. Keyboard input is not sent.
    ReadOnly,
    /// This process owns the pane via `control --takeover`.
    Controlling,
    Unavailable(String),
    ChangedPane,
    Disconnected(String),
    Resyncing,
    Controller(String),
    /// Focus command failed or the refreshed workspace id did not match.
    /// The current stream stays up.
    FocusError(String),
}

/// Header / debug placeholders the integrator can replace at runtime.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShellState {
    pub workspace_label: String,
    pub pane_label: String,
    pub title: String,
    pub pane_id: String,
    pub frame_count: u64,
    pub detection_count: u64,
    pub status: SessionStatus,
}

impl Default for ShellState {
    fn default() -> Self {
        Self {
            workspace_label: "Workspace".to_string(),
            pane_label: "Pane".to_string(),
            title: "Herdr Native".to_string(),
            pane_id: "fixture-pane".to_string(),
            frame_count: 1,
            detection_count: 0,
            status: SessionStatus::Live,
        }
    }
}

impl SessionStatus {
    /// Banner copy for every non-`Live` status. `Live` has no banner.
    pub fn banner_text(&self) -> Option<String> {
        match self {
            Self::Live | Self::Controlling => None,
            Self::ReadOnly => Some("Read-only mirror of this pane. Take over to type.".to_string()),
            Self::Unavailable(detail) => Some(format!("Herdr unavailable: {detail}")),
            Self::ChangedPane => Some("Focused pane changed or disappeared".to_string()),
            Self::Disconnected(reason) => Some(format!("Terminal session disconnected: {reason}")),
            Self::Resyncing => Some("Resyncing terminal session…".to_string()),
            Self::Controller(detail) => Some(format!("Controller error: {detail}")),
            Self::FocusError(detail) => Some(detail.clone()),
        }
    }
}

impl ShellState {
    pub fn banner_text(&self) -> Option<String> {
        self.status.banner_text()
    }

    pub fn debug_overlay_enabled() -> bool {
        std::env::var("HERDR_NATIVE_DEBUG").ok().as_deref() == Some("1")
    }

    pub fn format_debug_overlay(
        &self,
        cols: u16,
        rows: u16,
        cell_width: f64,
        cell_height: f64,
        scale: f64,
    ) -> String {
        format!(
            "pane_id={}\nrows={} cols={}\ncell={:.2}x{:.2}\nscale={:.3}\nframes={}\ndetections={}",
            self.pane_id,
            rows,
            cols,
            cell_width,
            cell_height,
            scale,
            self.frame_count,
            self.detection_count,
        )
    }

    /// Small in-memory fixture so the card is not empty. Includes a separator and a box.
    pub fn fixture_snapshot() -> TerminalSnapshot {
        let cols = 10u16;
        let rows = 5u16;
        let mut cells = Vec::with_capacity(usize::from(cols) * usize::from(rows));
        for y in 0..rows {
            for x in 0..cols {
                cells.push(Cell::blank(x, y));
            }
        }
        Self::paint_row(&mut cells, cols, 0, "Hello");
        Self::paint_row(&mut cells, cols, 1, "──────────");
        Self::paint_row(&mut cells, cols, 2, "┌────┐");
        Self::paint_row(&mut cells, cols, 3, "│demo│");
        Self::paint_row(&mut cells, cols, 4, "└────┘");
        TerminalSnapshot::try_new(cols, rows, Cursor::hidden(), cells)
            .expect("fixture snapshot is well-formed")
    }

    fn paint_row(cells: &mut [Cell], cols: u16, y: u16, text: &str) {
        for (i, ch) in text.chars().enumerate() {
            let x = i as u16;
            if x >= cols {
                break;
            }
            let index = usize::from(y) * usize::from(cols) + usize::from(x);
            cells[index].grapheme = ch.to_string();
            cells[index].fg = Color::Default;
            cells[index].bg = Color::Default;
        }
    }

    pub fn header_title(&self) -> String {
        format!("{} · {}", self.workspace_label, self.pane_label)
    }
}

/// Latest painted frame published by the observe worker for the GTK idle callback.
#[derive(Clone, Debug)]
pub struct LiveFrame {
    pub pane_id: String,
    pub generation: u64,
    pub snapshot: TerminalSnapshot,
    pub frame_count: u64,
    pub detection_count: u64,
}

/// Store `frame` when its generation is not older than the slot's current value.
///
/// Newer (or equal) generations replace the slot; older generations are ignored.
#[cfg_attr(
    not(any(test, all(feature = "gtk", feature = "ghostty"))),
    allow(dead_code)
)]
fn store_live_frame(slot: &Mutex<HashMap<String, LiveFrame>>, frame: LiveFrame) -> bool {
    let mut guard = slot.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(prev) = guard.get(&frame.pane_id) {
        if frame.generation < prev.generation {
            return false;
        }
    }
    guard.insert(frame.pane_id.clone(), frame);
    true
}

#[cfg_attr(not(all(feature = "gtk", feature = "ghostty")), allow(dead_code))]
fn status_clears_grid(status: &SessionStatus) -> bool {
    matches!(
        status,
        SessionStatus::Resyncing
            | SessionStatus::Disconnected(_)
            | SessionStatus::ChangedPane
            | SessionStatus::Unavailable(_)
    )
}

#[cfg_attr(not(feature = "gtk"), allow(dead_code))]
fn set_status(status: &mut SessionStatus, next: SessionStatus) {
    if *status != next {
        tracing::info!(?next, "session status changed");
        *status = next;
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(
    not(any(test, all(feature = "gtk", feature = "ghostty"))),
    allow(dead_code)
)]
struct GridSize {
    cols: u16,
    rows: u16,
    cell_w: u32,
    cell_h: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(
    not(any(test, all(feature = "gtk", feature = "ghostty"))),
    allow(dead_code)
)]
enum FrameFit {
    Use,
    Acknowledged,
    Ignore,
}

/// One in-flight Herdr resize and at most one newer target.
#[cfg_attr(
    not(any(test, all(feature = "gtk", feature = "ghostty"))),
    allow(dead_code)
)]
struct ResizeSync {
    confirmed: GridSize,
    inflight: Option<GridSize>,
    pending: Option<GridSize>,
}

impl ResizeSync {
    fn new(grid: GridSize) -> Self {
        Self {
            confirmed: GridSize {
                cols: grid.cols.max(1),
                rows: grid.rows.max(1),
                ..grid
            },
            inflight: None,
            pending: None,
        }
    }

    fn confirmed(&self) -> (u16, u16) {
        (self.confirmed.cols, self.confirmed.rows)
    }

    /// Queue `grid`. Returns the size to send now when nothing is in flight.
    fn note_desired(&mut self, grid: GridSize) -> Option<GridSize> {
        let grid = GridSize {
            cols: grid.cols.max(1),
            rows: grid.rows.max(1),
            ..grid
        };
        if self.inflight == Some(grid) {
            self.pending = None;
            return None;
        }
        if self.inflight.is_some() {
            self.pending = Some(grid);
            return None;
        }
        if self.confirmed == grid {
            self.pending = None;
            return None;
        }
        self.inflight = Some(grid);
        Some(grid)
    }

    fn on_frame(&mut self, cols: u16, rows: u16) -> FrameFit {
        let size = (cols, rows);
        if self
            .inflight
            .is_some_and(|target| (target.cols, target.rows) == size)
        {
            self.confirmed = self.inflight.take().expect("matching in-flight resize");
            return FrameFit::Acknowledged;
        }
        if size == (self.confirmed.cols, self.confirmed.rows) {
            return FrameFit::Use;
        }
        if self.inflight.is_some() {
            return FrameFit::Ignore;
        }
        self.confirmed.cols = cols;
        self.confirmed.rows = rows;
        FrameFit::Use
    }

    /// Drop the in-flight size. A different queued target may be sent next.
    fn reject_inflight(&mut self) -> Option<GridSize> {
        let rejected = self.inflight.take();
        let pending = self.pending.take()?;
        if Some(pending) == rejected || pending == self.confirmed {
            return None;
        }
        self.inflight = Some(pending);
        Some(pending)
    }

    fn next_pending(&mut self) -> Option<GridSize> {
        if self.inflight.is_some() {
            return None;
        }
        let pending = self.pending.take()?;
        if pending == self.confirmed {
            return None;
        }
        self.inflight = Some(pending);
        Some(pending)
    }
}

#[cfg_attr(
    not(any(test, all(feature = "gtk", feature = "ghostty"))),
    allow(dead_code)
)]
fn control_reject_is_resize(detail: &str) -> bool {
    detail.contains("resize")
}

/// Size passed to the first `herdr terminal session observe` or `control`.
///
/// `None` until the card reports a positive cols×rows. The 80×24 placeholder
/// must not be passed in here.
#[cfg_attr(
    not(any(test, all(feature = "gtk", feature = "ghostty"))),
    allow(dead_code)
)]
fn observe_spawn_size(capacity: Option<GridSize>) -> Option<GridSize> {
    let grid = capacity?;
    if grid.cols == 0 || grid.rows == 0 {
        return None;
    }
    Some(grid)
}

/// What to do with one observe frame relative to the size just requested.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(
    not(any(test, all(feature = "gtk", feature = "ghostty"))),
    allow(dead_code)
)]
enum ObserveFit {
    /// Cols and rows match the request. Safe to paint.
    Draw,
    /// Incremental frame at some other size. Do not paint it.
    Skip,
    /// Full frame whose width/height are not the request. Surface a disconnect.
    Disconnect,
}

#[cfg_attr(
    not(any(test, all(feature = "gtk", feature = "ghostty"))),
    allow(dead_code)
)]
fn classify_observe_frame(full: bool, width: u16, height: u16, cols: u16, rows: u16) -> ObserveFit {
    if width == cols && height == rows {
        ObserveFit::Draw
    } else if full {
        ObserveFit::Disconnect
    } else {
        ObserveFit::Skip
    }
}

/// What a finished snapshot request means for the attached pane.
#[derive(Clone, Debug, PartialEq, Eq)]
enum SnapshotResult {
    /// `herdr workspace focus` failed before a snapshot was accepted.
    FocusCommandFailed,
    /// Focus succeeded; this snapshot is the refresh that follows it.
    FocusSnapshot {
        generation: u64,
        requested_workspace_id: String,
        focused_workspace_id: Option<String>,
        focused_pane_id: Option<String>,
    },
    /// Slow timer refresh. Applied only when `generation` is still newest.
    Refresh {
        generation: u64,
        focused_pane_id: Option<String>,
    },
    /// `herdr agent focus` succeeded; this snapshot is the refresh that follows it.
    AgentFocusSnapshot {
        generation: u64,
        requested_pane_id: String,
        focused_pane_id: Option<String>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum AttachmentChange {
    /// Keep the current pane id and stream.
    Unchanged,
    /// Drop the stream and observe this pane read-only.
    Reconnect(String),
    /// The focused workspace has no pane. Stop the stream.
    Disconnect,
}

/// Apply a snapshot result only when its generation is still the newest request.
///
/// A failed focus, a mismatched workspace id, and an older timer snapshot leave
/// the pane id unchanged (`Unchanged`).
fn resolve_attachment(
    newest_generation: u64,
    pane_id: &str,
    result: SnapshotResult,
) -> AttachmentChange {
    match result {
        SnapshotResult::FocusCommandFailed => AttachmentChange::Unchanged,
        SnapshotResult::Refresh {
            generation,
            focused_pane_id,
            ..
        } => {
            if generation != newest_generation {
                return AttachmentChange::Unchanged;
            }
            match focused_pane_id {
                Some(id) if id != pane_id => AttachmentChange::Reconnect(id),
                _ => AttachmentChange::Unchanged,
            }
        }
        SnapshotResult::FocusSnapshot {
            generation,
            requested_workspace_id,
            focused_workspace_id,
            focused_pane_id,
        } => {
            if generation != newest_generation {
                return AttachmentChange::Unchanged;
            }
            if focused_workspace_id.as_deref() != Some(requested_workspace_id.as_str()) {
                return AttachmentChange::Unchanged;
            }
            match focused_pane_id {
                Some(id) => AttachmentChange::Reconnect(id),
                None => AttachmentChange::Disconnect,
            }
        }
        SnapshotResult::AgentFocusSnapshot {
            generation,
            requested_pane_id,
            focused_pane_id,
        } => {
            if generation != newest_generation {
                return AttachmentChange::Unchanged;
            }
            match focused_pane_id {
                Some(id) if id == requested_pane_id => AttachmentChange::Reconnect(id),
                Some(_) => AttachmentChange::Unchanged,
                None => AttachmentChange::Disconnect,
            }
        }
    }
}

#[cfg_attr(not(all(feature = "gtk", feature = "ghostty")), allow(dead_code))]
fn focus_race_message(requested: &str, actual: Option<&str>) -> String {
    match actual {
        Some(actual) => format!("Focus raced: workspace is {actual}, not {requested}"),
        None => format!("Focus raced: workspace {requested} is not focused"),
    }
}

#[cfg_attr(not(all(feature = "gtk", feature = "ghostty")), allow(dead_code))]
fn agent_focus_race_message(requested: &str, actual: Option<&str>) -> String {
    match actual {
        Some(actual) => format!("Focus raced: pane is {actual}, not {requested}"),
        None => format!("Focus raced: pane {requested} is not focused"),
    }
}

pub fn run() -> Result<()> {
    #[cfg(not(feature = "gtk"))]
    {
        anyhow::bail!(
            "GTK4 development files are not linked. Install gtk4-devel, libadwaita-devel, \
             pango-devel, glib2-devel, cairo-devel, cairo-gobject-devel, graphene-devel, \
             gobject-introspection-devel, and pkgconf-pkg-config, then rebuild with \
             `--features gtk`"
        );
    }
    #[cfg(feature = "gtk")]
    {
        run_gtk()
    }
}

#[cfg(feature = "gtk")]
fn run_gtk() -> Result<()> {
    use adw::prelude::*;

    let app = adw::Application::builder()
        .application_id("dev.herdr.native-poc")
        .build();

    app.connect_activate(|app| {
        ShellState::default().present(app);
    });

    let code = app.run();
    if code != gtk::glib::ExitCode::SUCCESS {
        anyhow::bail!("application exited with {code:?}");
    }
    Ok(())
}

#[cfg(feature = "gtk")]
fn apply_banner(banner: &adw::Banner, state: &ShellState) {
    match state.banner_text() {
        Some(text) => {
            banner.set_title(&text);
            banner.set_revealed(true);
        }
        None => banner.set_revealed(false),
    }
}

#[cfg(all(feature = "gtk", feature = "ghostty"))]
fn apply_header(
    header_label: &gtk::Label,
    window: &adw::ApplicationWindow,
    chrome: &crate::ui::ShellChrome,
    state: &ShellState,
) {
    use adw::prelude::*;
    header_label.set_label(&state.header_title());
    window.set_title(Some(&state.title));
    let title = state.title.trim();
    chrome.set_pane_header((!title.is_empty()).then_some(title));
}

#[cfg(all(feature = "gtk", feature = "ghostty"))]
fn apply_focused_pane(state: &mut ShellState, pane: &crate::herdr::FocusedPane) {
    state.workspace_label = pane
        .workspace_label
        .clone()
        .unwrap_or_else(|| pane.workspace_id.clone());
    state.pane_label = pane.pane_id.clone();
    state.pane_id = pane.pane_id.clone();
    state.title = pane
        .terminal_title
        .clone()
        .unwrap_or_else(|| "Herdr Native".to_string());
}

#[cfg(feature = "gtk")]
impl ShellState {
    fn present(self, app: &adw::Application) {
        use crate::render::TerminalView;
        use crate::ui::{ShellChrome, Theme};
        use adw::prelude::*;
        use std::sync::Arc;

        let state = Arc::new(Mutex::new(self));

        if let Some(display) = gtk::gdk::Display::default() {
            Theme::install_for_display(&display);
        }

        let terminal = TerminalView::new();
        terminal.set_snapshot(ShellState::fixture_snapshot());

        let chrome = Arc::new(ShellChrome::new(terminal));
        let debug_on = ShellState::debug_overlay_enabled();
        chrome.set_debug_visible(debug_on);
        chrome.refresh_debug(&state.lock().unwrap_or_else(|e| e.into_inner()));

        let enhance_label = gtk::Label::new(Some("Enhance TUI rendering"));
        enhance_label.set_halign(gtk::Align::Start);
        enhance_label.set_hexpand(true);

        let enhance_switch = gtk::Switch::builder().valign(gtk::Align::Center).build();

        let enhance_row = gtk::Box::new(gtk::Orientation::Horizontal, 12);
        enhance_row.set_margin_start(12);
        enhance_row.set_margin_end(12);
        enhance_row.set_margin_top(8);
        enhance_row.set_margin_bottom(8);
        enhance_row.append(&enhance_label);
        enhance_row.append(&enhance_switch);

        let banner = adw::Banner::new("");
        apply_banner(&banner, &state.lock().unwrap_or_else(|e| e.into_inner()));

        let header_label = gtk::Label::builder()
            .label(
                state
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .header_title(),
            )
            .css_classes(["title"])
            .build();

        let header = adw::HeaderBar::new();
        header.set_title_widget(Some(&header_label));

        let takeover_button = gtk::Button::with_label("Take over");
        takeover_button.set_sensitive(false);
        takeover_button.set_tooltip_text(Some("Take keyboard ownership of this pane"));
        header.pack_start(&takeover_button);

        let body = gtk::Box::new(gtk::Orientation::Vertical, 0);
        body.add_css_class(Theme::CSS_BACKGROUND_CLASS);
        body.set_hexpand(true);
        body.set_vexpand(true);
        body.append(&banner);
        body.append(&enhance_row);
        body.append(chrome.root());

        let shell = gtk::Box::new(gtk::Orientation::Vertical, 0);
        shell.add_css_class(Theme::CSS_BACKGROUND_CLASS);
        shell.set_hexpand(true);
        shell.set_vexpand(true);
        shell.append(&header);
        shell.append(&body);

        let window = adw::ApplicationWindow::builder()
            .application(app)
            .title(&state.lock().unwrap_or_else(|e| e.into_inner()).title)
            .content(&shell)
            .default_width(1100)
            .default_height(600)
            .build();
        window.set_size_request(520, 240);
        window.add_css_class(Theme::CSS_BACKGROUND_CLASS);

        let action = gio::SimpleAction::new_stateful("toggle-enhance", None, &false.to_variant());
        let chrome_for_action = Arc::clone(&chrome);
        let switch_for_action = enhance_switch.clone();
        action.connect_activate(move |action, _| {
            let current = action
                .state()
                .and_then(|v| v.get::<bool>())
                .unwrap_or(false);
            let next = !current;
            action.set_state(&next.to_variant());
            chrome_for_action.set_enhanced(next);
            if switch_for_action.is_active() != next {
                switch_for_action.set_active(next);
            }
        });
        app.add_action(&action);
        app.set_accels_for_action("app.toggle-enhance", &["<Control><Shift>e"]);

        let action_for_switch = action.clone();
        let chrome_for_switch = Arc::clone(&chrome);
        enhance_switch.connect_active_notify(move |switch| {
            let active = switch.is_active();
            chrome_for_switch.set_enhanced(active);
            let current = action_for_switch
                .state()
                .and_then(|v| v.get::<bool>())
                .unwrap_or(false);
            if current != active {
                action_for_switch.set_state(&active.to_variant());
            }
        });

        if debug_on {
            let chrome_w = Arc::clone(&chrome);
            let state_w = Arc::clone(&state);
            chrome
                .host()
                .connect_notify_local(Some("width"), move |_, _| {
                    chrome_w.refresh_debug(&state_w.lock().unwrap_or_else(|e| e.into_inner()));
                });
            let chrome_h = Arc::clone(&chrome);
            let state_h = Arc::clone(&state);
            chrome
                .host()
                .connect_notify_local(Some("height"), move |_, _| {
                    chrome_h.refresh_debug(&state_h.lock().unwrap_or_else(|e| e.into_inner()));
                });
        }

        #[cfg(all(feature = "gtk", not(feature = "ghostty")))]
        {
            let _ = takeover_button;
            {
                let mut borrowed = state.lock().unwrap_or_else(|e| e.into_inner());
                set_status(
                    &mut borrowed.status,
                    SessionStatus::Unavailable("rebuild with `--features gtk,ghostty`".to_string()),
                );
            }
            apply_banner(&banner, &state.lock().unwrap_or_else(|e| e.into_inner()));
            chrome.refresh_debug(&state.lock().unwrap_or_else(|e| e.into_inner()));
        }

        #[cfg(all(feature = "gtk", feature = "ghostty"))]
        {
            start_live_observe(
                Arc::clone(&state),
                Arc::clone(&chrome),
                header_label.clone(),
                banner.clone(),
                window.clone(),
                takeover_button,
            );
        }

        window.present();
    }
}

/// GTK widgets captured by the observe worker. Touched only on the main thread.
#[cfg(all(feature = "gtk", feature = "ghostty"))]
struct UiSurface {
    chrome: std::sync::Arc<crate::ui::ShellChrome>,
    header_label: gtk::Label,
    banner: adw::Banner,
    window: adw::ApplicationWindow,
    takeover: gtk::Button,
}

#[cfg(all(feature = "gtk", feature = "ghostty"))]
fn apply_takeover_button(button: &gtk::Button, status: &SessionStatus) {
    use gtk::prelude::*;
    match status {
        SessionStatus::Controlling => {
            button.set_label("Controlling");
            button.set_sensitive(false);
        }
        SessionStatus::ReadOnly | SessionStatus::FocusError(_) => {
            button.set_label("Take over");
            button.set_sensitive(true);
        }
        SessionStatus::Resyncing => {
            button.set_sensitive(false);
        }
        SessionStatus::Live
        | SessionStatus::Unavailable(_)
        | SessionStatus::ChangedPane
        | SessionStatus::Disconnected(_)
        | SessionStatus::Controller(_) => {
            button.set_label("Take over");
            button.set_sensitive(false);
        }
    }
}

/// Moves a main-thread value back onto the GTK thread before dropping it.
#[cfg(all(feature = "gtk", feature = "ghostty"))]
struct ReleaseOnMain<T: Send + 'static>(Option<T>);

#[cfg(all(feature = "gtk", feature = "ghostty"))]
impl<T: Send + 'static> ReleaseOnMain<T> {
    fn new(value: T) -> Self {
        Self(Some(value))
    }

    fn get(&self) -> &T {
        self.0.as_ref().expect("main-thread value")
    }
}

#[cfg(all(feature = "gtk", feature = "ghostty"))]
impl<T: Send + 'static> Drop for ReleaseOnMain<T> {
    fn drop(&mut self) {
        if let Some(value) = self.0.take() {
            glib::idle_add_once(move || drop(value));
        }
    }
}

#[cfg(all(feature = "gtk", feature = "ghostty"))]
const WORKSPACE_POLL_SECS: u32 = 2;

#[cfg(all(feature = "gtk", feature = "ghostty"))]
struct ChromeUpdate {
    generation: u64,
    workspaces: Vec<crate::herdr::WorkspaceRow>,
    focused_workspace_id: Option<String>,
    agents: Vec<crate::herdr::AgentRow>,
    focused_pane_id: Option<String>,
    pane: Option<crate::herdr::FocusedPane>,
    layout: Option<crate::herdr::TabLayout>,
}

#[cfg(all(feature = "gtk", feature = "ghostty"))]
fn store_chrome_update(slot: &Mutex<Option<ChromeUpdate>>, update: ChromeUpdate) -> bool {
    let mut guard = slot.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(prev) = guard.as_ref() {
        if update.generation < prev.generation {
            return false;
        }
    }
    *guard = Some(update);
    true
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg(all(feature = "gtk", feature = "ghostty"))]
enum StreamFollow {
    Stay,
    Restart,
    Stop,
}

#[cfg(all(feature = "gtk", feature = "ghostty"))]
struct FocusResolution {
    follow: StreamFollow,
    status: Option<SessionStatus>,
    generation: u64,
    chrome: Option<crate::herdr::HerdrSnapshot>,
    show_pane: bool,
}

#[cfg(all(feature = "gtk", feature = "ghostty"))]
impl FocusResolution {
    fn failed(message: String) -> Self {
        Self {
            follow: StreamFollow::Stay,
            status: Some(SessionStatus::FocusError(message)),
            generation: 0,
            chrome: None,
            show_pane: false,
        }
    }

    fn stale() -> Self {
        Self {
            follow: StreamFollow::Stay,
            status: Some(SessionStatus::ReadOnly),
            generation: 0,
            chrome: None,
            show_pane: false,
        }
    }
}

#[cfg(all(feature = "gtk", feature = "ghostty"))]
struct TimerPull {
    change: AttachmentChange,
    generation: u64,
    snap: Option<crate::herdr::HerdrSnapshot>,
}

#[cfg(all(feature = "gtk", feature = "ghostty"))]
fn issue_snapshot(gen: &std::sync::atomic::AtomicU64) -> u64 {
    use std::sync::atomic::Ordering;
    gen.fetch_add(1, Ordering::AcqRel).saturating_add(1)
}

#[cfg(all(feature = "gtk", feature = "ghostty"))]
fn herdr_error_text(err: &crate::herdr::HerdrError) -> String {
    use crate::herdr::HerdrError;
    match err {
        HerdrError::SnapshotParse(_) | HerdrError::SnapshotMissingFocus => {
            "Could not read the workspace snapshot".to_string()
        }
        other => other.to_string(),
    }
}

#[cfg(all(feature = "gtk", feature = "ghostty"))]
fn pull_workspace_focus(
    requested: &str,
    pane_id: &mut String,
    focused_workspace_id: &mut String,
    snapshot_generation: &std::sync::atomic::AtomicU64,
) -> FocusResolution {
    use crate::herdr::{fetch_snapshot, focus_workspace};
    use std::sync::atomic::Ordering;

    tracing::info!(workspace_id = %requested, "focusing workspace");
    if let Err(err) = focus_workspace(requested) {
        tracing::warn!(workspace_id = %requested, %err, "workspace focus command failed");
        return FocusResolution::failed(err.to_string());
    }
    let generation = issue_snapshot(snapshot_generation);
    let snap = match fetch_snapshot() {
        Ok(snap) => snap,
        Err(err) => {
            tracing::warn!(workspace_id = %requested, "workspace snapshot after focus failed");
            return FocusResolution::failed(herdr_error_text(&err));
        }
    };
    if snapshot_generation.load(Ordering::Acquire) != generation {
        return FocusResolution::stale();
    }
    let focused_pane_id = snap.focused.as_ref().map(|pane| pane.pane_id.clone());
    match resolve_attachment(
        generation,
        pane_id,
        SnapshotResult::FocusSnapshot {
            generation,
            requested_workspace_id: requested.to_string(),
            focused_workspace_id: snap.focused_workspace_id.clone(),
            focused_pane_id,
        },
    ) {
        AttachmentChange::Unchanged => {
            tracing::warn!(
                requested = %requested,
                actual = snap.focused_workspace_id.as_deref().unwrap_or(""),
                "workspace focus raced"
            );
            FocusResolution::failed(focus_race_message(
                requested,
                snap.focused_workspace_id.as_deref(),
            ))
        }
        AttachmentChange::Reconnect(id) => {
            *pane_id = id;
            if let Some(pane) = &snap.focused {
                *focused_workspace_id = pane.workspace_id.clone();
            }
            FocusResolution {
                follow: StreamFollow::Restart,
                status: Some(SessionStatus::ReadOnly),
                generation,
                chrome: Some(snap),
                show_pane: true,
            }
        }
        AttachmentChange::Disconnect => {
            if let Some(id) = snap.focused_workspace_id.clone() {
                *focused_workspace_id = id;
            }
            FocusResolution {
                follow: StreamFollow::Stop,
                status: Some(SessionStatus::Disconnected(
                    "Workspace has no focused pane".to_string(),
                )),
                generation,
                chrome: Some(snap),
                show_pane: false,
            }
        }
    }
}

#[cfg(all(feature = "gtk", feature = "ghostty"))]
fn pull_agent_focus(
    requested: &str,
    pane_id: &mut String,
    focused_workspace_id: &mut String,
    snapshot_generation: &std::sync::atomic::AtomicU64,
) -> FocusResolution {
    use crate::herdr::{fetch_snapshot, focus_agent};
    use std::sync::atomic::Ordering;

    tracing::info!(pane_id = %requested, "focusing agent");
    if let Err(err) = focus_agent(requested) {
        tracing::warn!(pane_id = %requested, %err, "agent focus command failed");
        return FocusResolution::failed(err.to_string());
    }
    let generation = issue_snapshot(snapshot_generation);
    let snap = match fetch_snapshot() {
        Ok(snap) => snap,
        Err(err) => {
            tracing::warn!(pane_id = %requested, "agent snapshot after focus failed");
            return FocusResolution::failed(herdr_error_text(&err));
        }
    };
    if snapshot_generation.load(Ordering::Acquire) != generation {
        return FocusResolution::stale();
    }
    let focused_pane_id = snap.focused.as_ref().map(|pane| pane.pane_id.clone());
    match resolve_attachment(
        generation,
        pane_id,
        SnapshotResult::AgentFocusSnapshot {
            generation,
            requested_pane_id: requested.to_string(),
            focused_pane_id,
        },
    ) {
        AttachmentChange::Unchanged => {
            let actual = snap.focused.as_ref().map(|pane| pane.pane_id.as_str());
            tracing::warn!(
                requested = %requested,
                actual = actual.unwrap_or(""),
                "agent focus raced"
            );
            FocusResolution::failed(agent_focus_race_message(requested, actual))
        }
        AttachmentChange::Reconnect(id) => {
            *pane_id = id;
            if let Some(pane) = &snap.focused {
                *focused_workspace_id = pane.workspace_id.clone();
            }
            FocusResolution {
                follow: StreamFollow::Restart,
                status: Some(SessionStatus::ReadOnly),
                generation,
                chrome: Some(snap),
                show_pane: true,
            }
        }
        AttachmentChange::Disconnect => {
            if let Some(id) = snap.focused_workspace_id.clone() {
                *focused_workspace_id = id;
            }
            FocusResolution {
                follow: StreamFollow::Stop,
                status: Some(SessionStatus::Disconnected(
                    "Workspace has no focused pane".to_string(),
                )),
                generation,
                chrome: Some(snap),
                show_pane: false,
            }
        }
    }
}

#[cfg(all(feature = "gtk", feature = "ghostty"))]
enum PendingFocus {
    Workspace(String),
    Agent(String),
}

#[cfg(all(feature = "gtk", feature = "ghostty"))]
fn focus_targets_other(pending: &PendingFocus, pane_id: &str, workspace_id: &str) -> bool {
    match pending {
        PendingFocus::Workspace(id) => id != workspace_id,
        PendingFocus::Agent(id) => id != pane_id,
    }
}

#[cfg(all(feature = "gtk", feature = "ghostty"))]
fn pull_pending_focus(
    pending: PendingFocus,
    pane_id: &mut String,
    focused_workspace_id: &mut String,
    snapshot_generation: &std::sync::atomic::AtomicU64,
) -> FocusResolution {
    match pending {
        PendingFocus::Workspace(id) => {
            pull_workspace_focus(&id, pane_id, focused_workspace_id, snapshot_generation)
        }
        PendingFocus::Agent(id) => {
            pull_agent_focus(&id, pane_id, focused_workspace_id, snapshot_generation)
        }
    }
}

#[cfg(all(feature = "gtk", feature = "ghostty"))]
fn show_snapshot_lists(chrome: &crate::ui::ShellChrome, snap: &crate::herdr::HerdrSnapshot) {
    chrome.set_workspaces(
        &snap.workspaces,
        snap.focused_workspace_id.as_deref(),
        &snap.agents,
        snap.focused.as_ref().map(|pane| pane.pane_id.as_str()),
    );
}

#[cfg(all(feature = "gtk", feature = "ghostty"))]
fn pull_timer_snapshot(
    pane_id: &mut String,
    focused_workspace_id: &mut String,
    snapshot_generation: &std::sync::atomic::AtomicU64,
) -> TimerPull {
    use std::sync::atomic::Ordering;

    let generation = issue_snapshot(snapshot_generation);
    let snap = match crate::herdr::fetch_snapshot() {
        Ok(snap) => snap,
        Err(_) => {
            tracing::warn!("workspace snapshot request failed");
            return TimerPull {
                change: AttachmentChange::Unchanged,
                generation,
                snap: None,
            };
        }
    };
    if snapshot_generation.load(Ordering::Acquire) != generation {
        return TimerPull {
            change: AttachmentChange::Unchanged,
            generation,
            snap: None,
        };
    }
    let change = resolve_attachment(
        generation,
        pane_id,
        SnapshotResult::Refresh {
            generation,
            focused_pane_id: snap.focused.as_ref().map(|pane| pane.pane_id.clone()),
        },
    );
    if let Some(id) = snap.focused_workspace_id.clone() {
        *focused_workspace_id = id;
    }
    if let AttachmentChange::Reconnect(ref id) = change {
        *pane_id = id.clone();
    }
    TimerPull {
        change,
        generation,
        snap: Some(snap),
    }
}

#[cfg(all(feature = "gtk", feature = "ghostty"))]
fn emit_focus_resolution(
    resolution: &FocusResolution,
    publish_chrome: &impl Fn(u64, &crate::herdr::HerdrSnapshot, bool),
    publish_status: &impl Fn(SessionStatus),
) -> StreamFollow {
    if let Some(snap) = resolution.chrome.as_ref() {
        publish_chrome(resolution.generation, snap, resolution.show_pane);
    }
    if let Some(status) = resolution.status.clone() {
        publish_status(status);
    }
    resolution.follow
}

#[cfg(all(feature = "gtk", feature = "ghostty"))]
fn start_live_observe(
    state: std::sync::Arc<Mutex<ShellState>>,
    chrome: std::sync::Arc<crate::ui::ShellChrome>,
    header_label: gtk::Label,
    banner: adw::Banner,
    window: adw::ApplicationWindow,
    takeover: gtk::Button,
) {
    use crate::herdr::{discover_snapshot, HerdrError};
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
    use std::sync::Arc;

    let unavailable = |err: String| {
        {
            let mut borrowed = state.lock().unwrap_or_else(|e| e.into_inner());
            set_status(&mut borrowed.status, SessionStatus::Unavailable(err));
        }
        apply_banner(&banner, &state.lock().unwrap_or_else(|e| e.into_inner()));
        chrome.refresh_debug(&state.lock().unwrap_or_else(|e| e.into_inner()));
    };

    let first = match discover_snapshot() {
        Ok(snap) => snap,
        Err(err) => {
            unavailable(err.to_string());
            return;
        }
    };
    let Some(first_pane) = first.focused.clone() else {
        show_snapshot_lists(&chrome, &first);
        unavailable(HerdrError::SnapshotMissingFocus.to_string());
        return;
    };

    {
        let mut borrowed = state.lock().unwrap_or_else(|e| e.into_inner());
        apply_focused_pane(&mut borrowed, &first_pane);
        borrowed.frame_count = 0;
        borrowed.detection_count = 0;
    }
    show_snapshot_lists(&chrome, &first);
    apply_header(
        &header_label,
        &window,
        &chrome,
        &state.lock().unwrap_or_else(|e| e.into_inner()),
    );
    chrome.refresh_debug(&state.lock().unwrap_or_else(|e| e.into_inner()));

    let second = match discover_snapshot() {
        Ok(snap) => snap,
        Err(err) => {
            unavailable(err.to_string());
            return;
        }
    };
    let Some(second_pane) = second.focused.clone() else {
        show_snapshot_lists(&chrome, &second);
        unavailable(HerdrError::SnapshotMissingFocus.to_string());
        return;
    };

    if second_pane.pane_id != first_pane.pane_id {
        show_snapshot_lists(&chrome, &second);
        {
            let mut borrowed = state.lock().unwrap_or_else(|e| e.into_inner());
            set_status(&mut borrowed.status, SessionStatus::ChangedPane);
        }
        apply_banner(&banner, &state.lock().unwrap_or_else(|e| e.into_inner()));
        chrome.refresh_debug(&state.lock().unwrap_or_else(|e| e.into_inner()));
        return;
    }

    {
        let mut borrowed = state.lock().unwrap_or_else(|e| e.into_inner());
        apply_focused_pane(&mut borrowed, &second_pane);
    }
    show_snapshot_lists(&chrome, &second);
    apply_header(
        &header_label,
        &window,
        &chrome,
        &state.lock().unwrap_or_else(|e| e.into_inner()),
    );

    {
        let mut borrowed = state.lock().unwrap_or_else(|e| e.into_inner());
        set_status(&mut borrowed.status, SessionStatus::ReadOnly);
    }
    apply_banner(&banner, &state.lock().unwrap_or_else(|e| e.into_inner()));
    apply_takeover_button(&takeover, &SessionStatus::ReadOnly);

    let Some(layout) = second.layout.clone() else {
        unavailable(
            second
                .layout_error
                .clone()
                .unwrap_or_else(|| "missing tab layout".into()),
        );
        return;
    };
    chrome.apply_tab_layout(&layout);

    let generation = Arc::new(AtomicU64::new(1));
    let snapshot_generation = Arc::new(AtomicU64::new(0));
    let frame_slot: Arc<Mutex<HashMap<String, LiveFrame>>> = Arc::new(Mutex::new(HashMap::new()));
    let status_slot: Arc<Mutex<Option<SessionStatus>>> = Arc::new(Mutex::new(None));
    let chrome_slot: Arc<Mutex<Option<ChromeUpdate>>> = Arc::new(Mutex::new(None));
    let queued = Arc::new(AtomicBool::new(false));
    let input_enabled = Arc::new(AtomicBool::new(false));
    let (cmd_tx, cmd_rx) = std::sync::mpsc::channel::<SessionCommand>();

    {
        use gtk::prelude::*;
        let tx = cmd_tx.clone();
        let state_for_click = Arc::clone(&state);
        let button_for_click = takeover.clone();
        takeover.connect_clicked(move |_| {
            let pane_id = state_for_click
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .pane_id
                .clone();
            tracing::info!(%pane_id, "takeover requested");
            button_for_click.set_sensitive(false);
            if tx.send(SessionCommand::Takeover).is_err() {
                button_for_click.set_sensitive(true);
            }
        });

        wire_pane_grids(&chrome, &cmd_tx);
        wire_pane_pointers(&chrome, &cmd_tx);
        bind_focused_keys(&chrome, &cmd_tx, &input_enabled);
        chrome.terminal().grab_focus();

        let tx = cmd_tx.clone();
        chrome.connect_workspace_activated(move |workspace_id| {
            tracing::info!(%workspace_id, "workspace focus requested");
            let _ = tx.send(SessionCommand::Focus(workspace_id));
        });

        let tx = cmd_tx.clone();
        chrome.connect_agent_activated(move |pane_id| {
            tracing::info!(%pane_id, "agent focus requested");
            let _ = tx.send(SessionCommand::FocusAgent(pane_id));
        });

        let tx = cmd_tx.clone();
        glib::timeout_add_seconds_local(WORKSPACE_POLL_SECS, move || {
            let _ = tx.send(SessionCommand::PollSnapshot);
            glib::ControlFlow::Continue
        });

        let tx = cmd_tx.clone();
        let close_once = Arc::new(AtomicBool::new(false));
        window.connect_close_request(move |window| {
            if close_once.swap(true, Ordering::AcqRel) {
                return gtk::glib::Propagation::Proceed;
            }
            let _ = tx.send(SessionCommand::Shutdown);
            let window = window.clone();
            glib::timeout_add_local_once(std::time::Duration::from_millis(500), move || {
                window.destroy();
            });
            gtk::glib::Propagation::Stop
        });
    }

    // Widgets are not Send. The guard is created on the GTK thread and only
    // dereferenced from idle callbacks that run there.
    let surface = ReleaseOnMain::new(Arc::new(glib::thread_guard::ThreadGuard::new(UiSurface {
        chrome,
        header_label,
        banner,
        window,
        takeover,
    })));

    let wake_ui = {
        let frame_slot = Arc::clone(&frame_slot);
        let status_slot = Arc::clone(&status_slot);
        let chrome_slot = Arc::clone(&chrome_slot);
        let queued = Arc::clone(&queued);
        let generation = Arc::clone(&generation);
        let snapshot_generation = Arc::clone(&snapshot_generation);
        let state = Arc::clone(&state);
        let input_enabled = Arc::clone(&input_enabled);
        let cmd_tx = cmd_tx.clone();
        move || {
            if queued
                .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
                .is_err()
            {
                return;
            }
            let frame_slot = Arc::clone(&frame_slot);
            let status_slot = Arc::clone(&status_slot);
            let chrome_slot = Arc::clone(&chrome_slot);
            let queued = Arc::clone(&queued);
            let generation = Arc::clone(&generation);
            let snapshot_generation = Arc::clone(&snapshot_generation);
            let state = Arc::clone(&state);
            let input_enabled = Arc::clone(&input_enabled);
            let cmd_tx = cmd_tx.clone();
            let surface = Arc::clone(surface.get());
            glib::idle_add_once(move || {
                queued.store(false, Ordering::Release);
                let ui = surface.get_ref();

                let pending_status = status_slot.lock().unwrap_or_else(|e| e.into_inner()).take();
                let pending_frames =
                    std::mem::take(&mut *frame_slot.lock().unwrap_or_else(|e| e.into_inner()));
                let pending_chrome = chrome_slot.lock().unwrap_or_else(|e| e.into_inner()).take();
                let clears_grid = pending_status.as_ref().is_some_and(status_clears_grid);

                let mut rebuilt = false;
                let mut grab_pane = false;
                if let Some(update) = pending_chrome {
                    if update.generation == snapshot_generation.load(Ordering::Acquire) {
                        ui.chrome.set_workspaces(
                            &update.workspaces,
                            update.focused_workspace_id.as_deref(),
                            &update.agents,
                            update.focused_pane_id.as_deref(),
                        );
                        if let Some(pane) = update.pane {
                            let mut borrowed = state.lock().unwrap_or_else(|e| e.into_inner());
                            let previous_pane = borrowed.pane_id.clone();
                            apply_focused_pane(&mut borrowed, &pane);
                            grab_pane = borrowed.pane_id != previous_pane;
                        }
                        if let Some(layout) = update.layout.as_ref() {
                            rebuilt = ui.chrome.apply_tab_layout(layout);
                            if rebuilt {
                                wire_pane_grids(&ui.chrome, &cmd_tx);
                            }
                            wire_pane_pointers(&ui.chrome, &cmd_tx);
                            bind_focused_keys(&ui.chrome, &cmd_tx, &input_enabled);
                        }
                    }
                }

                let focus_terminal;
                {
                    let mut borrowed = state.lock().unwrap_or_else(|e| e.into_inner());
                    let previous = borrowed.status.clone();
                    let focused_pane = borrowed.pane_id.clone();
                    let current = generation.load(Ordering::Acquire);
                    let mut drew = false;
                    if clears_grid {
                        for (_, view) in ui.chrome.panes() {
                            let Some(model) = view.snapshot_model() else {
                                continue;
                            };
                            if let Ok(blank) = TerminalSnapshot::blank(model.cols(), model.rows()) {
                                view.set_snapshot(blank);
                            }
                        }
                    } else {
                        for frame in pending_frames.into_values() {
                            if frame.generation != current {
                                continue;
                            }
                            if let Some(view) = pane_view(&ui.chrome, &frame.pane_id) {
                                view.set_snapshot(frame.snapshot);
                            }
                            if frame.pane_id == focused_pane {
                                borrowed.frame_count = frame.frame_count;
                                borrowed.detection_count = frame.detection_count;
                                drew = true;
                            }
                        }
                        if drew
                            && pending_status.is_none()
                            && matches!(borrowed.status, SessionStatus::Resyncing)
                        {
                            set_status(&mut borrowed.status, SessionStatus::ReadOnly);
                        }
                    }
                    if let Some(next) = pending_status {
                        set_status(&mut borrowed.status, next);
                    }
                    focus_terminal = (previous != borrowed.status
                        && matches!(borrowed.status, SessionStatus::Controlling))
                        || (rebuilt && matches!(borrowed.status, SessionStatus::Controlling));
                    input_enabled.store(
                        matches!(borrowed.status, SessionStatus::Controlling),
                        Ordering::Release,
                    );
                }

                let status = state
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .status
                    .clone();
                if focus_terminal || grab_pane {
                    use gtk::prelude::WidgetExt;
                    ui.chrome.terminal().grab_focus();
                }
                apply_takeover_button(&ui.takeover, &status);
                if matches!(status, SessionStatus::FocusError(_)) {
                    ui.chrome.restore_workspace_mark();
                    ui.chrome.restore_agent_mark();
                }
                apply_header(
                    &ui.header_label,
                    &ui.window,
                    &ui.chrome,
                    &state.lock().unwrap_or_else(|e| e.into_inner()),
                );
                apply_banner(&ui.banner, &state.lock().unwrap_or_else(|e| e.into_inner()));
                ui.chrome
                    .refresh_debug(&state.lock().unwrap_or_else(|e| e.into_inner()));
            });
        }
    };

    let pane_id = layout.focused_pane_id.clone();
    let visible = layout.visible_panes();
    let workspace_id = second
        .focused_workspace_id
        .clone()
        .unwrap_or(second_pane.workspace_id);
    std::thread::Builder::new()
        .name("herdr-session".into())
        .spawn(move || {
            session_worker(
                pane_id,
                visible,
                layout,
                workspace_id,
                generation,
                snapshot_generation,
                frame_slot,
                status_slot,
                chrome_slot,
                cmd_rx,
                wake_ui,
            );
        })
        .expect("spawn herdr session worker");
}

#[cfg(all(feature = "gtk", feature = "ghostty"))]
enum SessionCommand {
    PaneGrid {
        pane_id: String,
        grid: GridSize,
    },
    Takeover,
    Input(crate::herdr::KeyInput),
    /// Refresh the workspace and agent lists from `herdr api snapshot`.
    PollSnapshot,
    /// `herdr workspace focus <workspace_id>`.
    Focus(String),
    /// `herdr agent focus <pane_id>`.
    FocusAgent(String),
    /// Click on `pane_id`: one `herdr pane focus --direction` step toward it.
    FocusPane(String),
    /// Alt+arrow: `herdr pane focus --direction` from the focused pane.
    FocusDirection(crate::herdr::PaneDirection),
    Scroll {
        pane_id: String,
        direction: crate::herdr::ScrollDirection,
        lines: u16,
        source: crate::herdr::ScrollSource,
        column: Option<u16>,
        row: Option<u16>,
    },
    Shutdown,
}

#[cfg(all(feature = "gtk", feature = "ghostty"))]
fn map_terminal_key(
    keyval: gtk::gdk::Key,
    state: gtk::gdk::ModifierType,
) -> Option<crate::herdr::KeyInput> {
    use crate::herdr::{encode_key, Key as TermKey, KeyEvent};
    use gtk::gdk::{Key, ModifierType};

    if state.contains(ModifierType::SUPER_MASK)
        || state.contains(ModifierType::META_MASK)
        || state.contains(ModifierType::HYPER_MASK)
    {
        return None;
    }
    let key = match keyval {
        Key::Return | Key::KP_Enter => TermKey::Enter,
        Key::BackSpace => TermKey::Backspace,
        Key::Tab => TermKey::Tab,
        Key::Escape => TermKey::Escape,
        Key::Up | Key::KP_Up => TermKey::Up,
        Key::Down | Key::KP_Down => TermKey::Down,
        Key::Left | Key::KP_Left => TermKey::Left,
        Key::Right | Key::KP_Right => TermKey::Right,
        _ => match keyval.to_unicode() {
            Some(ch) if !ch.is_control() => TermKey::Char(ch),
            _ => TermKey::Other,
        },
    };
    encode_key(KeyEvent {
        key,
        ctrl: state.contains(ModifierType::CONTROL_MASK),
        alt: state.contains(ModifierType::ALT_MASK),
        shift: state.contains(ModifierType::SHIFT_MASK),
    })
}

#[cfg(all(feature = "gtk", feature = "ghostty"))]
fn push_terminal_frame(
    engine: &mut Option<crate::terminal::TerminalEngine>,
    engine_dims: &mut Option<(u16, u16)>,
    frame: &crate::terminal::FrameBytes,
) -> Result<crate::terminal::TerminalSnapshot, String> {
    use crate::terminal::TerminalEngine;
    if frame.full || engine.is_none() {
        let mut fresh =
            TerminalEngine::new(frame.cols, frame.rows).map_err(|err| err.to_string())?;
        fresh.feed(&frame.bytes);
        let snapshot = fresh.snapshot();
        *engine = Some(fresh);
        *engine_dims = Some((frame.cols, frame.rows));
        return Ok(snapshot);
    }
    let eng = engine.as_mut().expect("terminal engine");
    if *engine_dims != Some((frame.cols, frame.rows)) {
        eng.resize(frame.cols, frame.rows);
        *engine_dims = Some((frame.cols, frame.rows));
    }
    eng.feed(&frame.bytes);
    Ok(eng.snapshot())
}

#[cfg(all(feature = "gtk", feature = "ghostty"))]
fn write_resize(
    session: &mut crate::herdr::ControlSession,
    grid: GridSize,
) -> Result<(), crate::herdr::HerdrError> {
    tracing::info!(cols = grid.cols, rows = grid.rows, "resize sent");
    session.resize(grid.cols, grid.rows, grid.cell_w, grid.cell_h)
}

#[cfg(all(feature = "gtk", feature = "ghostty"))]
fn release_control(session: &mut crate::herdr::ControlSession, pane_id: &str, released: &mut bool) {
    if *released {
        return;
    }
    *released = true;
    tracing::info!(%pane_id, "releasing pane control");
    match session.release_with_timeout(std::time::Duration::from_millis(300)) {
        Ok(true) => tracing::info!(%pane_id, "control released"),
        Ok(false) => tracing::warn!(
            %pane_id,
            "control release flushed; process still running. Killing this app before the flush cannot guarantee detach; Herdr detaches on stdin EOF"
        ),
        Err(err) => tracing::error!(%err, %pane_id, "control release failed"),
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(
    not(any(test, all(feature = "gtk", feature = "ghostty"))),
    allow(dead_code)
)]
struct ObserverDiff {
    reap: Vec<String>,
    spawn: Vec<String>,
}

#[cfg_attr(
    not(any(test, all(feature = "gtk", feature = "ghostty"))),
    allow(dead_code)
)]
fn diff_observers(running: &[String], visible: &[String]) -> ObserverDiff {
    let running_set: std::collections::BTreeSet<&str> =
        running.iter().map(String::as_str).collect();
    let visible_set: std::collections::BTreeSet<&str> =
        visible.iter().map(String::as_str).collect();
    let mut reap: Vec<String> = running
        .iter()
        .filter(|id| !visible_set.contains(id.as_str()))
        .cloned()
        .collect();
    let mut spawn: Vec<String> = visible
        .iter()
        .filter(|id| !running_set.contains(id.as_str()))
        .cloned()
        .collect();
    reap.sort();
    spawn.sort();
    ObserverDiff { reap, spawn }
}

/// Observers cover every visible pane, except the focused pane while this
/// process holds the controller.
#[cfg_attr(
    not(any(test, all(feature = "gtk", feature = "ghostty"))),
    allow(dead_code)
)]
fn observe_targets(visible: &[String], controlling: bool, focused: &str) -> Vec<String> {
    if !controlling {
        return visible.to_vec();
    }
    visible
        .iter()
        .filter(|id| id.as_str() != focused)
        .cloned()
        .collect()
}

#[cfg_attr(
    not(any(test, all(feature = "gtk", feature = "ghostty"))),
    allow(dead_code)
)]
fn plan_observers(
    running: &[String],
    visible: &[String],
    controlling: bool,
    focused: &str,
) -> ObserverDiff {
    diff_observers(running, &observe_targets(visible, controlling, focused))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(
    not(any(test, all(feature = "gtk", feature = "ghostty"))),
    allow(dead_code)
)]
enum FocusControl {
    Keep,
    Release,
}

/// A focused-pane change while controlling drops the controller. Takeover
/// does not follow the new pane.
#[cfg_attr(
    not(any(test, all(feature = "gtk", feature = "ghostty"))),
    allow(dead_code)
)]
fn focus_control(controlling: bool, previous: &str, next: &str) -> FocusControl {
    if controlling && previous != next {
        FocusControl::Release
    } else {
        FocusControl::Keep
    }
}

#[cfg_attr(not(all(feature = "gtk", feature = "ghostty")), allow(dead_code))]
fn adopt_visible(
    pane_id: &mut String,
    visible: &mut Vec<String>,
    layout: &mut Option<crate::herdr::TabLayout>,
    snap: &crate::herdr::HerdrSnapshot,
) -> Result<(), String> {
    if let Some(err) = snap.layout_error.clone() {
        return Err(err);
    }
    if let Some(next) = snap.layout.as_ref() {
        *visible = next.visible_panes();
        *pane_id = next.focused_pane_id.clone();
        *layout = Some(next.clone());
    }
    Ok(())
}

/// Fold a fractional line delta into whole TUI scroll steps.
///
/// Touchpads emit many tiny `dy` values. Quantizing each one to at least one
/// line makes a light two-finger move jump. Carry the remainder until a full
/// line is due. Positive `delta_lines` scrolls down.
fn push_scroll_delta(
    accum: &mut f64,
    delta_lines: f64,
) -> Option<(crate::herdr::ScrollDirection, u16)> {
    use crate::herdr::ScrollDirection;
    if !delta_lines.is_finite() || delta_lines == 0.0 {
        return None;
    }
    *accum += delta_lines;
    let lines = accum.trunc() as i32;
    if lines == 0 {
        return None;
    }
    *accum -= f64::from(lines);
    let direction = if lines > 0 {
        ScrollDirection::Down
    } else {
        ScrollDirection::Up
    };
    let n = u16::try_from(lines.unsigned_abs())
        .unwrap_or(u16::MAX)
        .clamp(1, 64);
    Some((direction, n))
}

/// Wheel tick (`dy` ≈ ±1) to a Herdr viewport scroll. Positive `dy` scrolls down.
#[cfg_attr(not(test), allow(dead_code))]
fn scroll_lines_from_delta(dy: f64) -> Option<(crate::herdr::ScrollDirection, u16)> {
    let mut accum = 0.0;
    push_scroll_delta(&mut accum, dy * 3.0)
}

#[cfg(all(feature = "gtk", feature = "ghostty"))]
fn pane_view(
    chrome: &crate::ui::ShellChrome,
    pane_id: &str,
) -> Option<crate::render::TerminalView> {
    chrome
        .panes()
        .into_iter()
        .find(|(id, _)| id == pane_id)
        .map(|(_, view)| view)
}

#[cfg(all(feature = "gtk", feature = "ghostty"))]
fn wire_pane_grids(chrome: &crate::ui::ShellChrome, tx: &std::sync::mpsc::Sender<SessionCommand>) {
    for (pane_id, view) in chrome.panes() {
        let tx = tx.clone();
        let pane_id = pane_id.clone();
        view.set_on_grid_capacity(move |cols, rows, cell_w, cell_h| {
            let _ = tx.send(SessionCommand::PaneGrid {
                pane_id: pane_id.clone(),
                grid: GridSize {
                    cols,
                    rows,
                    cell_w,
                    cell_h,
                },
            });
        });
    }
}

#[cfg(all(feature = "gtk", feature = "ghostty"))]
fn bind_focused_keys(
    chrome: &crate::ui::ShellChrome,
    tx: &std::sync::mpsc::Sender<SessionCommand>,
    input_enabled: &std::sync::Arc<std::sync::atomic::AtomicBool>,
) {
    use gtk::prelude::*;
    use std::sync::atomic::Ordering;

    let view = chrome.terminal();
    if chrome.key_target_is(&view) {
        return;
    }
    let controller = gtk::EventControllerKey::new();
    let input_flag = std::sync::Arc::clone(input_enabled);
    let tx = tx.clone();
    let terminal_for_key = view.clone();
    let focused_pane = chrome.focused_pane_id().unwrap_or_default();
    controller.connect_key_pressed(move |_, keyval, _keycode, state| {
        use crate::herdr::{PaneDirection, ScrollDirection, ScrollSource};
        use gtk::gdk::{Key, ModifierType};

        if state.contains(ModifierType::ALT_MASK) && !state.contains(ModifierType::CONTROL_MASK) {
            let direction = match keyval {
                Key::Left | Key::KP_Left => Some(PaneDirection::Left),
                Key::Right | Key::KP_Right => Some(PaneDirection::Right),
                Key::Up | Key::KP_Up => Some(PaneDirection::Up),
                Key::Down | Key::KP_Down => Some(PaneDirection::Down),
                _ => None,
            };
            if let Some(direction) = direction {
                let _ = tx.send(SessionCommand::FocusDirection(direction));
                return gtk::glib::Propagation::Stop;
            }
        }
        if !state.contains(ModifierType::CONTROL_MASK)
            && !state.contains(ModifierType::ALT_MASK)
            && matches!(
                keyval,
                Key::Page_Up | Key::Page_Down | Key::KP_Page_Up | Key::KP_Page_Down
            )
        {
            let direction = if matches!(keyval, Key::Page_Up | Key::KP_Page_Up) {
                ScrollDirection::Up
            } else {
                ScrollDirection::Down
            };
            let (_, rows) = terminal_for_key.cols_rows(
                f64::from(terminal_for_key.width()).max(1.0),
                f64::from(terminal_for_key.height()).max(1.0),
            );
            let _ = tx.send(SessionCommand::Scroll {
                pane_id: focused_pane.clone(),
                direction,
                lines: rows.saturating_sub(1).max(1),
                source: ScrollSource::PageKey,
                column: None,
                row: None,
            });
            return gtk::glib::Propagation::Stop;
        }
        if !input_flag.load(Ordering::Acquire) || !terminal_for_key.has_focus() {
            return gtk::glib::Propagation::Proceed;
        }
        match map_terminal_key(keyval, state) {
            Some(input) => {
                let _ = tx.send(SessionCommand::Input(input));
                gtk::glib::Propagation::Stop
            }
            None => gtk::glib::Propagation::Proceed,
        }
    });
    view.add_controller(controller);
    chrome.remember_key_target(&view);
}

#[cfg(all(feature = "gtk", feature = "ghostty"))]
fn wire_pane_pointers(
    chrome: &crate::ui::ShellChrome,
    tx: &std::sync::mpsc::Sender<SessionCommand>,
) {
    use gtk::prelude::*;

    for (pane_id, view) in chrome.panes() {
        if chrome.pointer_wired(&view) {
            continue;
        }
        let click = gtk::GestureClick::new();
        click.set_button(1);
        let tx_click = tx.clone();
        let pane_for_click = pane_id.clone();
        let view_for_click = view.clone();
        click.connect_pressed(move |gesture, _, _, _| {
            view_for_click.grab_focus();
            let _ = tx_click.send(SessionCommand::FocusPane(pane_for_click.clone()));
            gesture.set_state(gtk::EventSequenceState::Claimed);
        });
        view.add_controller(click);

        let motion = gtk::EventControllerMotion::new();
        let view_for_motion = view.clone();
        motion.connect_enter(move |_, x, y| {
            view_for_motion.set_pointer(x, y);
        });
        let view_for_motion = view.clone();
        motion.connect_motion(move |_, x, y| {
            view_for_motion.set_pointer(x, y);
        });
        view.add_controller(motion);

        let scroll = gtk::EventControllerScroll::new(
            gtk::EventControllerScrollFlags::VERTICAL | gtk::EventControllerScrollFlags::KINETIC,
        );
        let tx_scroll = tx.clone();
        let pane_for_scroll = pane_id.clone();
        let view_for_scroll = view.clone();
        let accum = std::rc::Rc::new(std::cell::Cell::new(0.0));
        scroll.connect_scroll(move |controller, _dx, dy| {
            let delta_lines = if controller.unit() == gtk::gdk::ScrollUnit::Surface {
                dy / view_for_scroll.cell_height().max(1.0)
            } else {
                dy * 3.0
            };
            let mut remainder = accum.get();
            let stepped = push_scroll_delta(&mut remainder, delta_lines);
            accum.set(remainder);
            let Some((direction, lines)) = stepped else {
                return gtk::glib::Propagation::Stop;
            };
            let (column, row) = wheel_cell(controller, &view_for_scroll);
            let _ = tx_scroll.send(SessionCommand::Scroll {
                pane_id: pane_for_scroll.clone(),
                direction,
                lines,
                source: crate::herdr::ScrollSource::Wheel,
                column,
                row,
            });
            gtk::glib::Propagation::Stop
        });
        view.add_controller(scroll);
        chrome.remember_pointer(&view);
    }
}

#[cfg(all(feature = "gtk", feature = "ghostty"))]
fn wheel_cell(
    controller: &gtk::EventControllerScroll,
    view: &crate::render::TerminalView,
) -> (Option<u16>, Option<u16>) {
    use gtk::prelude::*;

    let Some((local_x, local_y)) = view
        .pointer()
        .or_else(|| scroll_event_widget_point(controller, view))
    else {
        return (None, None);
    };
    let width = f64::from(view.width());
    let height = f64::from(view.height());
    let (cap_cols, cap_rows) = view.cols_rows(width, height);
    let (cols, rows) = view
        .snapshot_model()
        .map(|snapshot| (snapshot.cols(), snapshot.rows()))
        .filter(|(cols, rows)| *cols > 0 && *rows > 0)
        .unwrap_or((cap_cols, cap_rows));
    crate::render::cell_under_pointer(
        local_x,
        local_y,
        width,
        height,
        view.cell_width(),
        view.cell_height(),
        cols,
        rows,
        cap_cols,
        cap_rows,
    )
    .map_or((None, None), |(column, row)| (Some(column), Some(row)))
}

/// Scroll-event position in `TerminalView` coordinates.
///
/// `GdkEvent` positions are surface-relative. Subtract the native surface
/// transform, then map from the native widget into the view. `None` when the
/// event has no position, so the wheel does not invent a cell.
#[cfg(all(feature = "gtk", feature = "ghostty"))]
fn scroll_event_widget_point(
    controller: &gtk::EventControllerScroll,
    view: &crate::render::TerminalView,
) -> Option<(f64, f64)> {
    use gtk::prelude::*;

    let event = controller.current_event()?;
    let (sx, sy) = event.position()?;
    let native = view.native()?;
    let (tx, ty) = native.surface_transform();
    let point = gtk::graphene::Point::new((sx - tx) as f32, (sy - ty) as f32);
    let local = native.compute_point(view, &point)?;
    Some((f64::from(local.x()), f64::from(local.y())))
}

#[cfg(all(feature = "gtk", feature = "ghostty"))]
struct PaneMirror {
    cols: u16,
    rows: u16,
    session: crate::herdr::ObserveSession,
    engine: Option<crate::terminal::TerminalEngine>,
    engine_dims: Option<(u16, u16)>,
    frame_count: u64,
    awaiting_full: bool,
    resync_attempted: bool,
}

#[cfg(all(feature = "gtk", feature = "ghostty"))]
struct ControlledPane {
    session: crate::herdr::ControlSession,
    sync: ResizeSync,
    engine: Option<crate::terminal::TerminalEngine>,
    engine_dims: Option<(u16, u16)>,
    frame_count: u64,
    released: bool,
}

#[cfg(all(feature = "gtk", feature = "ghostty"))]
enum ObservePoll {
    Idle,
    Respawn,
    Halt(SessionStatus),
}

#[cfg(all(feature = "gtk", feature = "ghostty"))]
fn poll_observe(
    mirror: &mut PaneMirror,
    pane_id: &str,
    focused_pane: &str,
    generation: &std::sync::atomic::AtomicU64,
    publish_status: &impl Fn(SessionStatus),
    publish_frame: &impl Fn(LiveFrame),
) -> ObservePoll {
    use crate::herdr::{discover_focused_pane, HerdrError};
    use crate::render::parse;
    use crate::terminal::TerminalEvent;
    use std::sync::atomic::Ordering;
    use std::time::Duration;

    loop {
        match mirror.session.recv_timeout(Duration::ZERO) {
            Err(HerdrError::Timeout) => return ObservePoll::Idle,
            Ok(TerminalEvent::Data(frame)) => {
                if mirror.awaiting_full && frame.full {
                    tracing::info!(
                        pane_id,
                        cols = mirror.cols,
                        rows = mirror.rows,
                        width = frame.cols,
                        height = frame.rows,
                        "observe full frame"
                    );
                    mirror.awaiting_full = false;
                }
                match classify_observe_frame(
                    frame.full,
                    frame.cols,
                    frame.rows,
                    mirror.cols,
                    mirror.rows,
                ) {
                    ObserveFit::Disconnect => {
                        tracing::warn!(
                            pane_id,
                            cols = mirror.cols,
                            rows = mirror.rows,
                            width = frame.cols,
                            height = frame.rows,
                            "observe frame does not match card"
                        );
                        return ObservePoll::Halt(SessionStatus::Disconnected(format!(
                            "observe frame {}x{} does not match {}x{}",
                            frame.cols, frame.rows, mirror.cols, mirror.rows
                        )));
                    }
                    ObserveFit::Skip => continue,
                    ObserveFit::Draw => {
                        mirror.frame_count = mirror.frame_count.saturating_add(1);
                        let snapshot = match push_terminal_frame(
                            &mut mirror.engine,
                            &mut mirror.engine_dims,
                            &frame,
                        ) {
                            Ok(snapshot) => snapshot,
                            Err(err) => return ObservePoll::Halt(SessionStatus::Controller(err)),
                        };
                        let detection_count = parse(&snapshot).node_count();
                        publish_frame(LiveFrame {
                            pane_id: pane_id.to_string(),
                            generation: generation.load(Ordering::Acquire),
                            snapshot,
                            frame_count: mirror.frame_count,
                            detection_count,
                        });
                    }
                }
            }
            Ok(TerminalEvent::Closed(closed)) => {
                return ObservePoll::Halt(SessionStatus::Disconnected(closed.reason));
            }
            Err(err)
                if matches!(
                    err,
                    HerdrError::SequenceResyncRequired
                        | HerdrError::StreamEof
                        | HerdrError::ProcessExit { .. }
                ) =>
            {
                if mirror.resync_attempted {
                    return ObservePoll::Halt(SessionStatus::Disconnected(err.to_string()));
                }
                mirror.resync_attempted = true;
                publish_status(SessionStatus::Resyncing);
                return match discover_focused_pane() {
                    Ok(pane) if pane.pane_id == focused_pane => ObservePoll::Respawn,
                    Ok(_) => ObservePoll::Halt(SessionStatus::ChangedPane),
                    Err(discover_err) => {
                        ObservePoll::Halt(SessionStatus::Disconnected(discover_err.to_string()))
                    }
                };
            }
            Err(err) => return ObservePoll::Halt(SessionStatus::Disconnected(err.to_string())),
        }
    }
}

#[cfg(all(feature = "gtk", feature = "ghostty"))]
struct ScrollHolds {
    panes: HashMap<String, ControlledPane>,
}

#[cfg(all(feature = "gtk", feature = "ghostty"))]
impl Drop for ScrollHolds {
    fn drop(&mut self) {
        let ids: Vec<String> = self.panes.keys().cloned().collect();
        for id in ids {
            if let Some(mut hold) = self.panes.remove(&id) {
                release_control(&mut hold.session, &id, &mut hold.released);
            }
        }
    }
}

#[cfg(all(feature = "gtk", feature = "ghostty"))]
fn drain_hold(
    hold: &mut ControlledPane,
    pane_id: &str,
    generation: &std::sync::atomic::AtomicU64,
    publish_frame: &impl Fn(LiveFrame),
) -> bool {
    use crate::herdr::HerdrError;
    use crate::render::parse;
    use crate::terminal::TerminalEvent;
    use std::sync::atomic::Ordering;
    use std::time::Duration;

    loop {
        match hold.session.recv_timeout(Duration::ZERO) {
            Err(HerdrError::Timeout) => return true,
            Ok(TerminalEvent::Data(frame)) => {
                let (cols, rows) = hold.sync.confirmed();
                match classify_observe_frame(frame.full, frame.cols, frame.rows, cols, rows) {
                    ObserveFit::Skip => continue,
                    ObserveFit::Disconnect => return false,
                    ObserveFit::Draw => {
                        hold.frame_count = hold.frame_count.saturating_add(1);
                        let snapshot = match push_terminal_frame(
                            &mut hold.engine,
                            &mut hold.engine_dims,
                            &frame,
                        ) {
                            Ok(snapshot) => snapshot,
                            Err(_) => return false,
                        };
                        let detection_count = parse(&snapshot).node_count();
                        publish_frame(LiveFrame {
                            pane_id: pane_id.to_string(),
                            generation: generation.load(Ordering::Acquire),
                            snapshot,
                            frame_count: hold.frame_count,
                            detection_count,
                        });
                    }
                }
            }
            Ok(TerminalEvent::Closed(_)) | Err(_) => return false,
        }
    }
}

#[cfg(all(feature = "gtk", feature = "ghostty"))]
fn session_worker(
    initial_pane_id: String,
    initial_visible: Vec<String>,
    initial_layout: crate::herdr::TabLayout,
    initial_workspace_id: String,
    generation: std::sync::Arc<std::sync::atomic::AtomicU64>,
    snapshot_generation: std::sync::Arc<std::sync::atomic::AtomicU64>,
    frame_slot: std::sync::Arc<Mutex<HashMap<String, LiveFrame>>>,
    status_slot: std::sync::Arc<Mutex<Option<SessionStatus>>>,
    chrome_slot: std::sync::Arc<Mutex<Option<ChromeUpdate>>>,
    cmd_rx: std::sync::mpsc::Receiver<SessionCommand>,
    wake_ui: impl Fn() + Send + 'static,
) {
    use crate::herdr::{
        discover_focused_pane, ControlSession, HerdrError, HerdrSnapshot, KeyInput, ObserveSession,
    };
    use crate::render::parse;
    use crate::terminal::TerminalEvent;
    use std::sync::atomic::Ordering;
    use std::sync::mpsc::RecvTimeoutError;
    use std::time::Duration;

    const POLL: Duration = Duration::from_millis(50);

    let publish_status = |status: SessionStatus| {
        *status_slot.lock().unwrap_or_else(|e| e.into_inner()) = Some(status);
        wake_ui();
    };

    let publish_frame = |frame: LiveFrame| {
        if store_live_frame(&frame_slot, frame) {
            wake_ui();
        }
    };

    let publish_chrome = |generation_now: u64, snap: &HerdrSnapshot, show_pane: bool| {
        let update = ChromeUpdate {
            generation: generation_now,
            workspaces: snap.workspaces.clone(),
            focused_workspace_id: snap.focused_workspace_id.clone(),
            agents: snap.agents.clone(),
            focused_pane_id: snap.focused.as_ref().map(|pane| pane.pane_id.clone()),
            pane: if show_pane {
                snap.focused.clone()
            } else {
                None
            },
            layout: snap.layout.clone(),
        };
        if store_chrome_update(&chrome_slot, update) {
            wake_ui();
        }
    };

    struct ScrollRequest {
        pane_id: String,
        direction: crate::herdr::ScrollDirection,
        lines: u16,
        source: crate::herdr::ScrollSource,
        column: Option<u16>,
        row: Option<u16>,
    }

    struct Batch {
        shutdown: bool,
        takeover: bool,
        poll: bool,
        focus: Option<PendingFocus>,
        focus_pane: Option<String>,
        nudge: Option<crate::herdr::PaneDirection>,
        scrolls: Vec<ScrollRequest>,
        input: Vec<KeyInput>,
    }

    let mut pane_id = initial_pane_id;
    let mut visible = initial_visible;
    let mut tab_layout = Some(initial_layout);
    let mut focused_workspace_id = initial_workspace_id;
    let mut sizes: HashMap<String, GridSize> = HashMap::new();
    let mut mirrors: HashMap<String, PaneMirror> = HashMap::new();
    let mut control: Option<ControlledPane> = None;
    let mut scrollers = ScrollHolds {
        panes: HashMap::new(),
    };
    let mut takeover_requested = false;

    tracing::info!("placeholder grid; waiting for card capacity");
    publish_status(SessionStatus::ReadOnly);

    let release_owned = |control: &mut Option<ControlledPane>, pane: &str| {
        if let Some(ctrl) = control.as_mut() {
            release_control(&mut ctrl.session, pane, &mut ctrl.released);
        }
        *control = None;
    };

    loop {
        let mut batch = Batch {
            shutdown: false,
            takeover: false,
            poll: false,
            focus: None,
            focus_pane: None,
            nudge: None,
            scrolls: Vec::new(),
            input: Vec::new(),
        };
        let mut fold = |cmd: SessionCommand, batch: &mut Batch| match cmd {
            SessionCommand::PaneGrid { pane_id, grid } => {
                if let Some(grid) = observe_spawn_size(Some(grid)) {
                    sizes.insert(pane_id, grid);
                }
            }
            SessionCommand::Takeover => batch.takeover = true,
            SessionCommand::Input(input) => batch.input.push(input),
            SessionCommand::Focus(id) => batch.focus = Some(PendingFocus::Workspace(id)),
            SessionCommand::FocusAgent(id) => batch.focus = Some(PendingFocus::Agent(id)),
            SessionCommand::FocusPane(id) => batch.focus_pane = Some(id),
            SessionCommand::FocusDirection(direction) => batch.nudge = Some(direction),
            SessionCommand::Scroll {
                pane_id,
                direction,
                lines,
                source,
                column,
                row,
            } => batch.scrolls.push(ScrollRequest {
                pane_id,
                direction,
                lines,
                source,
                column,
                row,
            }),
            SessionCommand::PollSnapshot => batch.poll = true,
            SessionCommand::Shutdown => batch.shutdown = true,
        };
        match cmd_rx.recv_timeout(POLL) {
            Ok(cmd) => fold(cmd, &mut batch),
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => batch.shutdown = true,
        }
        while let Ok(cmd) = cmd_rx.try_recv() {
            fold(cmd, &mut batch);
        }

        if batch.shutdown {
            let had_control = control.is_some();
            release_owned(&mut control, &pane_id);
            if !had_control {
                tracing::info!("shutdown while observing; no controller to release");
            }
            return;
        }

        let mut input_failed = None;
        if let Some(ctrl) = control.as_mut() {
            for input in batch.input.drain(..) {
                let result = match input {
                    KeyInput::Text(text) => ctrl.session.input_text(&text),
                    KeyInput::Bytes(bytes) => ctrl.session.input_bytes(&bytes),
                };
                if let Err(err) = result {
                    tracing::warn!(%err, "input write failed");
                    if matches!(err, HerdrError::Io(_)) {
                        let message = err.to_string();
                        release_control(&mut ctrl.session, &pane_id, &mut ctrl.released);
                        input_failed = Some(message);
                        break;
                    }
                }
            }
        }
        if let Some(message) = input_failed {
            publish_status(SessionStatus::Disconnected(message));
            return;
        }

        let mut suppress_takeover = false;
        if let Some(pending) = batch.focus.take() {
            if focus_targets_other(&pending, &pane_id, &focused_workspace_id) {
                let previous = pane_id.clone();
                let had_control = control.is_some();
                if had_control {
                    release_owned(&mut control, &previous);
                    publish_status(SessionStatus::ReadOnly);
                    suppress_takeover = true;
                    takeover_requested = false;
                }
                let resolution = pull_pending_focus(
                    pending,
                    &mut pane_id,
                    &mut focused_workspace_id,
                    &snapshot_generation,
                );
                let follow = emit_focus_resolution(&resolution, &publish_chrome, &publish_status);
                if let Some(snap) = resolution.chrome.as_ref() {
                    if let Err(err) =
                        adopt_visible(&mut pane_id, &mut visible, &mut tab_layout, snap)
                    {
                        publish_status(SessionStatus::Disconnected(err));
                        return;
                    }
                }
                if focus_control(had_control, &previous, &pane_id) == FocusControl::Release {
                    mirrors.clear();
                    takeover_requested = false;
                    suppress_takeover = true;
                    publish_status(SessionStatus::ReadOnly);
                }
                if matches!(follow, StreamFollow::Stop) {
                    return;
                }
            }
        } else {
            if let Some(target) = batch.focus_pane.take() {
                if let Some(direction) = tab_layout
                    .as_ref()
                    .and_then(|layout| layout.direction_toward(&pane_id, &target))
                {
                    if let Err(err) = crate::herdr::focus_pane_direction(direction, &pane_id) {
                        tracing::warn!(%err, %target, "pane focus failed");
                        publish_status(SessionStatus::FocusError(err.to_string()));
                    }
                    batch.poll = true;
                }
            }
            if let Some(direction) = batch.nudge.take() {
                if let Err(err) = crate::herdr::focus_pane_direction(direction, &pane_id) {
                    tracing::warn!(%err, ?direction, "pane focus failed");
                    publish_status(SessionStatus::FocusError(err.to_string()));
                }
                batch.poll = true;
            }
            if batch.poll {
                let previous = pane_id.clone();
                let had_control = control.is_some();
                let pull = pull_timer_snapshot(
                    &mut pane_id,
                    &mut focused_workspace_id,
                    &snapshot_generation,
                );
                if let Some(snap) = pull.snap.as_ref() {
                    publish_chrome(pull.generation, snap, snap.focused.is_some());
                    if let Err(err) =
                        adopt_visible(&mut pane_id, &mut visible, &mut tab_layout, snap)
                    {
                        release_owned(&mut control, &previous);
                        publish_status(SessionStatus::Disconnected(err));
                        return;
                    }
                }
                if focus_control(had_control, &previous, &pane_id) == FocusControl::Release {
                    release_owned(&mut control, &previous);
                    mirrors.clear();
                    takeover_requested = false;
                    suppress_takeover = true;
                    publish_status(SessionStatus::ReadOnly);
                }
                if matches!(pull.change, AttachmentChange::Disconnect) {
                    release_owned(&mut control, &pane_id);
                    publish_status(SessionStatus::Disconnected(
                        "Workspace has no focused pane".to_string(),
                    ));
                    return;
                }
            }
        }

        if batch.takeover && !suppress_takeover {
            takeover_requested = true;
        }

        if takeover_requested && control.is_none() && sizes.contains_key(&pane_id) {
            generation.fetch_add(1, Ordering::AcqRel);
            publish_status(SessionStatus::Resyncing);
            match discover_focused_pane() {
                Ok(pane) if pane.pane_id == pane_id => {}
                Ok(_) => {
                    publish_status(SessionStatus::ChangedPane);
                    return;
                }
                Err(err) => {
                    publish_status(SessionStatus::Disconnected(err.to_string()));
                    return;
                }
            }
            let size = sizes[&pane_id];
            if let Some(mut hold) = scrollers.panes.remove(&pane_id) {
                release_control(&mut hold.session, &pane_id, &mut hold.released);
            }
            mirrors.remove(&pane_id);
            tracing::info!(
                %pane_id,
                cols = size.cols,
                rows = size.rows,
                "takeover: this process now owns the pane"
            );
            match ControlSession::spawn(&pane_id, size.cols, size.rows, true) {
                Ok(session) => {
                    control = Some(ControlledPane {
                        session,
                        sync: ResizeSync::new(size),
                        engine: None,
                        engine_dims: None,
                        frame_count: 0,
                        released: false,
                    });
                    takeover_requested = false;
                    publish_status(SessionStatus::Controlling);
                }
                Err(err) => {
                    publish_status(SessionStatus::Controller(err.to_string()));
                    return;
                }
            }
        }

        let hidden: Vec<String> = scrollers
            .panes
            .keys()
            .filter(|id| !visible.iter().any(|pane| pane == *id))
            .cloned()
            .collect();
        for id in hidden {
            if let Some(mut hold) = scrollers.panes.remove(&id) {
                release_control(&mut hold.session, &id, &mut hold.released);
            }
        }
        let mut dead_scrollers = Vec::new();
        for request in batch.scrolls {
            if control.is_some() && request.pane_id == pane_id {
                if let Some(ctrl) = control.as_mut() {
                    if let Err(err) = ctrl.session.scroll(
                        request.direction,
                        request.lines,
                        request.source,
                        request.column,
                        request.row,
                    ) {
                        if matches!(err, HerdrError::Io(_)) {
                            let message = err.to_string();
                            release_control(&mut ctrl.session, &pane_id, &mut ctrl.released);
                            publish_status(SessionStatus::Disconnected(message));
                            return;
                        }
                    }
                }
                continue;
            }
            if !scrollers.panes.contains_key(&request.pane_id) {
                let Some(size) = sizes.get(&request.pane_id).copied() else {
                    continue;
                };
                match ControlSession::spawn(&request.pane_id, size.cols, size.rows, false) {
                    Ok(session) => {
                        generation.fetch_add(1, Ordering::AcqRel);
                        mirrors.remove(&request.pane_id);
                        scrollers.panes.insert(
                            request.pane_id.clone(),
                            ControlledPane {
                                session,
                                sync: ResizeSync::new(size),
                                engine: None,
                                engine_dims: None,
                                frame_count: 0,
                                released: false,
                            },
                        );
                    }
                    Err(err) => {
                        tracing::warn!(%err, pane_id = %request.pane_id, "scroll control failed");
                        continue;
                    }
                }
            }
            if let Some(hold) = scrollers.panes.get_mut(&request.pane_id) {
                if let Err(err) = hold.session.scroll(
                    request.direction,
                    request.lines,
                    request.source,
                    request.column,
                    request.row,
                ) {
                    tracing::warn!(%err, pane_id = %request.pane_id, "scroll write failed");
                    dead_scrollers.push(request.pane_id.clone());
                }
            }
        }
        for id in dead_scrollers {
            if let Some(mut hold) = scrollers.panes.remove(&id) {
                release_control(&mut hold.session, &id, &mut hold.released);
            }
        }
        for id in scrollers.panes.keys() {
            mirrors.remove(id);
        }

        let running: Vec<String> = mirrors.keys().cloned().collect();
        let diff = plan_observers(&running, &visible, control.is_some(), &pane_id);
        let mut spawn_now: Vec<String> = diff
            .spawn
            .iter()
            .filter(|id| sizes.contains_key(*id) && !scrollers.panes.contains_key(*id))
            .cloned()
            .collect();
        let mut resized = Vec::new();
        let mut resync_flags: HashMap<String, bool> = HashMap::new();
        for (id, mirror) in &mirrors {
            if diff.reap.iter().any(|reap| reap == id) {
                continue;
            }
            if let Some(size) = sizes.get(id) {
                if size.cols != mirror.cols || size.rows != mirror.rows {
                    resized.push(id.clone());
                    resync_flags.insert(id.clone(), mirror.resync_attempted);
                }
            }
        }
        for id in &resized {
            mirrors.remove(id);
            spawn_now.push(id.clone());
        }
        if !diff.reap.is_empty() || !spawn_now.is_empty() {
            generation.fetch_add(1, Ordering::AcqRel);
        }
        for id in &diff.reap {
            tracing::info!(pane_id = %id, "reaping observer");
            mirrors.remove(id);
        }
        for id in spawn_now {
            let attempted = resync_flags.get(&id).copied().unwrap_or(false);
            let size = sizes[&id];
            match ObserveSession::spawn(&id, size.cols, size.rows) {
                Ok(session) => {
                    tracing::info!(pane_id = %id, cols = size.cols, rows = size.rows, "observing pane");
                    mirrors.insert(
                        id,
                        PaneMirror {
                            cols: size.cols,
                            rows: size.rows,
                            session,
                            engine: None,
                            engine_dims: None,
                            frame_count: 0,
                            awaiting_full: true,
                            resync_attempted: attempted,
                        },
                    );
                }
                Err(err) => {
                    publish_status(SessionStatus::Disconnected(err.to_string()));
                    return;
                }
            }
        }

        let mut resize_failed = None;
        if let Some(ctrl) = control.as_mut() {
            if let Some(grid) = sizes.get(&pane_id).copied() {
                if let Some(next) = ctrl.sync.note_desired(grid) {
                    if let Err(err) = write_resize(&mut ctrl.session, next) {
                        let message = err.to_string();
                        release_control(&mut ctrl.session, &pane_id, &mut ctrl.released);
                        resize_failed = Some(message);
                    }
                }
            }
        }
        if let Some(message) = resize_failed {
            publish_status(SessionStatus::Disconnected(message));
            return;
        }

        let mirror_ids: Vec<String> = mirrors.keys().cloned().collect();
        let mut respawn = Vec::new();
        for id in mirror_ids {
            let Some(mirror) = mirrors.get_mut(&id) else {
                continue;
            };
            loop {
                match poll_observe(
                    mirror,
                    &id,
                    &pane_id,
                    &generation,
                    &publish_status,
                    &publish_frame,
                ) {
                    ObservePoll::Idle => break,
                    ObservePoll::Respawn => {
                        respawn.push(id.clone());
                        break;
                    }
                    ObservePoll::Halt(status) => {
                        release_owned(&mut control, &pane_id);
                        publish_status(status);
                        return;
                    }
                }
            }
        }
        for id in respawn {
            let attempted = mirrors
                .get(&id)
                .is_some_and(|mirror| mirror.resync_attempted);
            mirrors.remove(&id);
            let Some(size) = sizes.get(&id).copied() else {
                continue;
            };
            generation.fetch_add(1, Ordering::AcqRel);
            match ObserveSession::spawn(&id, size.cols, size.rows) {
                Ok(session) => {
                    tracing::info!(pane_id = %id, cols = size.cols, rows = size.rows, "observing pane");
                    mirrors.insert(
                        id,
                        PaneMirror {
                            cols: size.cols,
                            rows: size.rows,
                            session,
                            engine: None,
                            engine_dims: None,
                            frame_count: 0,
                            awaiting_full: true,
                            resync_attempted: attempted,
                        },
                    );
                }
                Err(err) => {
                    publish_status(SessionStatus::Disconnected(err.to_string()));
                    return;
                }
            }
        }

        if let Some(ctrl) = control.as_mut() {
            loop {
                match ctrl.session.recv_timeout(Duration::ZERO) {
                    Err(HerdrError::Timeout) => break,
                    Err(HerdrError::ControlInputRejected(detail)) => {
                        if control_reject_is_resize(&detail) {
                            let (cols, rows) = ctrl.sync.confirmed();
                            tracing::warn!(
                                %detail,
                                cols,
                                rows,
                                "resize rejected; keeping confirmed size"
                            );
                            if let Some(next) = ctrl.sync.reject_inflight() {
                                if let Err(err) = write_resize(&mut ctrl.session, next) {
                                    let message = err.to_string();
                                    release_control(
                                        &mut ctrl.session,
                                        &pane_id,
                                        &mut ctrl.released,
                                    );
                                    publish_status(SessionStatus::Disconnected(message));
                                    return;
                                }
                            }
                        } else {
                            tracing::warn!(%detail, "input rejected");
                        }
                    }
                    Ok(TerminalEvent::Data(frame)) => {
                        match ctrl.sync.on_frame(frame.cols, frame.rows) {
                            FrameFit::Ignore => continue,
                            FrameFit::Acknowledged => {
                                tracing::info!(
                                    cols = frame.cols,
                                    rows = frame.rows,
                                    "resize acknowledged"
                                );
                                if let Some(next) = ctrl.sync.next_pending() {
                                    if let Err(err) = write_resize(&mut ctrl.session, next) {
                                        let message = err.to_string();
                                        release_control(
                                            &mut ctrl.session,
                                            &pane_id,
                                            &mut ctrl.released,
                                        );
                                        publish_status(SessionStatus::Disconnected(message));
                                        return;
                                    }
                                }
                            }
                            FrameFit::Use => {}
                        }
                        ctrl.frame_count = ctrl.frame_count.saturating_add(1);
                        let snapshot = match push_terminal_frame(
                            &mut ctrl.engine,
                            &mut ctrl.engine_dims,
                            &frame,
                        ) {
                            Ok(snapshot) => snapshot,
                            Err(err) => {
                                release_control(&mut ctrl.session, &pane_id, &mut ctrl.released);
                                publish_status(SessionStatus::Controller(err));
                                return;
                            }
                        };
                        let detection_count = parse(&snapshot).node_count();
                        publish_frame(LiveFrame {
                            pane_id: pane_id.clone(),
                            generation: generation.load(Ordering::Acquire),
                            snapshot,
                            frame_count: ctrl.frame_count,
                            detection_count,
                        });
                    }
                    Ok(TerminalEvent::Closed(closed)) => {
                        let reason = closed.reason;
                        release_control(&mut ctrl.session, &pane_id, &mut ctrl.released);
                        publish_status(SessionStatus::Disconnected(reason));
                        return;
                    }
                    Err(err) => {
                        let message = err.to_string();
                        release_control(&mut ctrl.session, &pane_id, &mut ctrl.released);
                        publish_status(SessionStatus::Disconnected(message));
                        return;
                    }
                }
            }
        }

        let scroller_ids: Vec<String> = scrollers.panes.keys().cloned().collect();
        let mut dead_scrollers = Vec::new();
        for id in scroller_ids {
            let Some(hold) = scrollers.panes.get_mut(&id) else {
                continue;
            };
            if let Some(grid) = sizes.get(&id).copied() {
                if let Some(next) = hold.sync.note_desired(grid) {
                    if write_resize(&mut hold.session, next).is_err() {
                        dead_scrollers.push(id);
                        continue;
                    }
                }
            }
            if !drain_hold(hold, &id, &generation, &publish_frame) {
                dead_scrollers.push(id);
            }
        }
        dead_scrollers.sort();
        dead_scrollers.dedup();
        for id in dead_scrollers {
            if let Some(mut hold) = scrollers.panes.remove(&id) {
                release_control(&mut hold.session, &id, &mut hold.released);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(not(feature = "gtk"))]
    #[test]
    fn run_without_gtk_names_devel_packages() {
        let err = run().unwrap_err().to_string();
        assert!(err.contains("gtk4-devel"));
        assert!(err.contains("libadwaita-devel"));
        assert!(err.contains("--features gtk"));
    }

    #[test]
    fn wheel_delta_becomes_three_lines() {
        use crate::herdr::ScrollDirection;
        assert_eq!(scroll_lines_from_delta(0.0), None);
        assert_eq!(
            scroll_lines_from_delta(1.0),
            Some((ScrollDirection::Down, 3))
        );
        assert_eq!(
            scroll_lines_from_delta(-1.0),
            Some((ScrollDirection::Up, 3))
        );
        let mut accum = 0.0;
        assert_eq!(push_scroll_delta(&mut accum, 0.4), None);
        assert_eq!(push_scroll_delta(&mut accum, 0.4), None);
        assert_eq!(
            push_scroll_delta(&mut accum, 0.4),
            Some((ScrollDirection::Down, 1))
        );
        assert!((accum - 0.2).abs() < 1e-9, "{accum}");
    }

    #[test]
    fn wheel_cell_uses_the_centered_grid() {
        use crate::render::cell_under_pointer;
        assert_eq!(
            cell_under_pointer(8.0, 16.0, 80.0, 32.0, 8.0, 16.0, 10, 2, 10, 2),
            Some((1, 1))
        );
        assert_eq!(
            cell_under_pointer(0.0, 0.0, 100.0, 40.0, 8.0, 16.0, 10, 2, 12, 2),
            Some((0, 0))
        );
        assert_eq!(
            cell_under_pointer(14.0, 12.0, 100.0, 40.0, 8.0, 16.0, 10, 2, 12, 2),
            Some((0, 0))
        );
        assert_eq!(
            cell_under_pointer(18.0, 20.0, 100.0, 40.0, 8.0, 16.0, 10, 2, 12, 2),
            Some((1, 1))
        );
    }

    #[test]
    fn session_status_banners() {
        assert_eq!(SessionStatus::Live.banner_text(), None);
        assert!(SessionStatus::ChangedPane
            .banner_text()
            .unwrap()
            .contains("pane"));
        assert!(SessionStatus::Disconnected("eof".into())
            .banner_text()
            .unwrap()
            .contains("disconnected"));
        assert!(SessionStatus::Resyncing
            .banner_text()
            .unwrap()
            .contains("Resyncing"));
        assert!(SessionStatus::Unavailable("down".into())
            .banner_text()
            .unwrap()
            .contains("down"));
        assert!(SessionStatus::Controller("denied".into())
            .banner_text()
            .unwrap()
            .contains("denied"));
        assert!(SessionStatus::ReadOnly
            .banner_text()
            .unwrap()
            .contains("Read-only"));
        assert_eq!(SessionStatus::Controlling.banner_text(), None);
        assert_eq!(
            SessionStatus::FocusError("Focus raced: workspace is w9, not w4".into()).banner_text(),
            Some("Focus raced: workspace is w9, not w4".to_string())
        );
    }

    #[test]
    fn failed_focus_mismatch_and_stale_timer_leave_pane_id_unchanged() {
        let pane_id = "w9:p2";
        let newest = 5u64;

        assert_eq!(
            resolve_attachment(newest, pane_id, SnapshotResult::FocusCommandFailed),
            AttachmentChange::Unchanged
        );

        assert_eq!(
            resolve_attachment(
                newest,
                pane_id,
                SnapshotResult::FocusSnapshot {
                    generation: newest,
                    requested_workspace_id: "w4".into(),
                    focused_workspace_id: Some("w9".into()),
                    focused_pane_id: Some("w4:p1".into()),
                }
            ),
            AttachmentChange::Unchanged
        );

        assert_eq!(
            resolve_attachment(
                newest,
                pane_id,
                SnapshotResult::Refresh {
                    generation: newest - 1,
                    focused_pane_id: Some("w1:p1".into()),
                }
            ),
            AttachmentChange::Unchanged
        );

        assert_eq!(
            resolve_attachment(
                newest,
                pane_id,
                SnapshotResult::FocusSnapshot {
                    generation: newest,
                    requested_workspace_id: "w4".into(),
                    focused_workspace_id: Some("w4".into()),
                    focused_pane_id: Some("w4:p1".into()),
                }
            ),
            AttachmentChange::Reconnect("w4:p1".into())
        );

        assert_eq!(
            resolve_attachment(
                newest,
                pane_id,
                SnapshotResult::FocusSnapshot {
                    generation: newest,
                    requested_workspace_id: "w4".into(),
                    focused_workspace_id: Some("w4".into()),
                    focused_pane_id: None,
                }
            ),
            AttachmentChange::Disconnect
        );

        assert_eq!(
            resolve_attachment(
                newest,
                pane_id,
                SnapshotResult::AgentFocusSnapshot {
                    generation: newest,
                    requested_pane_id: "w4:p1".into(),
                    focused_pane_id: Some("w9:p2".into()),
                }
            ),
            AttachmentChange::Unchanged
        );

        assert_eq!(
            resolve_attachment(
                newest,
                pane_id,
                SnapshotResult::AgentFocusSnapshot {
                    generation: newest - 1,
                    requested_pane_id: "w4:p1".into(),
                    focused_pane_id: Some("w4:p1".into()),
                }
            ),
            AttachmentChange::Unchanged
        );

        assert_eq!(
            resolve_attachment(
                newest,
                pane_id,
                SnapshotResult::AgentFocusSnapshot {
                    generation: newest,
                    requested_pane_id: "w4:p1".into(),
                    focused_pane_id: Some("w4:p1".into()),
                }
            ),
            AttachmentChange::Reconnect("w4:p1".into())
        );

        assert_eq!(
            resolve_attachment(
                newest,
                pane_id,
                SnapshotResult::AgentFocusSnapshot {
                    generation: newest,
                    requested_pane_id: "w4:p1".into(),
                    focused_pane_id: None,
                }
            ),
            AttachmentChange::Disconnect
        );
    }

    #[test]
    fn observe_waits_for_positive_card_capacity() {
        assert!(observe_spawn_size(None).is_none());
        assert!(observe_spawn_size(Some(GridSize {
            cols: 0,
            rows: 24,
            cell_w: 8,
            cell_h: 16,
        }))
        .is_none());
        assert!(observe_spawn_size(Some(GridSize {
            cols: 80,
            rows: 0,
            cell_w: 8,
            cell_h: 16,
        }))
        .is_none());
        let ready = observe_spawn_size(Some(GridSize {
            cols: 100,
            rows: 40,
            cell_w: 9,
            cell_h: 18,
        }))
        .unwrap();
        assert_eq!(
            (ready.cols, ready.rows, ready.cell_w, ready.cell_h),
            (100, 40, 9, 18)
        );
    }

    #[test]
    fn full_frame_mismatch_disconnects_without_drawing() {
        assert_eq!(
            classify_observe_frame(true, 80, 24, 100, 40),
            ObserveFit::Disconnect
        );
        assert_eq!(
            classify_observe_frame(false, 80, 24, 100, 40),
            ObserveFit::Skip
        );
        assert_eq!(
            classify_observe_frame(true, 100, 40, 100, 40),
            ObserveFit::Draw
        );
        assert_eq!(
            classify_observe_frame(false, 100, 40, 100, 40),
            ObserveFit::Draw
        );
    }

    #[test]
    fn resize_sync_keeps_one_inflight_and_confirmed_size() {
        let mut sync = ResizeSync::new(GridSize {
            cols: 80,
            rows: 24,
            cell_w: 8,
            cell_h: 16,
        });
        assert!(sync
            .note_desired(GridSize {
                cols: 80,
                rows: 24,
                cell_w: 8,
                cell_h: 16,
            })
            .is_none());
        let sent = sync
            .note_desired(GridSize {
                cols: 100,
                rows: 30,
                cell_w: 8,
                cell_h: 16,
            })
            .unwrap();
        assert_eq!((sent.cols, sent.rows), (100, 30));
        assert!(sync
            .note_desired(GridSize {
                cols: 110,
                rows: 40,
                cell_w: 8,
                cell_h: 16,
            })
            .is_none());
        assert_eq!(sync.on_frame(80, 24), FrameFit::Use);
        assert_eq!(sync.confirmed(), (80, 24));
        assert_eq!(sync.on_frame(90, 20), FrameFit::Ignore);
        assert_eq!(sync.confirmed(), (80, 24));
        assert_eq!(sync.on_frame(100, 30), FrameFit::Acknowledged);
        assert_eq!(sync.confirmed(), (100, 30));
        let next = sync.next_pending().unwrap();
        assert_eq!((next.cols, next.rows), (110, 40));
        assert!(sync.reject_inflight().is_none());
        assert_eq!(sync.confirmed(), (100, 30));
        assert_eq!(sync.on_frame(100, 30), FrameFit::Use);
        let pixel_only = sync
            .note_desired(GridSize {
                cols: 100,
                rows: 30,
                cell_w: 16,
                cell_h: 32,
            })
            .unwrap();
        assert_eq!((pixel_only.cols, pixel_only.rows), (100, 30));
        assert_eq!(sync.on_frame(100, 30), FrameFit::Acknowledged);
        assert!(control_reject_is_resize(
            "terminal.resize cols and rows must be greater than 0"
        ));
        assert!(!control_reject_is_resize(
            "terminal.input accepts text or bytes, not both"
        ));
    }

    #[test]
    fn fixture_has_separator_and_box() {
        let snap = ShellState::fixture_snapshot();
        assert_eq!(snap.cols(), 10);
        assert_eq!(snap.rows(), 5);
        let row1: String = (0..snap.cols())
            .filter_map(|x| snap.cell(x, 1).map(|c| c.grapheme.as_str()))
            .collect();
        assert!(row1.contains('─'));
        let row2: String = (0..snap.cols())
            .filter_map(|x| snap.cell(x, 2).map(|c| c.grapheme.as_str()))
            .collect();
        assert!(row2.contains('┌'));
        assert!(row2.contains('┐'));
    }

    #[test]
    fn debug_overlay_omits_payload_bytes() {
        let state = ShellState::default();
        let text = state.format_debug_overlay(80, 24, 8.0, 16.0, 1.0);
        assert!(text.contains("pane_id="));
        assert!(text.contains("frames="));
        assert!(!text.contains("payload"));
        assert!(!text.contains("bytes"));
    }

    #[test]
    fn header_uses_workspace_and_pane_placeholders() {
        let state = ShellState::default();
        assert_eq!(state.header_title(), "Workspace · Pane");
        assert_eq!(state.title, "Herdr Native");
    }

    #[test]
    fn newer_generation_replaces_slot_older_does_not() {
        let slot = Mutex::new(HashMap::new());
        let snap_a = TerminalSnapshot::blank(2, 2).unwrap();
        let snap_b = TerminalSnapshot::blank(3, 3).unwrap();
        let snap_c = TerminalSnapshot::blank(4, 4).unwrap();

        assert!(store_live_frame(
            &slot,
            LiveFrame {
                pane_id: "w4:p1".into(),
                generation: 1,
                snapshot: snap_a,
                frame_count: 1,
                detection_count: 0,
            },
        ));
        assert_eq!(slot.lock().unwrap().get("w4:p1").unwrap().generation, 1);

        assert!(store_live_frame(
            &slot,
            LiveFrame {
                pane_id: "w4:p1".into(),
                generation: 2,
                snapshot: snap_b,
                frame_count: 2,
                detection_count: 1,
            },
        ));
        {
            let guard = slot.lock().unwrap();
            let frame = guard.get("w4:p1").unwrap();
            assert_eq!(frame.generation, 2);
            assert_eq!(frame.frame_count, 2);
            assert_eq!(frame.snapshot.cols(), 3);
        }

        assert!(!store_live_frame(
            &slot,
            LiveFrame {
                pane_id: "w4:p1".into(),
                generation: 1,
                snapshot: snap_c,
                frame_count: 99,
                detection_count: 99,
            },
        ));
        {
            let guard = slot.lock().unwrap();
            let frame = guard.get("w4:p1").unwrap();
            assert_eq!(frame.generation, 2);
            assert_eq!(frame.frame_count, 2);
            assert_eq!(frame.snapshot.cols(), 3);
        }
    }

    #[test]
    fn zoom_transition_reaps_hidden_observers() {
        let json = std::fs::read_to_string(
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("docs/contracts/fixtures/layout-zoomed.json"),
        )
        .unwrap();
        let root: serde_json::Value = serde_json::from_str(&json).unwrap();
        let layout =
            crate::herdr::tab_layout(root.pointer("/result/snapshot").unwrap(), "w4", "w4:t1")
                .unwrap();
        let running = vec!["w4:p1".to_string(), "w4:p2".to_string()];
        let diff = plan_observers(&running, &layout.visible_panes(), false, "w4:p1");
        assert_eq!(diff.reap, vec!["w4:p2".to_string()]);
        assert!(diff.spawn.is_empty());
    }

    #[test]
    fn focused_pane_change_releases_control() {
        assert_eq!(focus_control(true, "w4:p1", "w4:p2"), FocusControl::Release);
        assert_eq!(focus_control(true, "w4:p1", "w4:p1"), FocusControl::Keep);
        assert_eq!(focus_control(false, "w4:p1", "w4:p2"), FocusControl::Keep);
        let visible = vec!["w4:p1".to_string(), "w4:p2".to_string()];
        let diff = plan_observers(&[], &visible, false, "w4:p2");
        assert_eq!(diff.spawn, visible);
        assert!(diff.reap.is_empty());
    }
}
