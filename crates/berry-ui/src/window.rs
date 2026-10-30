use adw::prelude::*;
use gtk::{glib, Button};
use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};

use std::rc::Rc;
use std::sync::Arc;

use crate::library_game::open_library_game;
use crate::menu::{apply_theme, install_main_menu};
use crate::pages::library::{build_library_page, populate_library};
use crate::pages::runners::{
    build_runners_page, show_all_runners, Catalogs, Lists, RunnerMessage, ShowButtons,
};
use crate::pages::store::build_store_page;
use crate::style::install_css;
use berry_downloads as downloads;
use berry_games::catalog::{self};
use berry_games::library as games;
use berry_locale::fl;
use berry_media::backgrounds;
use berry_runners::RunnerProvider;
use berry_runners::RunnerService;

/// Set right before closing the window on purpose (the "close when a game starts" behaviour),
/// so that close does not ask for confirmation.
static SKIP_QUIT_CONFIRMATION: AtomicBool = AtomicBool::new(false);

/// Close the main window without the "downloads in progress" question.
pub(crate) fn close_without_confirmation(window: &adw::ApplicationWindow) {
    SKIP_QUIT_CONFIRMATION.store(true, Ordering::SeqCst);
    window.close();
}

/// Ask before closing the launcher while a game is being installed, updated or repaired.
/// Closing stops those operations in a resumable state, so the question only warns about it.
fn install_quit_confirmation(window: &adw::ApplicationWindow) {
    window.connect_close_request(|window| {
        let preferences = berry_core::settings::load_preferences();
        let busy = catalog::GAMES
            .iter()
            .any(|game| downloads::is_downloading(game.id));
        if !preferences.confirm_quit_while_downloading
            || !busy
            || SKIP_QUIT_CONFIRMATION.load(Ordering::SeqCst)
        {
            return glib::Propagation::Proceed;
        }

        let dialog = adw::AlertDialog::new(
            Some(&fl!("quit-confirm-heading")),
            Some(&fl!("quit-confirm-body")),
        );
        dialog.add_response("cancel", &fl!("common-cancel"));
        dialog.add_response("close", &fl!("quit-confirm-close"));
        dialog.set_response_appearance("close", adw::ResponseAppearance::Destructive);
        dialog.set_default_response(Some("cancel"));
        dialog.set_close_response("cancel");
        let window_for_response = window.clone();
        dialog.connect_response(None, move |_, response| {
            if response == "close" {
                close_without_confirmation(&window_for_response);
            }
        });
        dialog.present(Some(window));
        glib::Propagation::Stop
    });
}

pub struct MainWindow {
    window: adw::ApplicationWindow,
    _runner_service: Arc<RunnerService>,
}

impl MainWindow {
    pub fn new(app: &adw::Application) -> Self {
        install_css();
        let preferences = berry_core::settings::load_preferences();
        apply_theme(preferences.theme);

        let runner_service = RunnerService::new().expect("Could not initialize runner manager");
        let catalogs: Catalogs = Rc::new(RefCell::new(HashMap::new()));
        let lists: Lists = Rc::new(RefCell::new(HashMap::new()));
        let show_buttons: ShowButtons = Rc::new(RefCell::new(HashMap::new()));
        let (sender, receiver) = async_channel::unbounded::<RunnerMessage>();

        let window = adw::ApplicationWindow::builder()
            .application(app)
            .title("Berry Launcher")
            .default_width(1152)
            .default_height(648)
            .build();

        super::window_state::track_normal_window_size(&window);
        install_quit_confirmation(&window);

        let toolbar = adw::ToolbarView::new();
        let header = adw::HeaderBar::new();
        let stack = adw::ViewStack::new();
        stack.set_vexpand(true);
        let switcher = adw::ViewSwitcher::new();
        switcher.set_stack(Some(&stack));
        switcher.set_policy(adw::ViewSwitcherPolicy::Wide);
        header.set_title_widget(Some(&switcher));
        install_main_menu(&window, &header);
        toolbar.add_top_bar(&header);

        let library_navigation = adw::NavigationView::new();
        let store_navigation = adw::NavigationView::new();
        let store_back_button: Rc<RefCell<Option<Button>>> = Rc::new(RefCell::new(None));
        let store_detail_open = Rc::new(RefCell::new(false));
        let (library_root, library_grid, library_count) = build_library_page(
            &window,
            &header,
            &library_navigation,
            &runner_service,
            &stack,
            &store_navigation,
            &store_back_button,
            &store_detail_open,
        );
        let library_page = adw::NavigationPage::new(&library_root, &fl!("nav-games"));
        library_page.set_tag(Some("library-root"));
        library_navigation.push(&library_page);
        super::navigation::restore_header_for_root(&library_navigation, &header, "library-root");
        stack.add_titled(&library_navigation, Some("library"), &fl!("nav-games"));
        stack
            .page(&library_navigation)
            .set_icon_name(Some("applications-games-symbolic"));

        let store_root = build_store_page(
            &window,
            &header,
            &store_navigation,
            &store_back_button,
            &store_detail_open,
        );
        let store_page = adw::NavigationPage::new(&store_root, &fl!("nav-store"));
        store_page.set_tag(Some("store-root"));
        store_navigation.push(&store_page);

        stack.add_titled(&store_navigation, Some("store"), &fl!("nav-store"));
        stack
            .page(&store_navigation)
            .set_icon_name(Some("shopping-cart-symbolic"));

        let runners = build_runners_page(&lists, &show_buttons);
        stack.add_titled(&runners, Some("runners"), &fl!("nav-runners"));
        stack
            .page(&runners)
            .set_icon_name(Some("system-run-symbolic"));

        {
            let library_navigation = library_navigation.clone();
            let library_grid = library_grid.clone();
            let library_count = library_count.clone();
            let parent = window.clone();
            let header_for_library = header.clone();
            let runner_service_for_library = runner_service.clone();
            let store_back_for_stack = store_back_button.clone();
            let store_detail_for_stack = store_detail_open.clone();
            let store_nav_for_stack = store_navigation.clone();
            stack.connect_visible_child_notify(move |stack| {
                if stack.visible_child_name().as_deref() != Some("store") {
                    if let Some(button) = store_back_for_stack.borrow_mut().take() {
                        header_for_library.remove(&button);
                    }
                } else if *store_detail_for_stack.borrow()
                    && store_back_for_stack.borrow().is_none()
                {
                    let back = Button::from_icon_name("go-previous-symbolic");
                    back.set_tooltip_text(Some(&fl!("store-back")));
                    back.set_has_frame(false);
                    let nav = store_nav_for_stack.clone();
                    back.connect_clicked(move |_| {
                        // Popping hides the detail page, which removes this button.
                        nav.pop();
                    });
                    header_for_library.pack_start(&back);
                    *store_back_for_stack.borrow_mut() = Some(back);
                }
                if stack.visible_child_name().as_deref() == Some("library") {
                    library_navigation.pop_to_tag("library-root");
                    populate_library(
                        &library_grid,
                        &library_count,
                        &parent,
                        &header_for_library,
                        &library_navigation,
                        &runner_service_for_library,
                        &stack.clone(),
                        &store_nav_for_stack,
                        &store_back_for_stack,
                        &store_detail_for_stack,
                    );
                }
            });
        }

        for provider in RunnerProvider::ALL {
            let Some(button) = show_buttons.borrow().get(&provider).cloned() else {
                continue;
            };
            let lists = lists.clone();
            let show_buttons = show_buttons.clone();
            let catalogs = catalogs.clone();
            let window = window.clone();
            let service = runner_service.clone();
            let sender = sender.clone();
            button.connect_clicked(move |_| {
                show_all_runners(
                    provider,
                    &lists,
                    &show_buttons,
                    &catalogs,
                    &window,
                    &service,
                    &sender,
                );
            });
        }

        toolbar.set_content(Some(&stack));
        window.set_content(Some(&toolbar));

        super::runner_controller::start_runner_updates(
            &window,
            &runner_service,
            &catalogs,
            &lists,
            &show_buttons,
            receiver,
            sender,
        );

        if let Some(id) = games::last_game_id().filter(|_| preferences.restore_last_game) {
            if let Some(game) = catalog::by_id(&id) {
                if games::is_in_library(game.folder_name) {
                    open_library_game(
                        &window,
                        &header,
                        &library_navigation,
                        *game,
                        &runner_service,
                    );
                }
            }
        }

        for game in catalog::GAMES.iter().copied() {
            if games::is_in_library(game.folder_name) {
                std::thread::spawn(move || {
                    if let Err(error) = backgrounds::update_background(game) {
                        tracing::warn!("Could not update background for {}: {error}", game.id);
                    }
                });
            }
        }

        Self {
            window,
            _runner_service: runner_service,
        }
    }

    pub fn present(&self) {
        self.window.present();
    }
}
