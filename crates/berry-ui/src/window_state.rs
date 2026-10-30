use adw::prelude::*;
use gtk::glib;
use std::cell::Cell;
use std::rc::Rc;

/// Remembers the last non-maximized size and restores it when the window leaves maximized state.
pub(crate) fn track_normal_window_size(window: &adw::ApplicationWindow) {
    let normal_window_size = Rc::new(Cell::new((1152, 648)));
    let restoring_window_size = Rc::new(Cell::new(false));

    for property in ["width", "height"] {
        let normal_window_size = normal_window_size.clone();
        let restoring_window_size = restoring_window_size.clone();
        window.connect_notify_local(Some(property), move |window, _| {
            if !restoring_window_size.get()
                && !window.is_maximized()
                && window.width() > 0
                && window.height() > 0
            {
                normal_window_size.set((window.width(), window.height()));
            }
        });
    }

    window.connect_maximized_notify(move |window| {
        if !window.is_maximized() {
            let (width, height) = normal_window_size.get();
            restoring_window_size.set(true);
            window.set_default_size(width, height);
            let window = window.clone();
            let restoring_window_size = restoring_window_size.clone();
            glib::idle_add_local_once(move || {
                window.set_default_size(width, height);
                glib::idle_add_local_once(move || restoring_window_size.set(false));
            });
        }
    });
}
