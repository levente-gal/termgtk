//! Agents under the workspace list. The terminal widget never sees this list.

use std::cell::RefCell;
use std::rc::Rc;

use crate::herdr::{AgentRow, AgentStatus};

pub struct AgentPanel {
    widget: gtk::Box,
    list: gtk::ListBox,
    slots: Rc<RefCell<Vec<Option<String>>>>,
    marked: Rc<RefCell<Option<String>>>,
}

impl AgentPanel {
    pub fn new() -> Self {
        use gtk::prelude::*;

        let list = gtk::ListBox::new();
        list.set_selection_mode(gtk::SelectionMode::Single);
        list.add_css_class("herdr-rows");
        list.set_vexpand(true);
        list.set_hexpand(true);

        let scroll = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vscrollbar_policy(gtk::PolicyType::Automatic)
            .hexpand(true)
            .vexpand(true)
            .child(&list)
            .build();
        scroll.add_css_class("herdr-scroll");

        let header = gtk::Label::new(Some("Agents"));
        header.set_xalign(0.0);
        header.set_halign(gtk::Align::Fill);
        header.add_css_class("herdr-card-header");

        let widget = gtk::Box::new(gtk::Orientation::Vertical, 0);
        widget.add_css_class("herdr-card");
        widget.set_width_request(160);
        widget.set_hexpand(true);
        widget.set_vexpand(true);
        widget.set_overflow(gtk::Overflow::Hidden);
        widget.append(&header);
        widget.append(&scroll);

        Self {
            widget,
            list,
            slots: Rc::new(RefCell::new(Vec::new())),
            marked: Rc::new(RefCell::new(None)),
        }
    }

    pub fn widget(&self) -> &gtk::Box {
        &self.widget
    }

    pub fn set_rows(&self, rows: &[AgentRow], focused_pane_id: Option<&str>) {
        use gtk::prelude::*;

        while let Some(child) = self.list.first_child() {
            self.list.remove(&child);
        }

        let mut slots = Vec::new();
        let mut group: Option<&str> = None;
        for row in rows {
            if group != Some(row.workspace_id.as_str()) {
                group = Some(row.workspace_id.as_str());
                self.list.append(&group_header(&row.workspace_label));
                slots.push(None);
            }
            self.list.append(&agent_row(row, focused_pane_id));
            slots.push(Some(row.pane_id.clone()));
        }
        *self.slots.borrow_mut() = slots;
        *self.marked.borrow_mut() = focused_pane_id.map(str::to_string);
        self.select_marked();
    }

    /// Put the highlight back on the last snapshot focus after a rejected click.
    pub fn restore_mark(&self) {
        self.select_marked();
    }

    pub fn connect_activated(&self, callback: impl Fn(String) + 'static) {
        use gtk::prelude::*;

        let slots = Rc::clone(&self.slots);
        let marked = Rc::clone(&self.marked);
        self.list.connect_row_activated(move |_, row| {
            let index = row.index();
            if index < 0 {
                return;
            }
            let Some(Some(id)) = slots.borrow().get(index as usize).cloned() else {
                return;
            };
            if marked.borrow().as_deref() == Some(id.as_str()) {
                return;
            }
            callback(id);
        });
    }

    fn select_marked(&self) {
        let marked = self.marked.borrow().clone();
        let row = marked.as_deref().and_then(|id| {
            let index = self
                .slots
                .borrow()
                .iter()
                .position(|slot| slot.as_deref() == Some(id))?;
            self.list.row_at_index(index as i32)
        });
        self.list.select_row(row.as_ref());
    }
}

fn group_header(label: &str) -> gtk::ListBoxRow {
    use gtk::prelude::*;

    let title = gtk::Label::builder()
        .label(label)
        .halign(gtk::Align::Start)
        .margin_top(8)
        .margin_bottom(2)
        .margin_start(8)
        .margin_end(8)
        .build();
    title.add_css_class("dim-label");
    let row = gtk::ListBoxRow::new();
    row.set_child(Some(&title));
    row.set_activatable(false);
    row.set_selectable(false);
    row
}

fn agent_row(row: &AgentRow, focused_pane_id: Option<&str>) -> gtk::ListBoxRow {
    use gtk::prelude::*;

    let place = join_bits(&[row.workspace_label.as_str(), row.tab_label.as_str()]);
    let title = gtk::Label::builder()
        .label(&place)
        .halign(gtk::Align::Start)
        .hexpand(true)
        .ellipsize(gtk::pango::EllipsizeMode::End)
        .build();
    title.add_css_class("herdr-title");

    let text = gtk::Box::new(gtk::Orientation::Vertical, 1);
    text.set_hexpand(true);
    text.set_valign(gtk::Align::Center);
    text.append(&title);
    if !row.identity.is_empty() {
        let identity = gtk::Label::builder()
            .label(&row.identity)
            .halign(gtk::Align::Start)
            .hexpand(true)
            .ellipsize(gtk::pango::EllipsizeMode::End)
            .build();
        identity.add_css_class("herdr-meta");
        text.append(&identity);
    }

    let head = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    head.append(&status_dot(row.agent_status));
    head.append(&text);

    let list_row = gtk::ListBoxRow::new();
    list_row.add_css_class("herdr-row");
    list_row.add_css_class("herdr-nav");
    list_row.set_child(Some(&head));
    if Some(row.pane_id.as_str()) == focused_pane_id {
        list_row.add_css_class("herdr-selected");
    }
    list_row
}

fn join_bits(parts: &[&str]) -> String {
    parts
        .iter()
        .copied()
        .filter(|text| !text.is_empty())
        .collect::<Vec<_>>()
        .join("  ")
}

fn status_dot(status: AgentStatus) -> gtk::Box {
    use gtk::prelude::*;

    let dot = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    dot.add_css_class("herdr-status");
    dot.add_css_class(status.mark_class());
    dot.set_size_request(8, 8);
    dot.set_valign(gtk::Align::Center);
    dot.set_halign(gtk::Align::Center);
    dot
}
