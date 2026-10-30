//! Layout of the store details "hero": two columns, the promotional images (with the description
//! below them) on the left and the poster (with the library button, developer / publisher and
//! tags below it) on the right.
//!
//! The images and the poster start at the same height and have the same size: the images are 16:9
//! and the poster 3:4, so for a total width
//! `W` the images get `64/91` and the poster `27/91` of `W - GAP`. The widths are *derived from
//! the allocation* by a [`gtk::CustomLayout`]; nothing is pinned with `set_size_request`. The
//! previous implementation pinned the children to sizes computed from the page width, which made
//! the page's minimum width equal to its current width: the window could grow but never shrink
//! again.
//!
//! The only minimum the layout reports is the one the children really need (for example the
//! "add to library" button under the poster), so the window can shrink down to that.

use gtk::glib;
use gtk::prelude::*;
use gtk::subclass::prelude::*;
use gtk::{Orientation, SizeRequestMode, Widget};

/// Space between the images and the poster.
const GAP: i32 = 24;
/// Share of the usable width: 16:9 images next to a 3:4 poster of the same height.
const MEDIA_PART: i32 = 64;
const SIDE_PART: i32 = 27;
const PARTS: i32 = MEDIA_PART + SIDE_PART;
/// Preferred width when the parent has no opinion.
const NATURAL_WIDTH: i32 = 960;

/// Widths of the two columns for a hero `width` wide.
fn split(width: i32) -> (i32, i32) {
    let usable = (width - GAP).max(0);
    let media = (usable * MEDIA_PART + PARTS / 2) / PARTS;
    (media, usable - media)
}

/// Smallest hero width at which both columns still get what they need at minimum.
fn width_for_minimums(media_min: i32, side_min: i32) -> i32 {
    // Round up so that `split` hands every column at least its minimum.
    let media = (media_min * PARTS + MEDIA_PART - 1) / MEDIA_PART;
    let side = (side_min * PARTS + SIDE_PART - 1) / SIDE_PART;
    GAP + media.max(side)
}

fn columns(widget: &Widget) -> Option<(Widget, Widget)> {
    let media = widget.first_child()?;
    let side = media.next_sibling()?;
    Some((media, side))
}

fn measure_hero(widget: &Widget, orientation: Orientation, for_size: i32) -> (i32, i32, i32, i32) {
    let Some((media, side)) = columns(widget) else {
        return (0, 0, -1, -1);
    };
    match orientation {
        Orientation::Horizontal => {
            let minimum = width_for_minimums(
                media.measure(Orientation::Horizontal, -1).0,
                side.measure(Orientation::Horizontal, -1).0,
            );
            (minimum, NATURAL_WIDTH.max(minimum), -1, -1)
        }
        _ => {
            let width = if for_size >= 0 {
                for_size
            } else {
                NATURAL_WIDTH
            };
            let (media_width, side_width) = split(width);
            let media = media.measure(Orientation::Vertical, media_width);
            let side = side.measure(Orientation::Vertical, side_width);
            (media.0.max(side.0), media.1.max(side.1), -1, -1)
        }
    }
}

fn allocate_hero(widget: &Widget, width: i32) {
    let Some((media, side)) = columns(widget) else {
        return;
    };
    let (media_width, side_width) = split(width);
    for (child, x, child_width) in [
        (&media, 0, media_width),
        (&side, width - side_width, side_width),
    ] {
        // Each column takes the height it needs for its width and sticks to the top.
        let child_height = child.measure(Orientation::Vertical, child_width).1;
        child.size_allocate(&gtk::Allocation::new(x, 0, child_width, child_height), -1);
    }
}

mod imp {
    use super::*;

    #[derive(Default)]
    pub struct HeroLayout;

    #[glib::object_subclass]
    impl ObjectSubclass for HeroLayout {
        const NAME: &'static str = "BerryHeroLayout";
        type Type = super::HeroLayout;
        type ParentType = gtk::LayoutManager;
    }

    impl ObjectImpl for HeroLayout {}

    impl LayoutManagerImpl for HeroLayout {
        fn request_mode(&self, _widget: &Widget) -> SizeRequestMode {
            SizeRequestMode::HeightForWidth
        }

        fn measure(
            &self,
            widget: &Widget,
            orientation: Orientation,
            for_size: i32,
        ) -> (i32, i32, i32, i32) {
            measure_hero(widget, orientation, for_size)
        }

        fn allocate(&self, widget: &Widget, width: i32, _height: i32, _baseline: i32) {
            allocate_hero(widget, width);
        }
    }
}

glib::wrapper! {
    /// Layout manager of the store details hero (see the module docs).
    pub struct HeroLayout(ObjectSubclass<imp::HeroLayout>) @extends gtk::LayoutManager;
}

/// Makes `hero` lay out its first two children as the images / poster pair.
pub(crate) fn install(hero: &impl IsA<Widget>) {
    hero.set_layout_manager(Some(glib::Object::new::<HeroLayout>()));
}
