//! Focusable terminal widget. One custom GTK widget paints the whole grid.

use crate::terminal::TerminalSnapshot;

use super::cell_renderer::device_cols_rows;

/// Default logical cell size until Pango metrics are measured.
const DEFAULT_CELL_WIDTH: f64 = 8.0;
const DEFAULT_CELL_HEIGHT: f64 = 16.0;
const DEFAULT_SCALE: f64 = 1.0;

/// Origin of a snapshot that fits the card, or `None` when it does not.
///
/// A smaller grid is centered at the measured cell size. Leftover pixels stay
/// empty; glyphs are not scaled to fill them. A larger grid is refused so the
/// painter does not clip it down to a corner.
fn letterbox_origin(
    view_w: f64,
    view_h: f64,
    cell_w: f64,
    cell_h: f64,
    snap_cols: u16,
    snap_rows: u16,
    cap_cols: u16,
    cap_rows: u16,
) -> Option<(f64, f64)> {
    if snap_cols > cap_cols || snap_rows > cap_rows {
        return None;
    }
    if !view_w.is_finite()
        || view_w <= 0.0
        || !view_h.is_finite()
        || view_h <= 0.0
        || !cell_w.is_finite()
        || cell_w <= 0.0
        || !cell_h.is_finite()
        || cell_h <= 0.0
    {
        return None;
    }
    let grid_w = f64::from(snap_cols) * cell_w;
    let grid_h = f64::from(snap_rows) * cell_h;
    Some(((view_w - grid_w) / 2.0, (view_h - grid_h) / 2.0))
}

/// 0-based cell under a widget-local pointer.
///
/// The origin is [`letterbox_origin`], the same one the painter uses. A point
/// in the empty margin clamps to the nearest cell. `None` when the snapshot
/// does not fit the card, so the caller does not invent a cell.
pub(crate) fn cell_under_pointer(
    local_x: f64,
    local_y: f64,
    view_w: f64,
    view_h: f64,
    cell_w: f64,
    cell_h: f64,
    cols: u16,
    rows: u16,
    cap_cols: u16,
    cap_rows: u16,
) -> Option<(u16, u16)> {
    if cols == 0 || rows == 0 || !local_x.is_finite() || !local_y.is_finite() {
        return None;
    }
    let (origin_x, origin_y) = letterbox_origin(
        view_w, view_h, cell_w, cell_h, cols, rows, cap_cols, cap_rows,
    )?;
    let col = ((local_x - origin_x) / cell_w).floor();
    let row = ((local_y - origin_y) / cell_h).floor();
    Some((
        col.clamp(0.0, f64::from(cols - 1)) as u16,
        row.clamp(0.0, f64::from(rows - 1)) as u16,
    ))
}

#[cfg(feature = "gtk")]
mod gtk_view {
    use super::*;
    use crate::terminal::Cursor;
    use std::cell::{Cell, RefCell};

    use std::time::Duration;

    use gtk::prelude::*;
    use gtk::subclass::prelude::*;
    use gtk::{gdk, glib, graphene, pango};

    use super::super::cell_renderer::{
        box_cell_strokes, cell_band, dominant_opaque_bg, group_background_runs, group_runs,
        paint_colors, snap_cell_px, snap_rect, Rgba, Run,
    };
    use super::super::overlay::{enhanced_chrome, PaneGui, SCRIM_ALPHA};
    const FONT_SIZE_PT: i32 = 12;

    pub struct TerminalViewImp {
        snapshot: RefCell<Option<TerminalSnapshot>>,
        enhanced: Cell<bool>,
        cell_width: Cell<f64>,
        cell_height: Cell<f64>,
        scale: Cell<f64>,
        font_desc: RefCell<Option<pango::FontDescription>>,
        metrics_ready: Cell<bool>,
        alloc_w: Cell<i32>,
        alloc_h: Cell<i32>,
        resize_gen: Cell<u64>,
        last_grid: Cell<(u16, u16, u32, u32)>,
        grid_cb: RefCell<Option<Box<dyn Fn(u16, u16, u32, u32)>>>,
        pointer_x: Cell<f64>,
        pointer_y: Cell<f64>,
        tree: RefCell<Option<super::super::LayoutTree>>,
        gui: RefCell<PaneGui>,
    }

    impl Default for TerminalViewImp {
        fn default() -> Self {
            Self {
                snapshot: RefCell::new(None),
                enhanced: Cell::new(false),
                cell_width: Cell::new(DEFAULT_CELL_WIDTH),
                cell_height: Cell::new(DEFAULT_CELL_HEIGHT),
                scale: Cell::new(DEFAULT_SCALE),
                font_desc: RefCell::new(None),
                metrics_ready: Cell::new(false),
                alloc_w: Cell::new(0),
                alloc_h: Cell::new(0),
                resize_gen: Cell::new(0),
                last_grid: Cell::new((0, 0, 0, 0)),
                grid_cb: RefCell::new(None),
                pointer_x: Cell::new(f64::NAN),
                pointer_y: Cell::new(f64::NAN),
                tree: RefCell::new(None),
                gui: RefCell::new(PaneGui::new()),
            }
        }
    }

    #[glib::object_subclass]
    impl ObjectSubclass for TerminalViewImp {
        const NAME: &'static str = "HerdrTerminalView";
        type Type = TerminalView;
        type ParentType = gtk::Widget;

        fn class_init(klass: &mut Self::Class) {
            klass.set_css_name("terminal-view");
        }
    }

    impl ObjectImpl for TerminalViewImp {
        fn constructed(&self) {
            self.parent_constructed();
            let obj = self.obj();
            obj.set_focusable(true);
            obj.set_can_focus(true);
            obj.set_focus_on_click(true);
            obj.set_overflow(gtk::Overflow::Hidden);
        }

        fn dispose(&self) {
            self.gui.borrow_mut().detach();
            self.tree.borrow_mut().take();
        }
    }

    impl WidgetImpl for TerminalViewImp {
        fn measure(&self, orientation: gtk::Orientation, _for_size: i32) -> (i32, i32, i32, i32) {
            self.ensure_font_metrics();
            let cell = match orientation {
                gtk::Orientation::Horizontal => self.snapped_cell_width(),
                _ => self.snapped_cell_height(),
            };
            let size = cell.ceil().max(1.0) as i32;
            (size, size, -1, -1)
        }

        fn size_allocate(&self, width: i32, height: i32, _baseline: i32) {
            let obj = self.obj();
            let scale = obj.scale_factor() as f64;
            if scale.is_finite() && scale > 0.0 {
                self.scale.set(scale);
            }
            self.alloc_w.set(width);
            self.alloc_h.set(height);
            self.ensure_font_metrics();
            let gen = self.resize_gen.get().wrapping_add(1);
            self.resize_gen.set(gen);
            if self.enhanced.get() {
                if let Some((origin_x, origin_y, cell_w, cell_h, cols, rows)) =
                    self.grid_metrics(width, height)
                {
                    self.gui
                        .borrow_mut()
                        .place(origin_x, origin_y, cols, rows, cell_w, cell_h);
                }
            }
            let obj = self.obj().clone();
            glib::timeout_add_local_once(Duration::from_millis(80), move || {
                if obj.imp().resize_gen.get() != gen {
                    return;
                }
                obj.imp().emit_grid_capacity();
            });
        }

        fn snapshot(&self, snapshot: &gtk::Snapshot) {
            self.ensure_font_metrics();
            let obj = self.obj();
            let width = obj.width() as f32;
            let height = obj.height() as f32;
            if width <= 0.0 || height <= 0.0 {
                self.gui.borrow_mut().detach();
                return;
            }

            let bounds = graphene::Rect::new(0.0, 0.0, width, height);
            snapshot.push_clip(&bounds);

            let terminal = self.snapshot.borrow();
            let Some(terminal) = terminal.as_ref() else {
                self.gui.borrow_mut().detach();
                snapshot.pop();
                return;
            };

            let scale = self.scale.get();
            let cell_w = self.snapped_cell_width();
            let cell_h = self.snapped_cell_height();
            let (cap_cols, cap_rows) =
                device_cols_rows(f64::from(width), f64::from(height), cell_w, cell_h, scale);
            let Some((origin_x, origin_y)) = letterbox_origin(
                f64::from(width),
                f64::from(height),
                cell_w,
                cell_h,
                terminal.cols(),
                terminal.rows(),
                cap_cols,
                cap_rows,
            ) else {
                self.gui.borrow_mut().detach();
                snapshot.pop();
                return;
            };
            let backdrop = if self.enhanced.get() {
                Some(enhanced_chrome().0)
            } else {
                dominant_opaque_bg(terminal)
            };
            if let Some(bg) = backdrop {
                Self::append_snapped(
                    snapshot,
                    &rgba_to_gdk(bg),
                    0.0,
                    0.0,
                    f64::from(width),
                    f64::from(height),
                    scale,
                );
            }

            let tree = if self.enhanced.get() {
                Some(super::super::reconcile(
                    terminal,
                    &mut self.tree.borrow_mut(),
                ))
            } else {
                self.tree.borrow_mut().take();
                None
            };
            if tree.is_none() {
                self.gui.borrow_mut().detach();
            }

            let font_desc = self.font_desc.borrow();
            snapshot.save();
            snapshot.translate(&graphene::Point::new(origin_x as f32, origin_y as f32));

            for run in group_background_runs(terminal) {
                let mut start = 0usize;
                while start < run.cells.len() {
                    while start < run.cells.len()
                        && tree.as_ref().is_some_and(|tree| {
                            tree.suppress()
                                .contains(&(run.cells[start].x, run.cells[start].y))
                        })
                    {
                        start += 1;
                    }
                    let mut end = start;
                    while end < run.cells.len()
                        && !tree.as_ref().is_some_and(|tree| {
                            tree.suppress()
                                .contains(&(run.cells[end].x, run.cells[end].y))
                        })
                    {
                        end += 1;
                    }
                    let Some(first) = run.cells.get(start) else {
                        break;
                    };
                    let (_, bg) = paint_colors(first.fg, first.bg, first.style);
                    if bg.is_visible() {
                        let (x, y, w, h) =
                            cell_band(first.x, first.y, (end - start) as u16, cell_w, cell_h);
                        Self::append_snapped(snapshot, &rgba_to_gdk(bg), x, y, w, h, scale);
                    }
                    start = end;
                }
            }

            for run in group_runs(terminal) {
                self.paint_text_run(
                    snapshot,
                    &*obj,
                    &run,
                    cell_w,
                    cell_h,
                    scale,
                    font_desc.as_ref(),
                    tree.as_ref().map(|tree| tree.suppress()),
                );
            }

            if !tree.as_ref().is_some_and(|tree| {
                let cursor = terminal.cursor();
                let x = if cursor.wide_tail {
                    cursor.x.saturating_sub(1)
                } else {
                    cursor.x
                };
                cursor.visible && tree.suppress().contains(&(x, cursor.y))
            }) {
                self.paint_cursor(snapshot, terminal.cursor(), cell_w, cell_h, scale);
            }
            snapshot.restore();

            if let Some(tree) = tree.as_ref() {
                let mut gui = self.gui.borrow_mut();
                gui.attach(obj.upcast_ref());
                gui.sync(tree, cell_w, cell_h);
                gui.place(
                    origin_x,
                    origin_y,
                    terminal.cols(),
                    terminal.rows(),
                    cell_w,
                    cell_h,
                );
                let dialogs_open = gui.dialogs_open();
                let layout = gui.layout_widget();
                let dialogs = gui.dialogs_widget();
                drop(gui);
                obj.snapshot_child(&layout, snapshot);
                if dialogs_open {
                    snapshot.append_color(&gdk::RGBA::new(0.0, 0.0, 0.0, SCRIM_ALPHA), &bounds);
                }
                obj.snapshot_child(&dialogs, snapshot);
            }
            snapshot.pop();
        }
    }

    impl TerminalViewImp {
        fn ensure_font_metrics(&self) {
            if self.metrics_ready.get() {
                return;
            }
            let obj = self.obj();
            let context = obj.create_pango_context();

            let mut desc = pango::FontDescription::new();
            desc.set_family("JetBrains Mono");
            desc.set_size(FONT_SIZE_PT * pango::SCALE);

            if !font_usable(&context, &desc) {
                desc.set_family("monospace");
            }
            if !font_usable(&context, &desc) {
                // Reject non-positive advances; keep defaults until a usable font appears.
                return;
            }

            let width = glyph_width(&context, &desc, "M");
            let metrics = context.metrics(Some(&desc), None);
            let height = metrics.height() as f64 / f64::from(pango::SCALE);
            if width <= 0.0 || !width.is_finite() || height <= 0.0 || !height.is_finite() {
                return;
            }

            self.cell_width.set(width);
            self.cell_height.set(height);
            self.font_desc.replace(Some(desc));
            self.metrics_ready.set(true);
            self.obj().queue_resize();
        }

        fn emit_grid_capacity(&self) {
            let width = self.alloc_w.get();
            let height = self.alloc_h.get();
            if width <= 0 || height <= 0 {
                return;
            }
            self.ensure_font_metrics();
            if !self.metrics_ready.get() {
                return;
            }
            let scale = self.scale.get();
            let cell_w = self.snapped_cell_width();
            let cell_h = self.snapped_cell_height();
            let (cols, rows) =
                device_cols_rows(f64::from(width), f64::from(height), cell_w, cell_h, scale);
            let cell_w = (cell_w * scale).round().max(1.0) as u32;
            let cell_h = (cell_h * scale).round().max(1.0) as u32;
            let next = (cols, rows, cell_w, cell_h);
            if next == self.last_grid.get() {
                return;
            }
            self.last_grid.set(next);
            if let Some(cb) = self.grid_cb.borrow().as_ref() {
                cb(cols, rows, cell_w, cell_h);
            }
        }

        fn snapped_cell_width(&self) -> f64 {
            snap_cell_px(self.cell_width.get(), self.scale.get())
        }

        fn snapped_cell_height(&self) -> f64 {
            snap_cell_px(self.cell_height.get(), self.scale.get())
        }

        fn append_snapped(
            snapshot: &gtk::Snapshot,
            color: &gdk::RGBA,
            x: f64,
            y: f64,
            w: f64,
            h: f64,
            scale: f64,
        ) {
            let (x, y, w, h) = snap_rect(x, y, w, h, scale);
            snapshot.append_color(
                color,
                &graphene::Rect::new(x as f32, y as f32, w as f32, h as f32),
            );
        }

        fn paint_text_run(
            &self,
            snapshot: &gtk::Snapshot,
            obj: &TerminalView,
            run: &Run<'_>,
            cell_w: f64,
            cell_h: f64,
            scale: f64,
            font_desc: Option<&pango::FontDescription>,
            suppress: Option<&std::collections::HashSet<(u16, u16)>>,
        ) {
            let partial = suppress.is_some_and(|set| {
                run.cells.iter().any(|cell| {
                    cell.occupancy != crate::terminal::CellOccupancy::WideContinuation
                        && set.contains(&(cell.x, cell.y))
                })
            });
            if partial {
                for index in 0..run.cells.len() {
                    let cell = &run.cells[index];
                    if cell.occupancy == crate::terminal::CellOccupancy::WideContinuation {
                        continue;
                    }
                    if suppress.is_some_and(|set| set.contains(&(cell.x, cell.y))) {
                        continue;
                    }
                    let one = Run {
                        cells: &run.cells[index..index + 1],
                    };
                    self.paint_text_run(
                        snapshot, obj, &one, cell_w, cell_h, scale, font_desc, None,
                    );
                }
                return;
            }

            let Some(first) = run.cells.first() else {
                return;
            };
            let (fg, _) = paint_colors(first.fg, first.bg, first.style);
            for cell in run.cells {
                if cell.occupancy == crate::terminal::CellOccupancy::WideContinuation
                    || suppress.is_some_and(|set| set.contains(&(cell.x, cell.y)))
                    || cell.grapheme.is_empty()
                {
                    continue;
                }
                let origin_x = f64::from(cell.x) * cell_w;
                let origin_y = f64::from(cell.y) * cell_h;
                if let Some(strokes) = box_cell_strokes(&cell.grapheme, cell_w, cell_h, scale) {
                    let (stroke_fg, _) = paint_colors(cell.fg, cell.bg, cell.style);
                    let color = rgba_to_gdk(stroke_fg);
                    for stroke in strokes {
                        Self::append_snapped(
                            snapshot,
                            &color,
                            origin_x + stroke.x,
                            origin_y + stroke.y,
                            stroke.width,
                            stroke.height,
                            scale,
                        );
                    }
                    continue;
                }
                let layout = obj.create_pango_layout(Some(&cell.grapheme));
                if let Some(desc) = font_desc {
                    let mut desc = desc.clone();
                    if first.style.bold {
                        desc.set_weight(pango::Weight::Bold);
                    }
                    if first.style.italic {
                        desc.set_style(pango::Style::Italic);
                    }
                    layout.set_font_description(Some(&desc));
                }
                snapshot.save();
                snapshot.translate(&graphene::Point::new(origin_x as f32, origin_y as f32));
                snapshot.append_layout(&layout, &rgba_to_gdk(fg));
                snapshot.restore();
            }
            if first.style.underline {
                let cols: f64 = run
                    .cells
                    .iter()
                    .map(|cell| match cell.occupancy {
                        crate::terminal::CellOccupancy::WideLeading => 2.0,
                        crate::terminal::CellOccupancy::WideContinuation => 0.0,
                        crate::terminal::CellOccupancy::Single => 1.0,
                    })
                    .sum();
                let hairline = 1.0 / scale.max(1.0);
                Self::append_snapped(
                    snapshot,
                    &rgba_to_gdk(fg),
                    f64::from(first.x) * cell_w,
                    (f64::from(first.y) + 1.0) * cell_h - hairline,
                    cell_w * cols.max(1.0),
                    hairline,
                    scale,
                );
            }
        }

        fn grid_metrics(&self, width: i32, height: i32) -> Option<(f64, f64, f64, f64, u16, u16)> {
            let terminal = self.snapshot.borrow();
            let terminal = terminal.as_ref()?;
            let width = f64::from(width);
            let height = f64::from(height);
            let scale = self.scale.get();
            let cell_w = self.snapped_cell_width();
            let cell_h = self.snapped_cell_height();
            let (cap_cols, cap_rows) = device_cols_rows(width, height, cell_w, cell_h, scale);
            let (origin_x, origin_y) = letterbox_origin(
                width,
                height,
                cell_w,
                cell_h,
                terminal.cols(),
                terminal.rows(),
                cap_cols,
                cap_rows,
            )?;
            Some((
                origin_x,
                origin_y,
                cell_w,
                cell_h,
                terminal.cols(),
                terminal.rows(),
            ))
        }

        fn paint_cursor(
            &self,
            snapshot: &gtk::Snapshot,
            cursor: Cursor,
            cell_w: f64,
            cell_h: f64,
            scale: f64,
        ) {
            if !cursor.visible {
                return;
            }
            let mut x = cursor.x;
            if cursor.wide_tail {
                x = x.saturating_sub(1);
            }
            let color = gdk::RGBA::new(DEFAULT_FG_F.0, DEFAULT_FG_F.1, DEFAULT_FG_F.2, 0.35);
            Self::append_snapped(
                snapshot,
                &color,
                f64::from(x) * cell_w,
                f64::from(cursor.y) * cell_h,
                cell_w,
                cell_h,
                scale,
            );
        }
    }

    const DEFAULT_FG_F: (f32, f32, f32) = (
        0xe8 as f32 / 255.0,
        0xe8 as f32 / 255.0,
        0xea as f32 / 255.0,
    );

    fn font_usable(context: &pango::Context, desc: &pango::FontDescription) -> bool {
        if context.load_font(desc).is_none() {
            return false;
        }
        let width = glyph_width(context, desc, "M");
        width.is_finite()
            && width > 0.0
            && ["i", "0", "─", "│"]
                .into_iter()
                .all(|glyph| (glyph_width(context, desc, glyph) - width).abs() < 0.01)
    }

    fn glyph_width(context: &pango::Context, desc: &pango::FontDescription, text: &str) -> f64 {
        let layout = pango::Layout::new(context);
        layout.set_font_description(Some(desc));
        layout.set_text(text);
        f64::from(layout.size().0) / f64::from(pango::SCALE)
    }

    fn rgba_to_gdk(c: Rgba) -> gdk::RGBA {
        gdk::RGBA::new(
            f32::from(c.r) / 255.0,
            f32::from(c.g) / 255.0,
            f32::from(c.b) / 255.0,
            f32::from(c.a) / 255.0,
        )
    }

    glib::wrapper! {
        pub struct TerminalView(ObjectSubclass<TerminalViewImp>)
            @extends gtk::Widget,
            @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
    }

    impl TerminalView {
        pub fn new() -> Self {
            glib::Object::new()
        }

        pub fn set_snapshot(&self, snapshot: TerminalSnapshot) {
            self.imp().snapshot.replace(Some(snapshot));
            self.queue_draw();
        }

        pub fn snapshot_model(&self) -> Option<TerminalSnapshot> {
            self.imp().snapshot.borrow().clone()
        }

        pub fn set_enhanced(&self, enhanced: bool) {
            let imp = self.imp();
            imp.enhanced.set(enhanced);
            if !enhanced {
                imp.tree.borrow_mut().take();
                imp.gui.borrow_mut().detach();
            }
            // Toggle only flips local state; it must not reconnect Herdr/Ghostty.
            self.queue_draw();
        }

        pub fn enhanced(&self) -> bool {
            self.imp().enhanced.get()
        }

        pub fn cell_width(&self) -> f64 {
            self.imp().ensure_font_metrics();
            self.imp().snapped_cell_width()
        }

        pub fn cell_height(&self) -> f64 {
            self.imp().ensure_font_metrics();
            self.imp().snapped_cell_height()
        }

        pub fn scale(&self) -> f64 {
            self.imp().scale.get()
        }

        pub fn set_scale(&self, scale: f64) {
            if scale.is_finite() && scale > 0.0 {
                self.imp().scale.set(scale);
            }
        }

        pub fn cols_rows(&self, width_px: f64, height_px: f64) -> (u16, u16) {
            device_cols_rows(
                width_px,
                height_px,
                self.cell_width(),
                self.cell_height(),
                self.scale(),
            )
        }

        /// Widget-local pointer from motion. Non-finite values are ignored.
        pub fn set_pointer(&self, x: f64, y: f64) {
            if x.is_finite() && y.is_finite() {
                self.imp().pointer_x.set(x);
                self.imp().pointer_y.set(y);
            }
        }

        /// Last widget-local pointer, if motion or enter has reported one.
        pub fn pointer(&self) -> Option<(f64, f64)> {
            let x = self.imp().pointer_x.get();
            let y = self.imp().pointer_y.get();
            if x.is_finite() && y.is_finite() {
                Some((x, y))
            } else {
                None
            }
        }

        /// Debounced grid capacity from the latest allocation. Main thread only.
        pub fn set_on_grid_capacity<F>(&self, f: F)
        where
            F: Fn(u16, u16, u32, u32) + 'static,
        {
            self.imp().grid_cb.replace(Some(Box::new(f)));
        }
    }

    impl Default for TerminalView {
        fn default() -> Self {
            Self::new()
        }
    }
}

#[cfg(feature = "gtk")]
pub use gtk_view::TerminalView;

/// Headless twin of the GTK widget so the crate builds without gtk devel packages.
#[cfg(not(feature = "gtk"))]
#[derive(Debug)]
pub struct TerminalView {
    snapshot: Option<TerminalSnapshot>,
    enhanced: bool,
    cell_width: f64,
    cell_height: f64,
    scale: f64,
}

#[cfg(not(feature = "gtk"))]
impl Default for TerminalView {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(not(feature = "gtk"))]
impl TerminalView {
    pub fn new() -> Self {
        Self {
            snapshot: None,
            enhanced: false,
            cell_width: DEFAULT_CELL_WIDTH,
            cell_height: DEFAULT_CELL_HEIGHT,
            scale: DEFAULT_SCALE,
        }
    }

    /// Replace the latest snapshot wholesale (do not patch in place).
    pub fn set_snapshot(&mut self, snapshot: TerminalSnapshot) {
        self.snapshot = Some(snapshot);
    }

    pub fn snapshot_model(&self) -> Option<&TerminalSnapshot> {
        self.snapshot.as_ref()
    }

    /// Enhancement toggle plumbing. Does not reconnect any backend.
    pub fn set_enhanced(&mut self, enhanced: bool) {
        self.enhanced = enhanced;
    }

    pub fn enhanced(&self) -> bool {
        self.enhanced
    }

    pub fn cell_width(&self) -> f64 {
        super::cell_renderer::snap_cell_px(self.cell_width, self.scale)
    }

    pub fn cell_height(&self) -> f64 {
        super::cell_renderer::snap_cell_px(self.cell_height, self.scale)
    }

    pub fn scale(&self) -> f64 {
        self.scale
    }

    pub fn set_scale(&mut self, scale: f64) {
        if scale.is_finite() && scale > 0.0 {
            self.scale = scale;
        }
    }

    /// Grid capacity for the resize owner via [`device_cols_rows`].
    pub fn cols_rows(&self, width_px: f64, height_px: f64) -> (u16, u16) {
        device_cols_rows(
            width_px,
            height_px,
            self.cell_width(),
            self.cell_height(),
            self.scale,
        )
    }
}

#[cfg(test)]
mod fit_tests {
    use super::{cell_under_pointer, letterbox_origin};

    #[test]
    fn smaller_snapshot_is_centered_and_larger_is_refused() {
        let origin = letterbox_origin(100.0, 40.0, 8.0, 16.0, 10, 2, 12, 2).unwrap();
        assert_eq!(origin, (10.0, 4.0));
        assert_eq!(
            letterbox_origin(100.0, 40.0, 8.0, 16.0, 12, 2, 12, 2),
            Some((2.0, 4.0))
        );
        assert!(letterbox_origin(100.0, 40.0, 8.0, 16.0, 13, 2, 12, 2).is_none());
        assert!(letterbox_origin(100.0, 40.0, 8.0, 16.0, 12, 3, 12, 2).is_none());
        assert_eq!(
            letterbox_origin(10.0, 10.0, 8.0, 16.0, 1, 1, 1, 1),
            Some((1.0, -3.0))
        );
    }

    #[test]
    fn pointer_on_left_and_right_of_a_letterboxed_grid_are_different_columns() {
        // 800×400 card, 8×16 cells, 80×20 snapshot. Grid is 640×320, centered
        // at (80, 40). Capacity is 100×25.
        let hit =
            |x: f64, y: f64| cell_under_pointer(x, y, 800.0, 400.0, 8.0, 16.0, 80, 20, 100, 25);
        assert_eq!(hit(0.0, 200.0), Some((0, 10)));
        assert_eq!(hit(80.0 + 8.0 * 4.0, 200.0), Some((4, 10)));
        assert_eq!(hit(80.0 + 8.0 * 50.0, 200.0), Some((50, 10)));
        assert!(hit(96.0, 200.0).unwrap().0 < hit(480.0, 200.0).unwrap().0);
    }
}

#[cfg(all(test, not(feature = "gtk")))]
mod tests {
    use super::*;
    use crate::terminal::TerminalSnapshot;

    #[test]
    fn defaults_and_snapshot_replace() {
        let mut view = TerminalView::new();
        assert!(!view.enhanced());
        assert_eq!(view.cell_width(), DEFAULT_CELL_WIDTH);
        assert_eq!(view.cell_height(), DEFAULT_CELL_HEIGHT);
        assert_eq!(view.scale(), DEFAULT_SCALE);
        assert_eq!(view.cols_rows(80.0, 32.0), (10, 2));

        view.set_enhanced(true);
        assert!(view.enhanced());

        let snap = TerminalSnapshot::blank(4, 3).unwrap();
        view.set_snapshot(snap);
        assert_eq!(view.snapshot_model().unwrap().cols(), 4);
        assert_eq!(view.snapshot_model().unwrap().rows(), 3);

        let snap2 = TerminalSnapshot::blank(2, 2).unwrap();
        view.set_snapshot(snap2);
        assert_eq!(view.snapshot_model().unwrap().cols(), 2);
    }
}
