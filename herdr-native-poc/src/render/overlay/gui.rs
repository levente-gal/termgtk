//! GTK cards for the layout tree. Cards are click-through so the terminal
//! under them keeps wheel and two-finger scroll. Overflow is a fade and an
//! optional `N of M` chip, not a GTK scrollbar.

use std::collections::{HashMap, HashSet};

use gtk::prelude::*;
use gtk::{gdk, graphene, pango};

use crate::render::cell_renderer::{paint_colors, resolve_fg, Rgba};

use super::cards::{
    card_stylesheet, inset_card, pick_focused, resolve_slots, ring_class, CardSlot, FrameRect,
    PaneFace,
};
use super::tree::{
    line_text, node_body, node_children, node_id, node_rect, node_scroll, node_thumb, node_title,
    ContentLine, LayoutTree, PaneBody, Span,
};

pub(crate) struct PaneGui {
    layout: CardBoard,
    dialogs: CardBoard,
    slots: HashMap<u64, Slot>,
    chrome: Vec<gtk::Label>,
    chrome_signature: String,
    css: gtk::CssProvider,
    css_signature: String,
    css_installed: bool,
    dialogs_open: bool,
    attached: bool,
    origin_x: f64,
    origin_y: f64,
}

struct Slot {
    card: gtk::Box,
    header: gtk::Box,
    title: gtk::Label,
    count: gtk::Label,
    body: gtk::Box,
    fade: gtk::Widget,
    geom: CardSlot,
    signature: String,
    layer: Layer,
    ring: Option<String>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Layer {
    Layout,
    Dialog,
}

struct Chrome {
    focused: Option<u64>,
    dialog_open: bool,
    ring_class: Option<String>,
}

impl PaneGui {
    pub(crate) fn new() -> Self {
        let layout = CardBoard::new();
        let dialogs = CardBoard::new();
        Self {
            layout,
            dialogs,
            slots: HashMap::new(),
            chrome: Vec::new(),
            chrome_signature: String::new(),
            css: gtk::CssProvider::new(),
            css_signature: String::new(),
            css_installed: false,
            dialogs_open: false,
            attached: false,
            origin_x: 0.0,
            origin_y: 0.0,
        }
    }

    pub(crate) fn attach(&mut self, parent: &gtk::Widget) {
        if self.attached {
            return;
        }
        self.layout.set_parent(parent);
        self.dialogs.set_parent(parent);
        self.attached = true;
        self.install_css();
    }

    pub(crate) fn detach(&mut self) {
        if !self.attached {
            return;
        }
        self.layout.unparent();
        self.dialogs.unparent();
        self.attached = false;
    }

    pub(crate) fn place(
        &mut self,
        origin_x: f64,
        origin_y: f64,
        cols: u16,
        rows: u16,
        cell_w: f64,
        cell_h: f64,
    ) {
        self.origin_x = origin_x;
        self.origin_y = origin_y;
        if !self.attached {
            return;
        }
        let width = (f64::from(cols) * cell_w).round().max(1.0) as i32;
        let height = (f64::from(rows) * cell_h).round().max(1.0) as i32;
        let point = graphene::Point::new(origin_x as f32, origin_y as f32);
        self.layout.allocate(
            width,
            height,
            -1,
            Some(gtk::gsk::Transform::default().translate(&point)),
        );
        self.dialogs.allocate(
            width,
            height,
            -1,
            Some(gtk::gsk::Transform::default().translate(&point)),
        );
    }

    pub(crate) fn layout_widget(&self) -> gtk::Widget {
        self.layout.clone().upcast()
    }

    pub(crate) fn dialogs_widget(&self) -> gtk::Widget {
        self.dialogs.clone().upcast()
    }

    pub(crate) fn dialogs_open(&self) -> bool {
        self.dialogs_open
    }

    pub(crate) fn sync(&mut self, tree: &LayoutTree, cell_w: f64, cell_h: f64) {
        self.install_css();
        let layout_faces = faces(tree.layout());
        let dialog_faces = faces(tree.overlays());
        let dialog_open = !dialog_faces.is_empty();
        self.dialogs_open = dialog_open;
        let focused = if dialog_open {
            dialog_faces.last().map(|face| face.id)
        } else {
            pick_focused(&layout_faces)
        };
        let ring = focused.and_then(|id| {
            layout_faces
                .iter()
                .chain(&dialog_faces)
                .find(|face| face.id == id)
                .map(|face| resolve_fg(face.border))
        });
        self.apply_css(ring);
        let chrome = Chrome {
            focused,
            dialog_open,
            ring_class: ring.map(ring_class),
        };
        let mut seen = HashSet::new();
        let layout_slots = pane_slots(tree.layout(), cell_w, cell_h, true);
        let dialog_slots = pane_slots(tree.overlays(), cell_w, cell_h, false);
        self.sync_nodes(
            tree.layout(),
            cell_h,
            &layout_slots,
            Layer::Layout,
            &chrome,
            &mut seen,
        );
        self.sync_nodes(
            tree.overlays(),
            cell_h,
            &dialog_slots,
            Layer::Dialog,
            &chrome,
            &mut seen,
        );
        self.drop_unseen(&seen);
        let (cols, _) = tree.screen();
        let grid_w = (f64::from(cols) * cell_w).round().max(1.0) as i32;
        self.sync_chrome(tree, cell_h, grid_w);
    }

    fn install_css(&mut self) {
        if self.css_installed || !self.attached {
            return;
        }
        gtk::style_context_add_provider_for_display(
            &self.layout.display(),
            &self.css,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
        self.css_installed = true;
    }

    fn apply_css(&mut self, ring: Option<Rgba>) {
        let css = card_stylesheet(ring);
        if css == self.css_signature {
            return;
        }
        self.css.load_from_string(&css);
        self.css_signature = css;
    }

    fn sync_nodes(
        &mut self,
        nodes: &[super::tree::Node],
        cell_h: f64,
        slots: &HashMap<u64, CardSlot>,
        layer: Layer,
        chrome: &Chrome,
        seen: &mut HashSet<u64>,
    ) {
        for node in nodes {
            if is_card(node) {
                self.sync_node(node, cell_h, slots, layer, chrome, seen);
            }
            self.sync_nodes(node_children(node), cell_h, slots, layer, chrome, seen);
        }
    }

    fn sync_node(
        &mut self,
        node: &super::tree::Node,
        cell_h: f64,
        slots: &HashMap<u64, CardSlot>,
        layer: Layer,
        chrome: &Chrome,
        seen: &mut HashSet<u64>,
    ) {
        let id = node_id(node);
        let Some(card_slot) = slots.get(&id).copied() else {
            return;
        };
        seen.insert(id);
        let row_h = cell_h.round().max(1.0) as i32;
        let signature = card_signature(node, row_h);
        let board = match layer {
            Layer::Layout => self.layout.clone(),
            Layer::Dialog => self.dialogs.clone(),
        };
        if self.slots.contains_key(&id) {
            let (old_layer, needs_refill) = {
                let slot = self.slots.get(&id).expect("slot");
                (slot.layer, slot.signature != signature)
            };
            if old_layer != layer {
                let slot = self.slots.get(&id).expect("slot");
                match old_layer {
                    Layer::Layout => self.layout.remove_child(&slot.card),
                    Layer::Dialog => self.dialogs.remove_child(&slot.card),
                }
            }
            let slot = self.slots.get_mut(&id).expect("slot");
            if needs_refill {
                refill_card(slot, node, row_h);
                slot.signature = signature;
            }
            slot.layer = layer;
            place_card(&board, slot, card_slot);
            paint_role(&slot.card, layer, id, chrome, &mut slot.ring);
            return;
        }
        let mut slot = new_slot(node, row_h, layer, signature);
        paint_role(&slot.card, layer, id, chrome, &mut slot.ring);
        place_card(&board, &mut slot, card_slot);
        self.slots.insert(id, slot);
    }

    fn drop_unseen(&mut self, seen: &HashSet<u64>) {
        let stale: Vec<u64> = self
            .slots
            .keys()
            .copied()
            .filter(|id| !seen.contains(id))
            .collect();
        for id in stale {
            if let Some(slot) = self.slots.remove(&id) {
                match slot.layer {
                    Layer::Layout => self.layout.remove_child(&slot.card),
                    Layer::Dialog => self.dialogs.remove_child(&slot.card),
                }
            }
        }
    }

    fn sync_chrome(&mut self, tree: &LayoutTree, cell_h: f64, grid_w: i32) {
        let signature = tree
            .chrome()
            .iter()
            .map(|row| format!("{}:{}", row.y, line_text(&row.line)))
            .collect::<Vec<_>>()
            .join("\n");
        let height = cell_h.round().max(1.0) as i32;
        if signature != self.chrome_signature {
            for label in self.chrome.drain(..) {
                self.layout.remove_child(&label);
            }
            for row in tree.chrome() {
                let label = line_label(&row.line, true);
                inert(&label);
                self.layout
                    .set_slot(&label, chrome_slot(row.y, cell_h, grid_w, height));
                self.chrome.push(label);
            }
            self.chrome_signature = signature;
            return;
        }
        for (label, row) in self.chrome.iter().zip(tree.chrome()) {
            self.layout
                .set_slot(label, chrome_slot(row.y, cell_h, grid_w, height));
        }
    }
}

fn chrome_slot(y: u16, cell_h: f64, grid_w: i32, height: i32) -> CardSlot {
    CardSlot {
        x: 0.0,
        y: f64::from(y) * cell_h,
        width: grid_w,
        height,
    }
}

fn pane_slots(
    nodes: &[super::tree::Node],
    cell_w: f64,
    cell_h: f64,
    tile: bool,
) -> HashMap<u64, CardSlot> {
    let frames = frames_of(nodes);
    if tile {
        let rects: Vec<FrameRect> = frames.iter().map(|(_, rect)| *rect).collect();
        return resolve_slots(&rects, cell_w, cell_h)
            .into_iter()
            .zip(frames)
            .map(|(slot, (id, _))| (id, slot))
            .collect();
    }
    frames
        .into_iter()
        .map(|(id, rect)| (id, inset_frame(rect, cell_w, cell_h)))
        .collect()
}

fn frames_of(nodes: &[super::tree::Node]) -> Vec<(u64, FrameRect)> {
    let mut out = Vec::new();
    collect_frames(nodes, &mut out);
    out
}

fn collect_frames(nodes: &[super::tree::Node], out: &mut Vec<(u64, FrameRect)>) {
    for node in nodes {
        if is_card(node) {
            let (x, y, cols, rows, _) = node_rect(node);
            out.push((node_id(node), FrameRect { x, y, cols, rows }));
        }
        collect_frames(node_children(node), out);
    }
}

/// Empty parents are grouping rects. Empty leaves still get a card and a slot.
fn is_card(node: &super::tree::Node) -> bool {
    node_children(node).is_empty() || !matches!(node_body(node), PaneBody::Empty)
}

fn inset_frame(rect: FrameRect, cell_w: f64, cell_h: f64) -> CardSlot {
    let (x, y, width, height) = inset_card(
        f64::from(rect.x) * cell_w,
        f64::from(rect.y) * cell_h,
        f64::from(rect.cols) * cell_w,
        f64::from(rect.rows) * cell_h,
    );
    CardSlot {
        x,
        y,
        width,
        height,
    }
}

fn faces(nodes: &[super::tree::Node]) -> Vec<PaneFace> {
    let mut out = Vec::new();
    collect_faces(nodes, &mut out);
    out
}

fn collect_faces(nodes: &[super::tree::Node], out: &mut Vec<PaneFace>) {
    for node in nodes {
        if is_card(node) {
            let (.., border) = node_rect(node);
            out.push(PaneFace {
                id: node_id(node),
                border,
            });
        }
        collect_faces(node_children(node), out);
    }
}

fn paint_role(card: &gtk::Box, layer: Layer, id: u64, chrome: &Chrome, ring: &mut Option<String>) {
    if layer == Layer::Dialog {
        card.add_css_class("herdr-dialog");
    } else {
        card.remove_css_class("herdr-dialog");
    }
    let dimmed = match layer {
        Layer::Dialog => false,
        Layer::Layout if chrome.dialog_open => true,
        Layer::Layout => chrome.focused.is_some_and(|focused| focused != id),
    };
    if dimmed {
        card.add_css_class("herdr-dimmed");
    } else {
        card.remove_css_class("herdr-dimmed");
    }
    let focused = chrome.focused == Some(id);
    if focused {
        card.add_css_class("focused");
    } else {
        card.remove_css_class("focused");
    }
    if let Some(old) = ring.take() {
        card.remove_css_class(&old);
    }
    if focused {
        if let Some(name) = &chrome.ring_class {
            card.add_css_class(name);
            *ring = Some(name.clone());
        }
    }
}

fn new_slot(node: &super::tree::Node, row_h: i32, layer: Layer, signature: String) -> Slot {
    let card = gtk::Box::new(gtk::Orientation::Vertical, 0);
    card.add_css_class("herdr-card");
    card.set_overflow(gtk::Overflow::Hidden);
    card.set_halign(gtk::Align::Fill);
    card.set_valign(gtk::Align::Fill);
    inert(&card);

    let title = gtk::Label::new(None);
    title.set_ellipsize(pango::EllipsizeMode::End);
    title.set_max_width_chars(1);
    title.set_xalign(0.0);
    title.set_halign(gtk::Align::Fill);
    title.set_hexpand(true);
    title.add_css_class("herdr-card-header");
    inert(&title);

    let count = gtk::Label::new(None);
    count.set_xalign(1.0);
    count.add_css_class("herdr-meta");
    count.add_css_class("herdr-card-count");
    inert(&count);

    let header = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    header.set_hexpand(true);
    header.append(&title);
    header.append(&count);
    inert(&header);

    let body = gtk::Box::new(gtk::Orientation::Vertical, 0);
    body.add_css_class("herdr-pane-body");
    body.set_hexpand(true);
    body.set_vexpand(true);
    body.set_halign(gtk::Align::Fill);
    body.set_valign(gtk::Align::Fill);
    inert(&body);

    let fade = gtk::Box::new(gtk::Orientation::Vertical, 0);
    fade.add_css_class("herdr-fade");
    fade.set_halign(gtk::Align::Fill);
    fade.set_valign(gtk::Align::End);
    fade.set_hexpand(true);
    fade.set_vexpand(false);
    inert(&fade);

    let pane = gtk::Overlay::new();
    pane.set_hexpand(true);
    pane.set_vexpand(true);
    pane.set_halign(gtk::Align::Fill);
    pane.set_valign(gtk::Align::Fill);
    pane.set_child(Some(&body));
    pane.add_overlay(&fade);
    inert(&pane);

    card.append(&header);
    card.append(&pane);

    let mut slot = Slot {
        card,
        header,
        title,
        count,
        body,
        fade: fade.upcast(),
        geom: CardSlot {
            x: 0.0,
            y: 0.0,
            width: 0,
            height: 0,
        },
        signature,
        layer,
        ring: None,
    };
    refill_card(&mut slot, node, row_h);
    slot
}

fn refill_card(slot: &mut Slot, node: &super::tree::Node, row_h: i32) {
    fill_body(&slot.body, node, row_h);
    apply_overflow(slot, node, row_h);
}

fn fill_body(body: &gtk::Box, node: &super::tree::Node, row_h: i32) {
    while let Some(child) = body.first_child() {
        body.remove(&child);
    }
    let content: gtk::Widget = match node_body(node) {
        PaneBody::Rows(lines) => rows_list(lines, row_h).upcast(),
        PaneBody::Text(lines) => text_view(lines).upcast(),
        PaneBody::Empty => return,
    };
    inert(&content);
    content.set_hexpand(true);
    content.set_vexpand(true);
    content.set_halign(gtk::Align::Fill);
    content.set_valign(gtk::Align::Start);
    body.append(&content);
}

fn rows_list(lines: &[ContentLine], row_h: i32) -> gtk::ListBox {
    let list = gtk::ListBox::new();
    list.set_selection_mode(gtk::SelectionMode::None);
    list.add_css_class("herdr-rows");
    list.set_hexpand(true);
    list.set_vexpand(true);
    list.set_halign(gtk::Align::Fill);
    list.set_valign(gtk::Align::Fill);
    inert(&list);
    for line in lines {
        let label = line_label(line, false);
        label.set_valign(gtk::Align::Center);
        let row = gtk::ListBoxRow::new();
        row.add_css_class("herdr-row");
        row.set_size_request(-1, row_h);
        if line.selected {
            row.add_css_class("herdr-selected");
        }
        row.set_child(Some(&label));
        inert(&row);
        inert(&label);
        list.append(&row);
    }
    list
}

fn text_view(lines: &[ContentLine]) -> gtk::TextView {
    let view = gtk::TextView::builder()
        .editable(false)
        .cursor_visible(false)
        .monospace(true)
        .wrap_mode(gtk::WrapMode::None)
        .hexpand(true)
        .vexpand(true)
        .halign(gtk::Align::Fill)
        .valign(gtk::Align::Fill)
        .build();
    inert(&view);
    let buffer = view.buffer();
    let mut iter = buffer.start_iter();
    for (index, line) in lines.iter().enumerate() {
        if index > 0 {
            buffer.insert(&mut iter, "\n");
        }
        for span in &line.spans {
            let tag = span_tag(span);
            buffer.tag_table().add(&tag);
            buffer.insert_with_tags(&mut iter, &span.text, &[&tag]);
        }
    }
    view
}

fn place_card(board: &CardBoard, slot: &mut Slot, card_slot: CardSlot) {
    slot.geom = card_slot;
    board.set_slot(&slot.card, card_slot);
}

fn content_fingerprint(node: &super::tree::Node) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    node_title(node).hash(&mut hasher);
    node_scroll(node).hash(&mut hasher);
    node_thumb(node).hash(&mut hasher);
    node_body(node).hash(&mut hasher);
    hasher.finish()
}

fn card_signature(node: &super::tree::Node, row_h: i32) -> String {
    let body = match node_body(node) {
        PaneBody::Empty => "empty",
        PaneBody::Rows(_) => "rows",
        PaneBody::Text(_) => "text",
    };
    let header = match (node_title(node).is_some(), count_text(node).is_some()) {
        (true, true) => "title-count",
        (true, false) => "title",
        (false, true) => "count",
        (false, false) => "none",
    };
    format!("{header}|{body}|{row_h}|{:016x}", content_fingerprint(node))
}

fn count_text(node: &super::tree::Node) -> Option<String> {
    let (current, total) = node_scroll(node)?;
    Some(format!("{current} / {total}"))
}

fn overflow_hinted(node: &super::tree::Node) -> bool {
    if node_thumb(node).is_some() {
        return true;
    }
    node_scroll(node).is_some_and(|(_, total)| total > viewport_rows(node))
}

fn viewport_rows(node: &super::tree::Node) -> u32 {
    let (_, _, _, rows, _) = node_rect(node);
    u32::from(rows.saturating_sub(2).max(1))
}

fn fade_height(row_h: i32) -> i32 {
    (row_h.max(1) * 2 / 5).saturating_add(16).max(20)
}

fn apply_overflow(slot: &Slot, node: &super::tree::Node, row_h: i32) {
    match node_title(node) {
        Some(title) => {
            slot.title.set_text(title);
            slot.title.set_visible(true);
        }
        None => slot.title.set_visible(false),
    }
    match count_text(node) {
        Some(text) => {
            slot.count.set_text(&text);
            slot.count.set_visible(true);
        }
        None => slot.count.set_visible(false),
    }
    slot.header
        .set_visible(slot.title.is_visible() || slot.count.is_visible());
    let more = overflow_hinted(node);
    if more {
        slot.card.add_css_class("herdr-more");
    } else {
        slot.card.remove_css_class("herdr-more");
    }
    slot.fade.set_size_request(-1, fade_height(row_h));
    slot.fade.set_visible(more);
}

fn line_label(line: &ContentLine, honor_reverse: bool) -> gtk::Label {
    let mut text = String::new();
    let attrs = pango::AttrList::new();
    for span in &line.spans {
        let start = text.len() as u32;
        text.push_str(&span.text);
        let end = text.len() as u32;
        if start == end {
            continue;
        }
        let mut style = span.style;
        if !honor_reverse {
            style.reverse = false;
        }
        let (fg, bg) = paint_colors(span.fg, span.bg, style);
        insert_attr(
            &attrs,
            pango::AttrColor::new_foreground(channel(fg.r), channel(fg.g), channel(fg.b)).upcast(),
            start,
            end,
        );
        if bg.is_visible() {
            insert_attr(
                &attrs,
                pango::AttrColor::new_background(channel(bg.r), channel(bg.g), channel(bg.b))
                    .upcast(),
                start,
                end,
            );
        }
        if span.style.bold {
            insert_attr(
                &attrs,
                pango::AttrInt::new_weight(pango::Weight::Bold).upcast(),
                start,
                end,
            );
        }
        if span.style.italic {
            insert_attr(
                &attrs,
                pango::AttrInt::new_style(pango::Style::Italic).upcast(),
                start,
                end,
            );
        }
        if span.style.underline {
            insert_attr(
                &attrs,
                pango::AttrInt::new_underline(pango::Underline::Single).upcast(),
                start,
                end,
            );
        }
    }
    let mut font = pango::FontDescription::new();
    font.set_family("monospace");
    insert_attr(
        &attrs,
        pango::AttrFontDesc::new(&font).upcast(),
        0,
        text.len() as u32,
    );
    let label = gtk::Label::new(Some(&text));
    label.set_attributes(Some(&attrs));
    label.set_xalign(0.0);
    label.set_halign(gtk::Align::Start);
    label
}

fn span_tag(span: &Span) -> gtk::TextTag {
    let (fg, bg) = paint_colors(span.fg, span.bg, span.style);
    let tag = gtk::TextTag::builder()
        .foreground_rgba(&gdk_rgba(fg))
        .weight(if span.style.bold { 700 } else { 400 })
        .style(if span.style.italic {
            pango::Style::Italic
        } else {
            pango::Style::Normal
        })
        .underline(if span.style.underline {
            pango::Underline::Single
        } else {
            pango::Underline::None
        })
        .build();
    if bg.is_visible() {
        tag.set_background_rgba(Some(&gdk_rgba(bg)));
    }
    tag
}

fn insert_attr(list: &pango::AttrList, mut attr: pango::Attribute, start: u32, end: u32) {
    attr.set_start_index(start);
    attr.set_end_index(end);
    list.insert(attr);
}

fn channel(value: u8) -> u16 {
    u16::from(value) * 257
}

fn gdk_rgba(color: Rgba) -> gdk::RGBA {
    gdk::RGBA::new(
        f32::from(color.r) / 255.0,
        f32::from(color.g) / 255.0,
        f32::from(color.b) / 255.0,
        f32::from(color.a) / 255.0,
    )
}

fn inert(widget: &impl IsA<gtk::Widget>) {
    let widget = widget.upcast_ref::<gtk::Widget>();
    widget.set_can_target(false);
    widget.set_can_focus(false);
    widget.set_focusable(false);
}

mod board {
    use std::cell::RefCell;
    use std::collections::HashMap;

    use gtk::prelude::*;
    use gtk::subclass::prelude::*;
    use gtk::{glib, graphene};

    use super::super::cards::CardSlot;

    pub struct CardBoardImp;

    impl Default for CardBoardImp {
        fn default() -> Self {
            Self
        }
    }

    #[glib::object_subclass]
    impl ObjectSubclass for CardBoardImp {
        const NAME: &'static str = "HerdrCardBoard";
        type Type = CardBoard;
        type ParentType = gtk::Widget;

        fn class_init(klass: &mut Self::Class) {
            klass.set_layout_manager_type::<CardLayout>();
            klass.set_css_name("card-board");
        }
    }

    impl ObjectImpl for CardBoardImp {
        fn dispose(&self) {
            let obj = self.obj();
            while let Some(child) = obj.first_child() {
                child.unparent();
            }
        }
    }

    impl WidgetImpl for CardBoardImp {
        fn contains(&self, _x: f64, _y: f64) -> bool {
            false
        }
    }

    glib::wrapper! {
        pub struct CardBoard(ObjectSubclass<CardBoardImp>)
            @extends gtk::Widget,
            @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
    }

    impl CardBoard {
        pub(crate) fn new() -> Self {
            glib::Object::new()
        }

        pub(crate) fn set_slot(&self, child: &impl IsA<gtk::Widget>, slot: CardSlot) {
            let child = child.clone().upcast::<gtk::Widget>();
            let board = self.upcast_ref::<gtk::Widget>();
            match child.parent() {
                Some(parent) if &parent == board => {}
                Some(_) => {
                    child.unparent();
                    child.set_parent(self);
                }
                None => child.set_parent(self),
            }
            if self.manager().set_slot(&child, slot) {
                self.queue_allocate();
            }
        }

        pub(crate) fn remove_child(&self, child: &impl IsA<gtk::Widget>) {
            let child = child.clone().upcast::<gtk::Widget>();
            self.manager().clear_slot(&child);
            if child
                .parent()
                .is_some_and(|parent| &parent == self.upcast_ref::<gtk::Widget>())
            {
                child.unparent();
            }
            self.queue_allocate();
        }

        fn manager(&self) -> CardLayout {
            self.layout_manager()
                .and_downcast::<CardLayout>()
                .expect("card board uses card layout")
        }
    }

    pub struct CardLayoutImp {
        slots: RefCell<HashMap<usize, CardSlot>>,
    }

    impl Default for CardLayoutImp {
        fn default() -> Self {
            Self {
                slots: RefCell::new(HashMap::new()),
            }
        }
    }

    #[glib::object_subclass]
    impl ObjectSubclass for CardLayoutImp {
        const NAME: &'static str = "HerdrCardLayout";
        type Type = CardLayout;
        type ParentType = gtk::LayoutManager;
    }

    impl ObjectImpl for CardLayoutImp {}

    impl LayoutManagerImpl for CardLayoutImp {
        fn measure(
            &self,
            _widget: &gtk::Widget,
            orientation: gtk::Orientation,
            _for_size: i32,
        ) -> (i32, i32, i32, i32) {
            let extent = self
                .slots
                .borrow()
                .values()
                .map(|slot| match orientation {
                    gtk::Orientation::Horizontal => (slot.x + f64::from(slot.width)).ceil() as i32,
                    _ => (slot.y + f64::from(slot.height)).ceil() as i32,
                })
                .max()
                .unwrap_or(0);
            (extent, extent, -1, -1)
        }

        fn allocate(&self, widget: &gtk::Widget, _width: i32, _height: i32, _baseline: i32) {
            let slots = self.slots.borrow();
            let mut child = widget.first_child();
            while let Some(current) = child {
                let key = current.as_ptr() as usize;
                if let Some(slot) = slots.get(&key) {
                    let transform = gtk::gsk::Transform::new()
                        .translate(&graphene::Point::new(slot.x as f32, slot.y as f32));
                    current.allocate(slot.width.max(1), slot.height.max(1), -1, Some(transform));
                }
                child = current.next_sibling();
            }
        }
    }

    glib::wrapper! {
        pub struct CardLayout(ObjectSubclass<CardLayoutImp>)
            @extends gtk::LayoutManager;
    }

    impl CardLayout {
        fn set_slot(&self, child: &gtk::Widget, slot: CardSlot) -> bool {
            let key = child.as_ptr() as usize;
            let mut slots = self.imp().slots.borrow_mut();
            if slots.get(&key) == Some(&slot) {
                return false;
            }
            slots.insert(key, slot);
            true
        }

        fn clear_slot(&self, child: &gtk::Widget) {
            self.imp()
                .slots
                .borrow_mut()
                .remove(&(child.as_ptr() as usize));
        }
    }
}

use board::CardBoard;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::overlay::test_grid::test_grid;
    use crate::render::overlay::tree::{parse, Node};

    fn stamp(cell: &mut crate::terminal::Cell, x0: u16, y0: u16, x1: u16, y1: u16) {
        let x = cell.x;
        let y = cell.y;
        if x < x0 || x > x1 || y < y0 || y > y1 {
            return;
        }
        let glyph = if x == x0 && y == y0 {
            "┌"
        } else if x == x1 && y == y0 {
            "┐"
        } else if x == x0 && y == y1 {
            "└"
        } else if x == x1 && y == y1 {
            "┘"
        } else if y == y0 || y == y1 {
            "─"
        } else if x == x0 || x == x1 {
            "│"
        } else {
            return;
        };
        cell.grapheme = glyph.to_string();
    }

    fn write_at(cell: &mut crate::terminal::Cell, x: u16, y: u16, text: &str) {
        if cell.y != y || cell.x < x {
            return;
        }
        if let Some(ch) = text.chars().nth((cell.x - x) as usize) {
            cell.grapheme = ch.to_string();
        }
    }

    fn frame_ids(nodes: &[Node]) -> Vec<u64> {
        let mut frames = Vec::new();
        collect_frames(nodes, &mut frames);
        frames.into_iter().map(|(id, _)| id).collect()
    }

    #[test]
    fn an_empty_leaf_pane_still_gets_a_card_slot() {
        let snap = test_grid(24, 8, |cell| {
            stamp(cell, 0, 0, 23, 7);
            write_at(cell, 2, 0, "Files");
        });
        let tree = parse(&snap);
        let node = &tree.layout()[0];
        assert!(matches!(node_body(node), PaneBody::Empty), "{tree}");
        assert!(node_children(node).is_empty(), "{tree}");
        assert_eq!(frame_ids(tree.layout()), vec![node_id(node)], "{tree}");
        assert_eq!(faces(tree.layout()).len(), 1, "{tree}");
    }

    #[test]
    fn an_empty_parent_does_not_cover_its_child() {
        let snap = test_grid(30, 16, |cell| {
            stamp(cell, 0, 0, 29, 15);
            stamp(cell, 1, 1, 28, 14);
            write_at(cell, 3, 3, "INNER");
        });
        let tree = parse(&snap);
        let parent = &tree.layout()[0];
        let child = &node_children(parent)[0];
        assert!(matches!(node_body(parent), PaneBody::Empty), "{tree}");
        assert!(!node_children(parent).is_empty(), "{tree}");
        assert_eq!(frame_ids(tree.layout()), vec![node_id(child)], "{tree}");
    }

    #[test]
    fn n_of_m_is_a_count_chip_not_a_scroll_model() {
        let snap = test_grid(40, 10, |cell| {
            stamp(cell, 0, 0, 39, 9);
            write_at(cell, 2, 0, "Files");
            write_at(cell, 30, 9, "9 of 24");
        });
        let tree = parse(&snap);
        let node = &tree.layout()[0];
        assert_eq!(count_text(node).as_deref(), Some("9 / 24"));
        assert!(overflow_hinted(node));
    }

    #[test]
    fn a_tui_thumb_hints_overflow_without_n_of_m() {
        let snap = test_grid(20, 12, |cell| {
            stamp(cell, 0, 0, 19, 11);
            write_at(cell, 2, 0, "Commits");
            if cell.x == 19 && (4..=7).contains(&cell.y) {
                cell.grapheme = "▐".to_string();
            }
        });
        let tree = parse(&snap);
        let node = &tree.layout()[0];
        assert_eq!(count_text(node), None);
        assert!(overflow_hinted(node));
    }

    #[test]
    fn a_fitting_pane_has_no_overflow_hint() {
        let snap = test_grid(20, 8, |cell| {
            stamp(cell, 0, 0, 19, 7);
            write_at(cell, 2, 0, "Files");
            write_at(cell, 2, 2, "main.rs");
        });
        let tree = parse(&snap);
        let node = &tree.layout()[0];
        assert_eq!(count_text(node), None);
        assert!(!overflow_hinted(node));
    }

    #[test]
    fn fade_covers_the_last_row_fraction() {
        assert!(fade_height(20) >= 20 * 2 / 5);
        assert!(fade_height(20) > 20 * 2 / 5);
    }

    #[test]
    fn card_signature_changes_when_row_text_changes() {
        let first = test_grid(20, 8, |cell| {
            stamp(cell, 0, 0, 19, 7);
            write_at(cell, 2, 0, "Files");
            write_at(cell, 2, 2, "aaa");
        });
        let second = test_grid(20, 8, |cell| {
            stamp(cell, 0, 0, 19, 7);
            write_at(cell, 2, 0, "Files");
            write_at(cell, 2, 2, "bbb");
        });
        let first_tree = parse(&first);
        let second_tree = parse(&second);
        let a = &first_tree.layout()[0];
        let b = &second_tree.layout()[0];
        assert_ne!(card_signature(a, 16), card_signature(b, 16));
        assert_ne!(node_body(a), node_body(b));
    }

    #[test]
    fn card_signature_stays_when_body_is_unchanged() {
        let snap = test_grid(20, 8, |cell| {
            stamp(cell, 0, 0, 19, 7);
            write_at(cell, 2, 0, "Files");
            write_at(cell, 2, 2, "aaa");
        });
        let tree = parse(&snap);
        let node = &tree.layout()[0];
        assert_eq!(card_signature(node, 16), card_signature(node, 16));
    }
}
