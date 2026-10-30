use adw::prelude::*;
use gtk::{Align, Box, Button, FlowBox, Label, Orientation, ScrolledWindow};
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use crate::cards::card_button;
use crate::sorting::{sort_button, sorted_games, SortScope};
use berry_games::catalog::{self};
use berry_games::library as games;
use berry_locale::fl;
use berry_runners::RunnerService;

pub(crate) fn build_library_page(
    parent: &adw::ApplicationWindow,
    header: &adw::HeaderBar,
    navigation: &adw::NavigationView,
    runner_service: &Arc<RunnerService>,
    stack: &adw::ViewStack,
    store_navigation: &adw::NavigationView,
    store_back_button: &Rc<RefCell<Option<Button>>>,
    store_detail_open: &Rc<RefCell<bool>>,
) -> (ScrolledWindow, FlowBox, Label) {
    let scroll = ScrolledWindow::new();
    scroll.set_hscrollbar_policy(gtk::PolicyType::Never);
    let content = Box::new(Orientation::Vertical, 16);
    content.set_margin_top(28);
    content.set_margin_bottom(36);
    crate::layout::apply_page_padding(&content);
    scroll.set_child(Some(&content));

    let heading = Label::new(Some(&fl!("nav-games")));
    heading.set_halign(Align::Start);
    heading.set_hexpand(true);
    heading.add_css_class("title-1");
    let heading_row = Box::new(Orientation::Horizontal, 12);
    heading_row.append(&heading);
    content.append(&heading_row);

    let count = library_count_label();
    content.append(&count);

    let grid = FlowBox::new();
    grid.set_selection_mode(gtk::SelectionMode::None);
    grid.set_row_spacing(18);
    grid.set_column_spacing(18);
    grid.set_min_children_per_line(3);
    grid.set_max_children_per_line(3);
    grid.set_homogeneous(true);
    content.append(&grid);

    let sort = {
        let (grid, count) = (grid.clone(), count.clone());
        let (parent, header, navigation) = (parent.clone(), header.clone(), navigation.clone());
        let runner_service = runner_service.clone();
        let (stack, store_navigation) = (stack.clone(), store_navigation.clone());
        let (store_back_button, store_detail_open) =
            (store_back_button.clone(), store_detail_open.clone());
        sort_button(SortScope::Library, move || {
            populate_library(
                &grid,
                &count,
                &parent,
                &header,
                &navigation,
                &runner_service,
                &stack,
                &store_navigation,
                &store_back_button,
                &store_detail_open,
            );
        })
    };
    heading_row.append(&sort);

    populate_library(
        &grid,
        &count,
        parent,
        header,
        navigation,
        runner_service,
        stack,
        store_navigation,
        store_back_button,
        store_detail_open,
    );
    (scroll, grid, count)
}

pub(crate) fn library_count_label() -> Label {
    let count = catalog::GAMES
        .iter()
        .filter(|game| games::is_in_library(game.folder_name))
        .count();
    let label = fl!("library-count", count = count);
    let widget = Label::new(Some(&label));
    widget.set_halign(Align::Start);
    widget.add_css_class("dim-label");
    widget
}

pub(crate) fn populate_library(
    grid: &FlowBox,
    count_label: &Label,
    parent: &adw::ApplicationWindow,
    header: &adw::HeaderBar,
    navigation: &adw::NavigationView,
    runner_service: &Arc<RunnerService>,
    stack: &adw::ViewStack,
    store_navigation: &adw::NavigationView,
    store_back_button: &Rc<RefCell<Option<Button>>>,
    store_detail_open: &Rc<RefCell<bool>>,
) {
    while let Some(child) = grid.first_child() {
        grid.remove(&child);
    }

    let count = catalog::GAMES
        .iter()
        .filter(|game| games::is_in_library(game.folder_name))
        .count();
    let count_text = fl!(
        "library-filtered-count",
        count = count,
        total = catalog::GAMES.len()
    );
    count_label.set_text(&count_text);
    grid.set_visible(true);

    // Games in the library come first; each of the two lists is alphabetical.
    for game in sorted_games(catalog::GAMES, SortScope::Library, true) {
        grid.insert(
            &card_button(
                &game,
                games::is_in_library(game.folder_name),
                !games::is_in_library(game.folder_name),
                parent,
                header,
                navigation,
                runner_service,
                Some(stack),
                Some(store_navigation),
                Some(store_back_button),
                Some(store_detail_open),
            ),
            -1,
        );
    }
}
