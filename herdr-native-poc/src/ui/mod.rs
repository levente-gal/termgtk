//! Presentation chrome for the native shell. Card frame is CSS/GSK, never box glyphs.

#[cfg(feature = "gtk")]
mod agent_list;
mod split;
mod theme;
#[cfg(feature = "gtk")]
mod workspace_list;

pub use theme::Theme;

/// Builds the graphical terminal card (rounded clip, border, shadow via CSS/GSK).
#[cfg(feature = "gtk")]
pub struct ShellChrome {
    root: gtk::Paned,
    host: gtk::Box,
    panes: std::cell::RefCell<Vec<(String, crate::render::TerminalView)>>,
    focused_pane_id: std::cell::RefCell<Option<String>>,
    structure: std::cell::RefCell<Option<String>>,
    enhanced: std::cell::Cell<bool>,
    key_target: std::cell::RefCell<Vec<crate::render::TerminalView>>,
    pointer_wired: std::cell::RefCell<Vec<crate::render::TerminalView>>,
    debug_label: gtk::Label,
    workspaces: workspace_list::WorkspaceSidebar,
    agents: agent_list::AgentPanel,
    pane_header: std::cell::RefCell<Option<String>>,
}

#[cfg(feature = "gtk")]
impl ShellChrome {
    pub fn new(terminal: crate::render::TerminalView) -> Self {
        use gtk::prelude::*;

        terminal.set_hexpand(true);
        terminal.set_vexpand(true);
        terminal.set_halign(gtk::Align::Fill);
        terminal.set_valign(gtk::Align::Fill);
        terminal.set_overflow(gtk::Overflow::Hidden);

        let card = card_for(&terminal);

        let debug_label = gtk::Label::builder()
            .halign(gtk::Align::Start)
            .valign(gtk::Align::Start)
            .margin_top(8)
            .margin_start(8)
            .margin_end(8)
            .wrap(true)
            .xalign(0.0)
            .visible(false)
            .build();
        debug_label.add_css_class("herdr-debug-overlay");
        debug_label.add_css_class("dim-label");
        debug_label.set_can_target(false);

        let card_layer = gtk::Overlay::new();
        card_layer.set_hexpand(true);
        card_layer.set_vexpand(true);
        card_layer.set_overflow(gtk::Overflow::Hidden);
        let host = gtk::Box::new(gtk::Orientation::Vertical, 0);
        host.set_hexpand(true);
        host.set_vexpand(true);
        host.set_overflow(gtk::Overflow::Hidden);
        host.append(&card);
        card_layer.set_child(Some(&host));
        card_layer.add_overlay(&debug_label);

        let workspaces = workspace_list::WorkspaceSidebar::new();
        let agents = agent_list::AgentPanel::new();
        let side = gtk::Box::new(gtk::Orientation::Vertical, 8);
        side.set_hexpand(true);
        side.set_vexpand(true);
        side.append(workspaces.widget());
        side.append(agents.widget());
        let root = gtk::Paned::new(gtk::Orientation::Horizontal);
        root.add_css_class("herdr-split");
        root.set_wide_handle(true);
        root.set_resize_start_child(false);
        root.set_shrink_start_child(true);
        root.set_resize_end_child(true);
        root.set_shrink_end_child(true);
        root.set_margin_top(8);
        root.set_margin_bottom(8);
        root.set_margin_start(8);
        root.set_margin_end(8);
        root.set_hexpand(true);
        root.set_vexpand(true);
        root.set_start_child(Some(&side));
        root.set_end_child(Some(&card_layer));
        root.set_position(248);

        Self {
            root,
            host,
            panes: std::cell::RefCell::new(vec![(String::new(), terminal)]),
            focused_pane_id: std::cell::RefCell::new(None),
            structure: std::cell::RefCell::new(None),
            enhanced: std::cell::Cell::new(false),
            key_target: std::cell::RefCell::new(Vec::new()),
            pointer_wired: std::cell::RefCell::new(Vec::new()),
            debug_label,
            workspaces,
            agents,
            pane_header: std::cell::RefCell::new(None),
        }
    }

    /// Replace the card tree when the split structure changes.
    ///
    /// Returns true when widgets were rebuilt. A focus-only change keeps the
    /// views and returns false. A click focuses a neighbor with
    /// `herdr pane focus --direction`.
    pub fn apply_tab_layout(&self, layout: &crate::herdr::TabLayout) -> bool {
        use gtk::prelude::*;

        let key = structure_key(layout);
        if self.structure.borrow().as_deref() == Some(key.as_str()) {
            self.note_focus(&layout.focused_pane_id);
            self.restyle_panes();
            return false;
        }
        let (widget, panes) = build_layout(&layout.root, &layout.focused_pane_id);
        while let Some(child) = self.host.first_child() {
            self.host.remove(&child);
        }
        self.key_target.borrow_mut().clear();
        self.pointer_wired.borrow_mut().clear();
        self.host.append(&widget);
        *self.panes.borrow_mut() = panes;
        *self.structure.borrow_mut() = Some(key);
        self.note_focus(&layout.focused_pane_id);
        self.restyle_panes();
        true
    }

    pub fn panes(&self) -> Vec<(String, crate::render::TerminalView)> {
        self.panes.borrow().clone()
    }

    pub fn set_enhanced(&self, enhanced: bool) {
        self.enhanced.set(enhanced);
        self.restyle_panes();
    }

    /// Raw panes wear `herdr-card`. Enhanced panes sit on the shell background.
    pub fn set_pane_header(&self, title: Option<&str>) {
        let next = title
            .map(str::trim)
            .filter(|text| !text.is_empty())
            .map(str::to_string);
        if self.pane_header.borrow().as_deref() == next.as_deref() {
            return;
        }
        *self.pane_header.borrow_mut() = next;
        self.restyle_panes();
    }

    fn restyle_panes(&self) {
        let enhanced = self.enhanced.get();
        let header = self.pane_header.borrow().clone();
        let focused = self.focused_pane_id.borrow().clone();
        for (pane_id, view) in self.panes.borrow().iter() {
            view.set_enhanced(enhanced);
            style_pane_frame(
                view,
                enhanced,
                focused.as_deref() == Some(pane_id.as_str()),
                header.as_deref(),
            );
        }
    }

    pub fn enhanced(&self) -> bool {
        self.enhanced.get()
    }

    pub fn key_target_is(&self, view: &crate::render::TerminalView) -> bool {
        self.key_target.borrow().iter().any(|wired| wired == view)
    }

    pub fn remember_key_target(&self, view: &crate::render::TerminalView) {
        let mut wired = self.key_target.borrow_mut();
        if !wired.iter().any(|existing| existing == view) {
            wired.push(view.clone());
        }
    }

    pub fn pointer_wired(&self, view: &crate::render::TerminalView) -> bool {
        self.pointer_wired
            .borrow()
            .iter()
            .any(|wired| wired == view)
    }

    pub fn remember_pointer(&self, view: &crate::render::TerminalView) {
        let mut wired = self.pointer_wired.borrow_mut();
        if !wired.iter().any(|existing| existing == view) {
            wired.push(view.clone());
        }
    }

    pub fn focused_pane_id(&self) -> Option<String> {
        self.focused_pane_id.borrow().clone()
    }

    pub fn host(&self) -> &gtk::Box {
        &self.host
    }

    fn note_focus(&self, focused: &str) {
        use gtk::prelude::*;
        *self.focused_pane_id.borrow_mut() = Some(focused.to_string());
        for (pane_id, view) in self.panes.borrow().iter() {
            let on = pane_id == focused;
            view.set_can_focus(on);
            view.set_focusable(on);
            view.set_focus_on_click(on);
        }
    }

    pub fn root(&self) -> &gtk::Paned {
        &self.root
    }

    pub fn set_workspaces(
        &self,
        rows: &[crate::herdr::WorkspaceRow],
        focused_workspace_id: Option<&str>,
        agents: &[crate::herdr::AgentRow],
        focused_pane_id: Option<&str>,
    ) {
        self.workspaces.set_rows(rows, focused_workspace_id);
        self.agents.set_rows(agents, focused_pane_id);
    }

    pub fn restore_workspace_mark(&self) {
        self.workspaces.restore_mark();
    }

    pub fn restore_agent_mark(&self) {
        self.agents.restore_mark();
    }

    pub fn connect_workspace_activated(&self, callback: impl Fn(String) + 'static) {
        self.workspaces.connect_activated(callback);
    }

    pub fn connect_agent_activated(&self, callback: impl Fn(String) + 'static) {
        self.agents.connect_activated(callback);
    }

    pub fn card(&self) -> &gtk::Box {
        &self.host
    }

    /// The Herdr-focused pane, or the single pre-layout card.
    pub fn terminal(&self) -> crate::render::TerminalView {
        let focused = self.focused_pane_id.borrow().clone();
        let panes = self.panes.borrow();
        if let Some(id) = focused.as_deref() {
            if let Some((_, view)) = panes.iter().find(|(pane_id, _)| pane_id == id) {
                return view.clone();
            }
        }
        panes
            .first()
            .map(|(_, view)| view.clone())
            .expect("card has a terminal")
    }

    pub fn set_debug_visible(&self, visible: bool) {
        use gtk::prelude::*;
        self.debug_label.set_visible(visible);
    }

    pub fn set_debug_text(&self, text: &str) {
        self.debug_label.set_text(text);
    }

    pub fn refresh_debug(&self, state: &crate::app::ShellState) {
        use gtk::prelude::*;
        let terminal = self.terminal();
        let width = f64::from(terminal.width()).max(1.0);
        let height = f64::from(terminal.height()).max(1.0);
        let (cols, rows) = terminal.cols_rows(width, height);
        self.set_debug_text(&state.format_debug_overlay(
            cols,
            rows,
            terminal.cell_width(),
            terminal.cell_height(),
            terminal.scale(),
        ));
    }

    /// Paint card chrome with Snapshot/GSK (rounded clip, border, outset shadow).
    pub fn paint_card_chrome(snapshot: &gtk::Snapshot, width: f32, height: f32) {
        use gtk::gdk;
        use gtk::gsk;
        use gtk::prelude::{SnapshotExt, SnapshotExtManual};

        if width <= 0.0 || height <= 0.0 {
            return;
        }

        let radius = Theme::RADIUS_PX;
        let bounds = gtk::graphene::Rect::new(0.0, 0.0, width, height);
        let outline = gsk::RoundedRect::from_rect(bounds, radius);

        snapshot.append_outset_shadow(
            &outline,
            &gdk::RGBA::new(0.0, 0.0, 0.0, Theme::SHADOW_ALPHA),
            0.0,
            Theme::SHADOW_OFFSET_Y,
            0.0,
            Theme::SHADOW_BLUR,
        );

        let (cr, cg, cb) = Theme::CARD;
        snapshot.push_rounded_clip(&outline);
        snapshot.append_color(
            &gdk::RGBA::new(
                f32::from(cr) / 255.0,
                f32::from(cg) / 255.0,
                f32::from(cb) / 255.0,
                1.0,
            ),
            &bounds,
        );
        snapshot.pop();

        let border = gdk::RGBA::new(1.0, 1.0, 1.0, Theme::BORDER_ALPHA);
        snapshot.append_border(&outline, &[1.0; 4], &[border; 4]);
    }
}

#[cfg(feature = "gtk")]
fn card_for(terminal: &crate::render::TerminalView) -> gtk::Box {
    use gtk::prelude::*;

    let card = gtk::Box::new(gtk::Orientation::Vertical, 0);
    card.add_css_class(Theme::CSS_CARD_CLASS);
    card.set_hexpand(true);
    card.set_vexpand(true);
    card.set_overflow(gtk::Overflow::Hidden);
    card.append(terminal);
    card
}

/// Swap the pane wrapper between a titled card and a bare canvas.
#[cfg(feature = "gtk")]
fn style_pane_frame(
    view: &crate::render::TerminalView,
    enhanced: bool,
    focused: bool,
    header: Option<&str>,
) {
    use gtk::prelude::*;

    let Some(card) = view
        .parent()
        .and_then(|widget| widget.downcast::<gtk::Box>().ok())
    else {
        return;
    };
    if enhanced {
        card.remove_css_class(Theme::CSS_CARD_CLASS);
        remove_pane_header(&card);
        return;
    }
    card.add_css_class(Theme::CSS_CARD_CLASS);
    card.set_overflow(gtk::Overflow::Hidden);
    if focused {
        if let Some(text) = header {
            ensure_pane_header(&card, text);
            return;
        }
    }
    remove_pane_header(&card);
}

#[cfg(feature = "gtk")]
fn remove_pane_header(card: &gtk::Box) {
    use gtk::prelude::*;

    let mut child = card.first_child();
    while let Some(node) = child {
        let next = node.next_sibling();
        if node.has_css_class("herdr-pane-header") {
            card.remove(&node);
        }
        child = next;
    }
}

#[cfg(feature = "gtk")]
fn ensure_pane_header(card: &gtk::Box, text: &str) {
    use gtk::prelude::*;

    let mut child = card.first_child();
    while let Some(node) = child {
        if node.has_css_class("herdr-pane-header") {
            if let Some(label) = node.downcast_ref::<gtk::Label>() {
                label.set_label(text);
            }
            return;
        }
        child = node.next_sibling();
    }
    let label = gtk::Label::new(Some(text));
    label.set_xalign(0.0);
    label.set_halign(gtk::Align::Fill);
    label.set_ellipsize(gtk::pango::EllipsizeMode::End);
    label.add_css_class("herdr-card-header");
    label.add_css_class("herdr-pane-header");
    card.prepend(&label);
}

#[cfg(feature = "gtk")]
fn build_layout(
    node: &crate::herdr::LayoutNode,
    focused: &str,
) -> (gtk::Widget, Vec<(String, crate::render::TerminalView)>) {
    use crate::herdr::{LayoutNode, SplitDirection};
    use crate::render::TerminalView;
    use gtk::prelude::*;

    match node {
        LayoutNode::Pane { pane_id } => {
            let view = TerminalView::new();
            view.set_hexpand(true);
            view.set_vexpand(true);
            view.set_halign(gtk::Align::Fill);
            view.set_valign(gtk::Align::Fill);
            view.set_overflow(gtk::Overflow::Hidden);
            let on = pane_id == focused;
            view.set_can_focus(on);
            view.set_focusable(on);
            view.set_focus_on_click(on);
            let card = card_for(&view);
            (card.upcast(), vec![(pane_id.clone(), view)])
        }
        LayoutNode::Split {
            direction,
            ratio,
            first,
            second,
            ..
        } => {
            let (first_widget, mut views) = build_layout(first, focused);
            let (second_widget, more) = build_layout(second, focused);
            views.extend(more);
            let orientation = match direction {
                SplitDirection::Right => gtk::Orientation::Horizontal,
                SplitDirection::Down => gtk::Orientation::Vertical,
            };
            let split =
                split::RatioSplit::new(orientation, ratio.get(), &first_widget, &second_widget);
            (split.upcast(), views)
        }
    }
}

#[cfg(feature = "gtk")]
fn structure_key(layout: &crate::herdr::TabLayout) -> String {
    fn walk(node: &crate::herdr::LayoutNode) -> String {
        match node {
            crate::herdr::LayoutNode::Pane { pane_id } => format!("p:{pane_id}"),
            crate::herdr::LayoutNode::Split {
                id,
                direction,
                ratio,
                first,
                second,
            } => format!(
                "s:{id}:{direction:?}:{:.8}:({})({})",
                ratio.get(),
                walk(first),
                walk(second)
            ),
        }
    }
    format!(
        "{}|{}|{}|{}",
        layout.workspace_id,
        layout.tab_id,
        layout.zoomed,
        walk(&layout.root)
    )
}
