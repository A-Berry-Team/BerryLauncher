use adw::prelude::*;
use gtk::{Align, Box, Button, FlowBox, FlowBoxChild, Label, Orientation, ScrolledWindow};
use std::cell::RefCell;
use std::rc::Rc;

use crate::cards::card_button;
use crate::sorting::{sort_button, sorted_games, SortScope};
use berry_games::catalog::{self, GameCatalogEntry};
use berry_locale::fl;
use berry_runners::RunnerService;

pub(crate) fn build_store_page(
    parent: &adw::ApplicationWindow,
    header: &adw::HeaderBar,
    navigation: &adw::NavigationView,
    store_back_button: &Rc<RefCell<Option<Button>>>,
    store_detail_open: &Rc<RefCell<bool>>,
) -> gtk::ScrolledWindow {
    let scroll = ScrolledWindow::new();
    scroll.set_hscrollbar_policy(gtk::PolicyType::Never);
    let content = Box::new(Orientation::Vertical, 16);
    content.set_margin_top(28);
    content.set_margin_bottom(36);
    crate::layout::apply_page_padding(&content);

    let heading = Label::new(Some(&fl!("store-title")));
    heading.set_halign(Align::Start);
    heading.set_hexpand(true);
    heading.add_css_class("title-1");
    let heading_row = Box::new(Orientation::Horizontal, 12);
    heading_row.append(&heading);
    content.append(&heading_row);

    let description = Label::new(Some(&fl!("store-subtitle")));
    description.set_halign(Align::Start);
    description.set_wrap(true);
    description.add_css_class("dim-label");
    content.append(&description);

    let grid = FlowBox::new();
    grid.set_selection_mode(gtk::SelectionMode::None);
    grid.set_homogeneous(true);
    grid.set_row_spacing(18);
    grid.set_column_spacing(18);
    grid.set_max_children_per_line(3);
    grid.set_min_children_per_line(3);
    populate_store(
        &grid,
        parent,
        header,
        navigation,
        store_back_button,
        store_detail_open,
    );
    content.append(&grid);

    let sort = {
        let grid = grid.clone();
        let (parent, header, navigation) = (parent.clone(), header.clone(), navigation.clone());
        let (store_back_button, store_detail_open) =
            (store_back_button.clone(), store_detail_open.clone());
        sort_button(SortScope::Store, move || {
            populate_store(
                &grid,
                &parent,
                &header,
                &navigation,
                &store_back_button,
                &store_detail_open,
            );
        })
    };
    heading_row.append(&sort);
    scroll.set_child(Some(&content));
    scroll
}

/// Fills `grid` with one card per game, alphabetical in the Store's own sort order.
fn populate_store(
    grid: &FlowBox,
    parent: &adw::ApplicationWindow,
    header: &adw::HeaderBar,
    navigation: &adw::NavigationView,
    store_back_button: &Rc<RefCell<Option<Button>>>,
    store_detail_open: &Rc<RefCell<bool>>,
) {
    while let Some(child) = grid.first_child() {
        grid.remove(&child);
    }
    for game in sorted_games(catalog::GAMES, SortScope::Store, false) {
        grid.insert(
            &store_card(
                &game,
                parent,
                header,
                navigation,
                store_back_button,
                store_detail_open,
            ),
            -1,
        );
    }
}

pub(crate) fn store_card(
    game: &GameCatalogEntry,
    parent: &adw::ApplicationWindow,
    header: &adw::HeaderBar,
    navigation: &adw::NavigationView,
    store_back_button: &Rc<RefCell<Option<Button>>>,
    store_detail_open: &Rc<RefCell<bool>>,
) -> FlowBoxChild {
    let child = FlowBoxChild::new();
    let runner_service = RunnerService::new().expect("runner service");
    child.set_child(Some(&card_button(
        game,
        false,
        false,
        parent,
        header,
        navigation,
        &runner_service,
        None,
        Some(navigation),
        Some(store_back_button),
        Some(store_detail_open),
    )));
    child
}
