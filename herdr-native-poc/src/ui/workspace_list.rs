//! Workspace names to the left of the terminal card. The terminal widget never
//! sees this list.

use std::cell::RefCell;
use std::rc::Rc;

use crate::herdr::{AgentStatus, WorkspaceRow};

pub struct WorkspaceSidebar {
    widget: gtk::Box,
    list: gtk::ListBox,
    ids: Rc<RefCell<Vec<String>>>,
    marked: Rc<RefCell<Option<String>>>,
}

impl WorkspaceSidebar {
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

        let header = gtk::Label::new(Some("Workspaces"));
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
            ids: Rc::new(RefCell::new(Vec::new())),
            marked: Rc::new(RefCell::new(None)),
        }
    }

    pub fn widget(&self) -> &gtk::Box {
        &self.widget
    }

    pub fn set_rows(&self, rows: &[WorkspaceRow], focused_workspace_id: Option<&str>) {
        use gtk::prelude::*;

        while let Some(child) = self.list.first_child() {
            self.list.remove(&child);
        }

        let mut ids = Vec::with_capacity(rows.len());
        for row in rows {
            let list_row = gtk::ListBoxRow::new();
            list_row.add_css_class("herdr-row");
            list_row.add_css_class("herdr-nav");
            list_row.set_child(Some(&workspace_lines(row)));
            if Some(row.workspace_id.as_str()) == focused_workspace_id {
                list_row.add_css_class("herdr-selected");
            }
            self.list.append(&list_row);
            ids.push(row.workspace_id.clone());
        }
        *self.ids.borrow_mut() = ids;
        *self.marked.borrow_mut() = focused_workspace_id.map(str::to_string);
        self.select_marked();
    }

    /// Put the highlight back on the last snapshot focus after a rejected click.
    pub fn restore_mark(&self) {
        self.select_marked();
    }

    pub fn connect_activated(&self, callback: impl Fn(String) + 'static) {
        use gtk::prelude::*;

        let ids = Rc::clone(&self.ids);
        let marked = Rc::clone(&self.marked);
        self.list.connect_row_activated(move |_, row| {
            let index = row.index();
            if index < 0 {
                return;
            }
            let Some(id) = ids.borrow().get(index as usize).cloned() else {
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
            let index = self.ids.borrow().iter().position(|cand| cand == id)?;
            self.list.row_at_index(index as i32)
        });
        self.list.select_row(row.as_ref());
    }
}

fn workspace_lines(row: &WorkspaceRow) -> gtk::Box {
    use gtk::prelude::*;

    let title = gtk::Label::builder()
        .label(&row.label)
        .halign(gtk::Align::Start)
        .hexpand(true)
        .ellipsize(gtk::pango::EllipsizeMode::End)
        .build();
    title.add_css_class("herdr-title");

    let text = gtk::Box::new(gtk::Orientation::Vertical, 1);
    text.set_hexpand(true);
    text.set_valign(gtk::Align::Center);
    text.append(&title);
    if let Some(meta) = git_line(row.branch.as_deref(), row.git_status.as_deref()) {
        let second = gtk::Label::builder()
            .label(&meta)
            .halign(gtk::Align::Start)
            .hexpand(true)
            .ellipsize(gtk::pango::EllipsizeMode::End)
            .build();
        second.add_css_class("herdr-meta");
        text.append(&second);
    }

    let head = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    head.append(&status_dot(row.agent_status));
    head.append(&text);
    head
}

fn git_line(branch: Option<&str>, status: Option<&str>) -> Option<String> {
    let branch = branch.filter(|text| !text.is_empty());
    let status = status.filter(|text| !text.is_empty());
    match (branch, status) {
        (None, None) => None,
        (Some(branch), None) => Some(branch.to_string()),
        (None, Some(status)) => Some(status.to_string()),
        (Some(branch), Some(status)) => Some(format!("{branch}  {status}")),
    }
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
