use adw::prelude::*;
use gtk::{Entry, Switch};
use std::cell::RefCell;
use std::rc::Rc;

use crate::labels::Labeled;
use crate::widgets::suffix_row;
use berry_games::library::{GamescopeFilter, GamescopeMode, GamescopeSettings};
use berry_locale::fl;

fn spin_row(title: &str, subtitle: &str, value: u32, max: f64) -> adw::SpinRow {
    let row = adw::SpinRow::with_range(0.0, max, 1.0);
    row.set_title(title);
    row.set_subtitle(subtitle);
    row.set_value(f64::from(value));
    row
}

fn choice_dropdown(labels: Vec<String>, selected: usize) -> gtk::DropDown {
    let refs = labels.iter().map(String::as_str).collect::<Vec<_>>();
    let dropdown = gtk::DropDown::from_strings(&refs);
    dropdown.set_selected(selected as u32);
    dropdown.set_size_request(200, -1);
    dropdown
}

/// Small window with the usual gamescope options. The values are written back into `state`
/// when it closes; the caller persists them together with the rest of the game settings.
pub(crate) fn open_gamescope_settings(parent: &adw::Dialog, state: Rc<RefCell<GamescopeSettings>>) {
    let current = state.borrow().clone();
    let window = adw::Dialog::builder()
        .title(fl!("gamescope-title"))
        .content_width(crate::layout::PADDED_WINDOW_WIDTH)
        .content_height(640)
        .build();
    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&adw::HeaderBar::new());
    let page = adw::PreferencesPage::new();
    let auto = fl!("gamescope-auto-hint");

    let display = adw::PreferencesGroup::new();
    display.set_title(&fl!("gamescope-display-group"));
    let width = spin_row(&fl!("gamescope-width"), &auto, current.width, 16384.0);
    let height = spin_row(&fl!("gamescope-height"), &auto, current.height, 16384.0);
    let game_width = spin_row(
        &fl!("gamescope-game-width"),
        &auto,
        current.game_width,
        16384.0,
    );
    let game_height = spin_row(
        &fl!("gamescope-game-height"),
        &auto,
        current.game_height,
        16384.0,
    );
    let fps = spin_row(&fl!("gamescope-fps"), &auto, current.fps, 1000.0);
    let fps_unfocused = spin_row(
        &fl!("gamescope-fps-unfocused"),
        &auto,
        current.fps_unfocused,
        1000.0,
    );
    let mode = choice_dropdown(
        GamescopeMode::options().map(|v| v.label()).collect(),
        current.mode.index(),
    );
    for row in [
        &width,
        &height,
        &game_width,
        &game_height,
        &fps,
        &fps_unfocused,
    ] {
        display.add(row);
    }
    display.add(&suffix_row(&fl!("gamescope-mode-title"), None, &mode));

    let scaling = adw::PreferencesGroup::new();
    scaling.set_title(&fl!("gamescope-scaling-group"));
    let filter = choice_dropdown(
        GamescopeFilter::options().map(|v| v.label()).collect(),
        current.filter.index(),
    );
    let sharpness = adw::SpinRow::with_range(0.0, 20.0, 1.0);
    sharpness.set_title(&fl!("gamescope-sharpness"));
    sharpness.set_subtitle(&fl!("gamescope-sharpness-subtitle"));
    sharpness.set_value(f64::from(current.sharpness.min(20)));
    let integer = Switch::new();
    integer.set_active(current.integer_scaling);
    scaling.add(&suffix_row(&fl!("gamescope-filter-title"), None, &filter));
    scaling.add(&sharpness);
    scaling.add(&suffix_row(
        &fl!("gamescope-integer"),
        Some(&fl!("gamescope-integer-subtitle")),
        &integer,
    ));

    let misc = adw::PreferencesGroup::new();
    misc.set_title(&fl!("gamescope-misc-group"));
    let hdr = Switch::new();
    hdr.set_active(current.hdr);
    let grab = Switch::new();
    grab.set_active(current.force_grab_cursor);
    let extra = Entry::new();
    extra.set_text(&current.extra_args);
    extra.set_placeholder_text(Some(&fl!("gamescope-extra-placeholder")));
    extra.set_width_chars(24);
    misc.add(&suffix_row(&fl!("gamescope-hdr"), None, &hdr));
    misc.add(&suffix_row(&fl!("gamescope-grab"), None, &grab));
    misc.add(&suffix_row(&fl!("gamescope-extra"), None, &extra));

    page.add(&display);
    page.add(&scaling);
    page.add(&misc);
    toolbar.set_content(Some(&page));
    window.set_child(Some(&toolbar));

    window.connect_closed(move |_| {
        let mut saved = state.borrow_mut();
        saved.width = width.value() as u32;
        saved.height = height.value() as u32;
        saved.game_width = game_width.value() as u32;
        saved.game_height = game_height.value() as u32;
        saved.fps = fps.value() as u32;
        saved.fps_unfocused = fps_unfocused.value() as u32;
        saved.mode = GamescopeMode::from_index(mode.selected() as usize);
        saved.filter = GamescopeFilter::from_index(filter.selected() as usize);
        saved.sharpness = sharpness.value() as u32;
        saved.integer_scaling = integer.is_active();
        saved.hdr = hdr.is_active();
        saved.force_grab_cursor = grab.is_active();
        saved.extra_args = extra.text().to_string();
    });
    window.present(Some(parent));
}
