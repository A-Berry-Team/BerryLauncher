use adw::prelude::*;
use gtk::{Box, Button, Label, Orientation, Picture};
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use crate::game_details::open_game_details;
use crate::library_game::open_library_game;
use crate::widgets::rounded_card_picture;
use berry_games::catalog::GameCatalogEntry;
use berry_runners::RunnerService;

pub(crate) fn card_button(
    game: &GameCatalogEntry,
    library: bool,
    locked: bool,
    parent: &adw::ApplicationWindow,
    header: &adw::HeaderBar,
    navigation: &adw::NavigationView,
    runner_service: &Arc<RunnerService>,
    stack: Option<&adw::ViewStack>,
    store_navigation: Option<&adw::NavigationView>,
    store_back_button: Option<&Rc<RefCell<Option<Button>>>>,
    store_detail_open: Option<&Rc<RefCell<bool>>>,
) -> Button {
    let button = Button::new();
    button.set_has_frame(true);
    button.add_css_class("flat");
    button.add_css_class("berry-library-card");
    button.set_size_request(220, if library || locked { 300 } else { 330 });
    button.set_hexpand(false);
    button.set_vexpand(false);

    let box_ = Box::new(Orientation::Vertical, 8);
    box_.set_margin_top(6);
    box_.set_margin_bottom(6);
    box_.set_margin_start(6);
    box_.set_margin_end(6);

    let media_height = if library || locked { 206 } else { 275 };
    let media = rounded_card_picture(
        if library || locked {
            game.logo
        } else {
            game.poster
        },
        206,
        media_height,
    );
    if locked {
        if let Some(image) = media
            .child()
            .and_then(|child| child.downcast::<Picture>().ok())
        {
            image.add_css_class("berry-card-locked");
        }
    }
    box_.append(&media);

    let label = Label::new(Some(&game.localized_title()));
    label.set_wrap(true);
    label.set_lines(2);
    label.set_ellipsize(gtk::pango::EllipsizeMode::End);
    label.set_xalign(0.5);
    label.set_size_request(206, 40);
    box_.append(&label);
    button.set_child(Some(&box_));

    let game = *game;
    let parent = parent.clone();
    let header = header.clone();
    let navigation = navigation.clone();
    let runner_service = runner_service.clone();
    let stack = stack.cloned();
    let store_navigation = store_navigation.cloned();
    let store_back_button = store_back_button.cloned();
    let store_detail_open = store_detail_open.cloned();
    button.connect_clicked(move |_| {
        if library {
            open_library_game(&parent, &header, &navigation, game, &runner_service);
        } else if locked {
            if let (
                Some(stack),
                Some(store_navigation),
                Some(store_back_button),
                Some(store_detail_open),
            ) = (
                stack.as_ref(),
                store_navigation.as_ref(),
                store_back_button.as_ref(),
                store_detail_open.as_ref(),
            ) {
                store_navigation.pop_to_tag("store-root");
                if let Some(previous) = store_back_button.borrow_mut().take() {
                    header.remove(&previous);
                }
                *store_detail_open.borrow_mut() = false;
                stack.set_visible_child_name("store");
                open_game_details(
                    &parent,
                    &header,
                    store_navigation,
                    game,
                    store_back_button,
                    store_detail_open,
                );
            }
        } else if let (Some(store_navigation), Some(store_back_button), Some(store_detail_open)) = (
            store_navigation.as_ref(),
            store_back_button.as_ref(),
            store_detail_open.as_ref(),
        ) {
            open_game_details(
                &parent,
                &header,
                store_navigation,
                game,
                store_back_button,
                store_detail_open,
            );
        }
    });
    button
}
