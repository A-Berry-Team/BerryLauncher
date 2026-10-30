use adw::prelude::*;
use gtk::{glib, Align, Box, Button, Label, Orientation, Picture, Stack};
use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::rc::Rc;

use crate::ratio_layout::ratio_frame;
use crate::widgets::cover_picture;

const ASPECT: f64 = 16.0 / 9.0;
const LOADING: &str = "loading";

/// Image carousel of the store details page. Images keep their own aspect ratio and are cropped
/// to fill a 16:9 frame; they are decoded when they are about to be shown.
pub(crate) struct Gallery {
    pub(crate) carousel: Stack,
    position: Label,
    controls: glib::WeakRef<Box>,
    paths: RefCell<Vec<PathBuf>>,
    slides: RefCell<Vec<Picture>>,
    current: Cell<usize>,
    ready: Cell<bool>,
}

fn frame(child: &impl IsA<gtk::Widget>) -> Box {
    ratio_frame(child, ASPECT)
}

fn slide_name(index: usize) -> String {
    format!("slide-{index}")
}

impl Gallery {
    /// The gallery and the box holding its carousel and controls.
    pub(crate) fn new() -> (Rc<Self>, Box) {
        let media_box = Box::new(Orientation::Vertical, 10);
        media_box.set_hexpand(false);
        media_box.set_valign(Align::Start);

        let carousel = Stack::new();
        carousel.set_transition_type(gtk::StackTransitionType::SlideLeftRight);
        carousel.set_transition_duration(280);
        carousel.set_hexpand(true);
        carousel.set_halign(Align::Fill);
        carousel.set_valign(Align::Start);
        media_box.append(&carousel);

        let controls = Box::new(Orientation::Horizontal, 8);
        controls.set_halign(Align::Center);
        controls.set_visible(false);
        let previous = Button::from_icon_name("go-previous-symbolic");
        previous.set_has_frame(false);
        let position = Label::new(None);
        let next = Button::from_icon_name("go-next-symbolic");
        next.set_has_frame(false);
        controls.append(&previous);
        controls.append(&position);
        controls.append(&next);
        media_box.append(&controls);

        let gallery = Rc::new(Self {
            carousel,
            position,
            controls: controls.downgrade(),
            paths: RefCell::default(),
            slides: RefCell::default(),
            current: Cell::new(0),
            ready: Cell::new(false),
        });
        for (button, delta) in [(&previous, -1), (&next, 1)] {
            let gallery = gallery.clone();
            button.connect_clicked(move |_| gallery.step(delta));
        }
        (gallery, media_box)
    }

    /// Shows only a loading animation until `set_images` is called.
    pub(crate) fn show_loading(&self) {
        self.ready.set(false);
        self.clear();
        let spinner = adw::Spinner::new();
        spinner.set_halign(Align::Center);
        spinner.set_valign(Align::Center);
        spinner.set_size_request(32, 32);
        self.carousel.add_named(&frame(&spinner), Some(LOADING));
        self.set_controls_visible(false);
    }

    /// Ends the loading state without images (the download failed and nothing is cached).
    pub(crate) fn stop_loading(&self) {
        if !self.ready.get() {
            self.set_images(Vec::new());
        }
    }

    pub(crate) fn set_images(&self, paths: Vec<PathBuf>) {
        if self.ready.get() && *self.paths.borrow() == paths {
            return;
        }
        self.ready.set(true);
        self.clear();
        let slides: Vec<Picture> = paths
            .iter()
            .enumerate()
            .map(|(index, _)| {
                let picture = cover_picture();
                self.carousel
                    .add_named(&frame(&picture), Some(&slide_name(index)));
                picture
            })
            .collect();
        *self.paths.borrow_mut() = paths;
        *self.slides.borrow_mut() = slides;
        self.show(0);
    }

    fn clear(&self) {
        while let Some(child) = self.carousel.first_child() {
            self.carousel.remove(&child);
        }
        self.paths.borrow_mut().clear();
        self.slides.borrow_mut().clear();
    }

    fn set_controls_visible(&self, visible: bool) {
        if let Some(controls) = self.controls.upgrade() {
            controls.set_visible(visible);
        }
    }

    fn step(&self, delta: isize) {
        let count = self.slides.borrow().len();
        if count > 1 {
            let target = (self.current.get() as isize + delta).rem_euclid(count as isize);
            self.show(target as usize);
        }
    }

    fn show(&self, index: usize) {
        let count = self.slides.borrow().len();
        self.set_controls_visible(count > 1);
        if count == 0 {
            return;
        }
        let index = index % count;
        self.current.set(index);
        self.load(index);
        self.load((index + 1) % count);
        self.carousel.set_visible_child_name(&slide_name(index));
        self.position.set_text(&format!("{} / {count}", index + 1));
    }

    fn load(&self, index: usize) {
        let slides = self.slides.borrow();
        let Some(picture) = slides
            .get(index)
            .filter(|picture| picture.paintable().is_none())
        else {
            return;
        };
        let texture = self
            .paths
            .borrow()
            .get(index)
            .and_then(|path| berry_media::images::texture_from_file(path));
        picture.set_paintable(texture.as_ref());
    }
}
