//! Ratio split for tab layouts. The gap is chrome between cards; each pane's
//! grid is measured from its own allocation.

/// Pixel lengths of the two children after `gap` is removed.
///
/// `ratio` is the first child's share of the remaining space (left or top).
pub(crate) fn split_lengths(total: i32, gap: i32, ratio: f64) -> (i32, i32) {
    let inner = total.saturating_sub(gap.max(0)).max(0);
    if inner == 0 || !ratio.is_finite() {
        return (0, 0);
    }
    let first = (f64::from(inner) * ratio).round() as i32;
    let first = first.clamp(0, inner);
    (first, inner - first)
}

#[cfg(feature = "gtk")]
mod gtk_split {
    use super::split_lengths;
    use std::cell::{Cell, RefCell};

    use gtk::prelude::*;
    use gtk::subclass::prelude::*;
    use gtk::{glib, graphene, gsk};

    pub const GAP_PX: i32 = 8;

    pub struct RatioSplitImp {
        orientation: Cell<gtk::Orientation>,
        ratio: Cell<f64>,
        first: RefCell<Option<gtk::Widget>>,
        second: RefCell<Option<gtk::Widget>>,
    }

    impl Default for RatioSplitImp {
        fn default() -> Self {
            Self {
                orientation: Cell::new(gtk::Orientation::Horizontal),
                ratio: Cell::new(0.5),
                first: RefCell::new(None),
                second: RefCell::new(None),
            }
        }
    }

    #[glib::object_subclass]
    impl ObjectSubclass for RatioSplitImp {
        const NAME: &'static str = "HerdrRatioSplit";
        type Type = RatioSplit;
        type ParentType = gtk::Widget;

        fn class_init(klass: &mut Self::Class) {
            klass.set_css_name("ratio-split");
        }
    }

    impl ObjectImpl for RatioSplitImp {
        fn dispose(&self) {
            if let Some(child) = self.first.borrow_mut().take() {
                child.unparent();
            }
            if let Some(child) = self.second.borrow_mut().take() {
                child.unparent();
            }
        }
    }

    impl WidgetImpl for RatioSplitImp {
        fn measure(&self, orientation: gtk::Orientation, for_size: i32) -> (i32, i32, i32, i32) {
            let first = self.first.borrow().clone();
            let second = self.second.borrow().clone();
            let (Some(first), Some(second)) = (first, second) else {
                return (0, 0, -1, -1);
            };
            let (first_min, first_nat, _, _) = first.measure(orientation, for_size);
            let (second_min, second_nat, _, _) = second.measure(orientation, for_size);
            if orientation == self.orientation.get() {
                (
                    first_min.saturating_add(second_min).saturating_add(GAP_PX),
                    first_nat.saturating_add(second_nat).saturating_add(GAP_PX),
                    -1,
                    -1,
                )
            } else {
                (first_min.max(second_min), first_nat.max(second_nat), -1, -1)
            }
        }

        fn size_allocate(&self, width: i32, height: i32, _baseline: i32) {
            let first = self.first.borrow().clone();
            let second = self.second.borrow().clone();
            let (Some(first), Some(second)) = (first, second) else {
                return;
            };
            let along = match self.orientation.get() {
                gtk::Orientation::Horizontal => width,
                _ => height,
            };
            let (primary, rest) = split_lengths(along, GAP_PX, self.ratio.get());
            let offset = primary.saturating_add(GAP_PX);
            match self.orientation.get() {
                gtk::Orientation::Horizontal => {
                    first.allocate(primary, height, -1, None);
                    let transform =
                        gsk::Transform::new().translate(&graphene::Point::new(offset as f32, 0.0));
                    second.allocate(rest, height, -1, Some(transform));
                }
                _ => {
                    first.allocate(width, primary, -1, None);
                    let transform =
                        gsk::Transform::new().translate(&graphene::Point::new(0.0, offset as f32));
                    second.allocate(width, rest, -1, Some(transform));
                }
            }
        }
    }

    glib::wrapper! {
        pub struct RatioSplit(ObjectSubclass<RatioSplitImp>)
            @extends gtk::Widget,
            @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
    }

    impl RatioSplit {
        pub fn new(
            orientation: gtk::Orientation,
            ratio: f64,
            first: &impl IsA<gtk::Widget>,
            second: &impl IsA<gtk::Widget>,
        ) -> Self {
            let obj: Self = glib::Object::new();
            let imp = obj.imp();
            imp.orientation.set(orientation);
            imp.ratio.set(ratio);
            let first = first.clone().upcast::<gtk::Widget>();
            let second = second.clone().upcast::<gtk::Widget>();
            first.set_parent(&obj);
            second.set_parent(&obj);
            imp.first.replace(Some(first));
            imp.second.replace(Some(second));
            obj.set_hexpand(true);
            obj.set_vexpand(true);
            obj.set_overflow(gtk::Overflow::Hidden);
            obj
        }
    }
}

#[cfg(feature = "gtk")]
pub use gtk_split::RatioSplit;

#[cfg(test)]
mod tests {
    use super::split_lengths;

    #[test]
    fn split_lengths_follow_ratio() {
        assert_eq!(split_lengths(100, 0, 0.25), (25, 75));
        let (first, second) = split_lengths(161, 8, 0.48795182);
        assert_eq!(first + second, 153);
        assert!((f64::from(first) / 153.0 - 0.48795182).abs() < 0.01);
    }
}
