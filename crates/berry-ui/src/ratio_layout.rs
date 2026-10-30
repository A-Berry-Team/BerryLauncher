//! A frame that keeps a fixed width : height ratio *and reports it to its parent*.
//!
//! `GtkAspectFrame` only fits its child inside whatever space it is given: it never says "for this
//! width I need this height", so a page that holds one needs fixed `set_size_request` heights to
//! get any size at all, and those pins are what stopped the store details page from shrinking.
//! [`ratio_frame`] is height-for-width aware: its minimum width is the child's own (a picture that
//! can shrink needs none) and the height it asks for follows from the width it gets.

use std::cell::Cell;

use gtk::glib;
use gtk::prelude::*;
use gtk::subclass::prelude::*;
use gtk::{Orientation, SizeRequestMode, Widget};

/// Height for `width` at `ratio` (width / height).
pub(crate) fn height_for(width: i32, ratio: f64) -> i32 {
    (f64::from(width.max(0)) / ratio).round() as i32
}

/// Largest `ratio` box that fits in `width` x `height`.
fn fit(width: i32, height: i32, ratio: f64) -> (i32, i32) {
    let wanted = height_for(width, ratio);
    if wanted <= height {
        (width, wanted)
    } else {
        (
            (f64::from(height.max(0)) * ratio).round() as i32,
            height.max(0),
        )
    }
}

mod imp {
    use super::*;

    pub struct RatioLayout {
        pub ratio: Cell<f64>,
    }

    impl Default for RatioLayout {
        fn default() -> Self {
            Self {
                ratio: Cell::new(1.0),
            }
        }
    }

    #[glib::object_subclass]
    impl ObjectSubclass for RatioLayout {
        const NAME: &'static str = "BerryRatioLayout";
        type Type = super::RatioLayout;
        type ParentType = gtk::LayoutManager;
    }

    impl ObjectImpl for RatioLayout {}

    impl LayoutManagerImpl for RatioLayout {
        fn request_mode(&self, _widget: &Widget) -> SizeRequestMode {
            SizeRequestMode::HeightForWidth
        }

        fn measure(
            &self,
            widget: &Widget,
            orientation: Orientation,
            for_size: i32,
        ) -> (i32, i32, i32, i32) {
            let Some(child) = widget.first_child().filter(|child| child.should_layout()) else {
                return (0, 0, -1, -1);
            };
            let ratio = self.ratio.get();
            match orientation {
                Orientation::Horizontal => {
                    let (minimum, natural, ..) = child.measure(Orientation::Horizontal, -1);
                    (minimum, natural, -1, -1)
                }
                _ if for_size >= 0 => {
                    let (minimum, ..) = child.measure(Orientation::Vertical, for_size);
                    let height = height_for(for_size, ratio).max(minimum);
                    (height, height, -1, -1)
                }
                _ => {
                    let (minimum, natural, ..) = child.measure(Orientation::Vertical, -1);
                    (minimum, natural, -1, -1)
                }
            }
        }

        fn allocate(&self, widget: &Widget, width: i32, height: i32, _baseline: i32) {
            let Some(child) = widget.first_child().filter(|child| child.should_layout()) else {
                return;
            };
            let (child_width, child_height) = fit(width, height, self.ratio.get());
            let (x, y) = ((width - child_width) / 2, (height - child_height) / 2);
            child.size_allocate(&gtk::Allocation::new(x, y, child_width, child_height), -1);
        }
    }
}

glib::wrapper! {
    pub struct RatioLayout(ObjectSubclass<imp::RatioLayout>) @extends gtk::LayoutManager;
}

/// Wraps `child` in a rounded, clipped frame of the given `ratio` (width / height).
pub(crate) fn ratio_frame(child: &impl IsA<Widget>, ratio: f64) -> gtk::Box {
    let frame = gtk::Box::new(Orientation::Vertical, 0);
    frame.add_css_class("berry-image-frame");
    frame.set_overflow(gtk::Overflow::Hidden);
    frame.append(child);
    let layout: RatioLayout = glib::Object::new();
    layout.imp().ratio.set(ratio);
    frame.set_layout_manager(Some(layout));
    frame
}
