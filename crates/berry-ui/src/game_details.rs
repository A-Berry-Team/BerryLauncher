use adw::prelude::*;
use gtk::pango::EllipsizeMode;
use gtk::{gio, glib, Align, Box, Button, Label, Orientation, ScrolledWindow};
use std::cell::RefCell;
use std::rc::Rc;

use crate::gallery::Gallery;
use crate::hero_layout;
use crate::labels::{CatalogText, Labeled};
use crate::widgets::{
    library_action_button, resource_picture_in_aspect_frame, set_library_action_button,
    symbolic_image,
};
use berry_games::catalog::{GameCatalogEntry, GameTag};
use berry_games::library as games;
use berry_locale::fl;
use berry_media::backgrounds;
use berry_media::screenshots;
use berry_runners::RunnerService;

// Shows the cached images at once and swaps in the Play Store listing when it has been fetched.
fn load_gallery(gallery: &Rc<Gallery>, game: GameCatalogEntry) {
    match screenshots::cached(&game) {
        Some(paths) => gallery.set_images(paths),
        None => gallery.show_loading(),
    }

    let (tx, rx) = async_channel::bounded(1);
    std::thread::spawn(move || {
        let _ = tx.send_blocking(screenshots::refresh(game));
    });
    let gallery = gallery.clone();
    glib::MainContext::default().spawn_local(async move {
        match rx.recv().await {
            Ok(Ok(paths)) => gallery.set_images(paths),
            Ok(Err(error)) => {
                tracing::warn!("[screenshots] {}: {error:#}", game.id);
                gallery.stop_loading();
            }
            Err(_) => gallery.stop_loading(),
        }
    });
}

pub(crate) fn open_game_details(
    parent: &adw::ApplicationWindow,
    main_header: &adw::HeaderBar,
    navigation: &adw::NavigationView,
    game: GameCatalogEntry,
    store_back_button: &Rc<RefCell<Option<Button>>>,
    store_detail_open: &Rc<RefCell<bool>>,
) {
    *store_detail_open.borrow_mut() = true;
    let back_button = Button::from_icon_name("go-previous-symbolic");
    back_button.set_tooltip_text(Some(&fl!("store-back")));
    back_button.set_has_frame(false);
    let navigation_back = navigation.clone();
    back_button.connect_clicked(move |_| {
        // The page's `hiding` handler (below) removes the button and clears the state.
        navigation_back.pop();
    });
    if let Some(previous) = store_back_button.borrow_mut().replace(back_button.clone()) {
        main_header.remove(&previous);
    }
    main_header.pack_start(&back_button);

    let scroller = ScrolledWindow::new();
    scroller.set_hscrollbar_policy(gtk::PolicyType::Never);
    scroller.set_vscrollbar_policy(gtk::PolicyType::Automatic);

    let content = Box::new(Orientation::Vertical, 0);
    content.set_margin_top(24);
    content.set_margin_bottom(32);
    crate::layout::apply_page_padding(&content);

    let hero = Box::new(Orientation::Horizontal, 0);
    hero.set_hexpand(true);
    hero.set_valign(Align::Start);

    let (gallery, media_box) = Gallery::new();
    load_gallery(&gallery, game);

    // Left column: promotional images, and the description under them over the same width.
    let media_column = Box::new(Orientation::Vertical, 0);
    media_column.set_hexpand(false);
    media_column.set_valign(Align::Start);
    media_column.append(&media_box);
    media_column.append(&about_section(&game));
    hero.append(&media_column);

    let side = Box::new(Orientation::Vertical, 14);
    side.set_hexpand(false);
    side.set_valign(Align::Start);
    let cover = resource_picture_in_aspect_frame(game.poster, 3.0 / 4.0);
    cover.set_hexpand(true);
    cover.set_halign(Align::Fill);
    cover.set_valign(Align::Start);
    side.append(&cover);

    let in_library = games::is_in_library(game.folder_name);
    let library_label = if in_library {
        fl!("library-remove")
    } else {
        fl!("store-add-to-library")
    };
    let add_button = library_action_button(
        &library_label,
        if in_library {
            "user-trash-symbolic"
        } else {
            "list-add-symbolic"
        },
        in_library,
    );
    add_button.set_hexpand(true);
    add_button.set_halign(Align::Fill);
    let folder = game.folder_name;
    let game_for_action = game;
    let parent_for_action = parent.clone();
    add_button.connect_clicked(move |button| {
        if games::is_in_library(folder) {
            remove_game_from_library_with_confirmation(&parent_for_action, game_for_action, button);
        } else if games::add_to_library(folder).is_ok() {
            let default_runner = RunnerService::new().ok().and_then(|s| s.default_runner());
            let _ = games::ensure_runner_setting(game_for_action.id, folder, default_runner);
            set_library_action_button(button, &fl!("library-remove"), "user-trash-symbolic", true);
            let game = game_for_action;
            std::thread::spawn(move || {
                let _ = backgrounds::update_background(game);
            });
        }
    });
    side.append(&add_button);

    // Right column, under the poster and the library button: who made it, then the tags.
    side.append(&developer_publisher(&game));
    side.append(&tag_list(&game));

    hero.append(&side);
    // Both columns follow the page width; images and poster start at the same height and have
    // the same size (see `hero_layout`).
    hero_layout::install(&hero);
    content.append(&hero);

    scroller.set_child(Some(&content));
    let page = adw::NavigationPage::new(&scroller, &game.localized_title());
    page.set_tag(Some(game.id));
    // Whatever closes the page (the button, Esc, a swipe, `pop_to_tag`) ends up here, so the back
    // button in the header can never outlive the page it belongs to.
    {
        let header = main_header.clone();
        let holder = store_back_button.clone();
        let detail = store_detail_open.clone();
        page.connect_hiding(move |_| clear_store_back(&header, &holder, &detail));
    }
    navigation.push(&page);
}

/// Remove the Store's back button from the header (if shown) and mark the detail as closed.
pub(crate) fn clear_store_back(
    header: &adw::HeaderBar,
    holder: &Rc<RefCell<Option<Button>>>,
    detail: &Rc<RefCell<bool>>,
) {
    let button = holder.borrow_mut().take();
    if let Some(button) = button {
        header.remove(&button);
    }
    *detail.borrow_mut() = false;
}

/// "About the game" and its description, shown under the promotional images.
fn about_section(game: &GameCatalogEntry) -> Box {
    let section = Box::new(Orientation::Vertical, 8);
    section.set_margin_top(24);
    let title = Label::new(Some(&fl!("game-about")));
    title.add_css_class("title-3");
    title.set_xalign(0.0);
    let description = Label::new(Some(&game.localized_description()));
    description.set_wrap(true);
    description.set_xalign(0.0);
    description.add_css_class("body");
    section.append(&title);
    section.append(&description);
    section
}

/// "Developer" and "Publisher" of a game, one under the other: small caption above the name.
fn developer_publisher(game: &GameCatalogEntry) -> Box {
    let section = Box::new(Orientation::Vertical, 10);
    section.set_margin_top(10);
    for (caption, value) in [
        (fl!("game-developer"), game.developer()),
        (fl!("game-publisher"), game.publisher()),
    ] {
        let column = Box::new(Orientation::Vertical, 2);
        let caption = Label::new(Some(&caption));
        caption.set_xalign(0.0);
        caption.add_css_class("caption-heading");
        caption.add_css_class("dim-label");
        let value = Label::new(Some(value));
        value.set_xalign(0.0);
        value.set_wrap(true);
        value.set_selectable(true);
        column.append(&caption);
        column.append(&value);
        section.append(&column);
    }
    section
}

/// Symbolic icon shown on the left of each tag (bundled in `src/resources/icons`).
fn tag_icon(tag: GameTag) -> &'static str {
    match tag {
        GameTag::Action => "tag-action-symbolic",
        GameTag::Adventure => "tag-adventure-symbolic",
        GameTag::Rpg => "tag-rpg-symbolic",
        GameTag::OpenWorld => "tag-open-world-symbolic",
        GameTag::TurnBased => "tag-turn-based-symbolic",
        GameTag::Multiplayer => "tag-multiplayer-symbolic",
        GameTag::Fantasy => "tag-fantasy-symbolic",
        GameTag::SciFi => "tag-sci-fi-symbolic",
        GameTag::Gacha => "tag-gacha-symbolic",
    }
}

/// One tag: a wide rounded bubble with its icon on the left.
fn tag_pill(tag: GameTag) -> Box {
    let pill = Box::new(Orientation::Horizontal, 8);
    pill.add_css_class("berry-tag");
    pill.set_halign(Align::Start);
    pill.append(&symbolic_image(tag_icon(tag)));
    let label = Label::new(Some(&tag.label()));
    label.set_ellipsize(EllipsizeMode::End);
    label.set_xalign(0.0);
    pill.append(&label);
    pill
}

/// The game's genre and feature tags, one bubble per line.
fn tag_list(game: &GameCatalogEntry) -> Box {
    let section = Box::new(Orientation::Vertical, 8);
    section.set_margin_top(10);
    let tags = game.tags();
    if tags.is_empty() {
        section.set_visible(false);
        return section;
    }
    let caption = Label::new(Some(&fl!("game-tags")));
    caption.set_xalign(0.0);
    caption.add_css_class("caption-heading");
    caption.add_css_class("dim-label");
    section.append(&caption);
    for tag in tags {
        section.append(&tag_pill(*tag));
    }
    section
}

pub(crate) fn remove_game_from_library_with_confirmation(
    parent: &adw::ApplicationWindow,
    game: GameCatalogEntry,
    button: &Button,
) {
    let path = games::game_path(game.folder_name);
    let has_content = std::fs::read_dir(&path)
        .map(|mut it| it.next().is_some())
        .unwrap_or(false);
    if !has_content {
        if let Err(error) = games::remove_from_library(game.folder_name) {
            let dialog = adw::AlertDialog::builder()
                .heading(&fl!("library-remove-error"))
                .body(error.to_string())
                .build();
            dialog.present(Some(parent));
            return;
        }
        set_library_action_button(
            button,
            &fl!("store-add-to-library"),
            "list-add-symbolic",
            false,
        );
        return;
    }

    let dialog = adw::AlertDialog::builder()
        .heading(&fl!("library-remove-confirm-title"))
        .body(&fl!("library-remove-confirm-detail"))
        .build();
    dialog.add_response("cancel", &fl!("common-cancel"));
    dialog.add_response("remove", &fl!("common-remove"));
    dialog.set_close_response("cancel");
    dialog.set_default_response(Some("remove"));
    let choose_parent = parent.clone();
    let callback_parent = parent.clone();
    let button = button.clone();
    dialog.choose(
        Some(&choose_parent),
        None::<&gio::Cancellable>,
        move |result| {
            if result.as_str() != "remove" {
                return;
            }
            if let Err(error) = games::remove_from_library(game.folder_name) {
                let dialog = adw::AlertDialog::builder()
                    .heading(&fl!("library-remove-error"))
                    .body(error.to_string())
                    .build();
                dialog.present(Some(&callback_parent));
                return;
            }
            set_library_action_button(
                &button,
                &fl!("store-add-to-library"),
                "list-add-symbolic",
                false,
            );
        },
    );
}
