use adw::prelude::*;
use gtk::{glib, Align, Box, Button, Label, Orientation, ProgressBar};
use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::download_details::open_download_details;
use crate::game_settings::open_game_settings;
use crate::media::{cached_background_view, watch_background_update};
use crate::widgets::{present_notice, set_game_action_button, symbolic_image};
use berry_downloads::{self as downloads, InstallEvent};
use berry_games::catalog::GameCatalogEntry;
use berry_games::library as games;
use berry_launcher::GameLauncher;
use berry_locale::fl;
use berry_media::backgrounds;
use berry_runners::RunnerService;

pub(crate) enum LaunchEvent {
    Started(Result<u32, String>),
    Exited,
}

pub(crate) fn open_library_game(
    parent: &adw::ApplicationWindow,
    header: &adw::HeaderBar,
    navigation: &adw::NavigationView,
    game: GameCatalogEntry,
    runner_service: &Arc<RunnerService>,
) {
    let _ = games::set_last_game_id(game.id);
    let _ =
        games::ensure_runner_setting(game.id, game.folder_name, runner_service.default_runner());
    header.set_visible(false);

    let overlay = gtk::Overlay::new();
    overlay.set_hexpand(true);
    overlay.set_vexpand(true);

    let background = cached_background_view(game.id);
    background.overlay.add_css_class("berry-library-promo");
    overlay.set_child(Some(&background.overlay));

    let (update_done, update_finished) = async_channel::bounded::<()>(1);
    watch_background_update(&background, game.id, update_finished);
    std::thread::spawn(move || {
        if let Err(error) = backgrounds::update_background(game) {
            tracing::warn!("Could not update background for {}: {error}", game.id);
        }
        let _ = update_done.send_blocking(());
    });

    let initial_settings = games::load_settings(game.id, games::game_path(game.folder_name));
    let complete = downloads::is_fully_downloaded(&game, &initial_settings.path);
    let active = downloads::is_downloading(game.id);
    let resumable = !active && downloads::has_incomplete_download(&initial_settings.path);
    let persisted_download = downloads::ui_state(game.id);

    let progress_box = Box::new(Orientation::Vertical, 4);
    progress_box.set_size_request(340, -1);
    progress_box.set_valign(Align::Center);
    progress_box.add_css_class("berry-library-progress-box");
    progress_box.set_visible(
        active
            || persisted_download
                .map(|state| state.paused)
                .unwrap_or(false),
    );
    let progress_label = Label::new(None);
    progress_label.set_xalign(0.0);
    progress_label.add_css_class("berry-progress-label");
    let progress_bar = ProgressBar::new();
    progress_box.append(&progress_label);
    progress_box.append(&progress_bar);

    // Opens the details window of the running (or paused) operation. It sits to the right of the
    // progress panel and is only shown while the panel is.
    let info_button = Button::from_icon_name("dialog-information-symbolic");
    info_button.set_has_frame(true);
    info_button.add_css_class("circular");
    info_button.add_css_class("berry-library-info-button");
    info_button.set_valign(Align::Center);
    info_button.set_tooltip_text(Some(&fl!("details-info-tooltip")));
    progress_box
        .bind_property("visible", &info_button, "visible")
        .sync_create()
        .build();
    {
        let parent = parent.clone();
        info_button.connect_clicked(move |_| open_download_details(&parent, game));
    }

    let progress_row = Box::new(Orientation::Horizontal, 10);
    progress_row.set_halign(Align::Start);
    progress_row.set_valign(Align::End);
    progress_row.set_margin_start(24);
    progress_row.set_margin_bottom(28);
    progress_row.append(&progress_box);
    progress_row.append(&info_button);
    overlay.add_overlay(&progress_row);

    if let Some(state) = persisted_download {
        progress_bar.set_fraction(state.progress.fraction());
        progress_label.set_text(&progress_text(&state));
    }

    let bottom_bar = Box::new(Orientation::Horizontal, 8);
    bottom_bar.set_halign(Align::End);
    bottom_bar.set_valign(Align::End);
    bottom_bar.set_margin_bottom(28);
    bottom_bar.set_margin_end(28);
    bottom_bar.add_css_class("berry-library-controls");

    let update_button = Button::from_icon_name("folder-download-symbolic");
    update_button.set_has_frame(true);
    update_button.add_css_class("circular");
    update_button.add_css_class("suggested-action");
    update_button.set_size_request(42, 42);
    update_button.set_visible(false);
    update_button.set_tooltip_text(Some(&fl!("game-update")));

    let preload_button = Button::from_icon_name("folder-download-symbolic");
    preload_button.set_has_frame(true);
    preload_button.add_css_class("circular");
    preload_button.add_css_class("suggested-action");
    preload_button.set_size_request(42, 42);
    preload_button.set_visible(false);
    preload_button.set_tooltip_text(Some("Pre-download game update"));

    let (play_label, play_icon) = if complete {
        (fl!("game-play"), "media-playback-start-symbolic")
    } else if active || resumable {
        if active {
            (fl!("game-pause"), "media-playback-pause-symbolic")
        } else {
            (fl!("game-resume"), "media-playback-start-symbolic")
        }
    } else {
        (fl!("game-install"), "folder-download-symbolic")
    };
    let play_button = Button::new();
    set_game_action_button(&play_button, &play_label, play_icon);
    play_button.add_css_class("suggested-action");
    play_button.set_size_request(150, 42);
    play_button.set_tooltip_text(Some(&fl!("game-action-tooltip")));

    let already_running = complete && GameLauncher::is_running(game.id);
    if already_running {
        set_game_action_button(&play_button, &fl!("game-running"), "system-run-symbolic");
        play_button.set_tooltip_text(Some(&fl!("game-running-tooltip")));
        play_button.set_sensitive(false);
    }

    // Version of the pre-download HoYoverse publishes for this game right now (`None`: nothing to
    // pre-download). The preload button is shown only while this holds a version that is neither
    // installed nor already downloaded, so it follows the pre-download's whole life cycle.
    let preload_offer: PreloadOffer = Rc::new(RefCell::new(None));
    let preload_checking = Rc::new(Cell::new(false));
    refresh_preload_offer(
        game,
        initial_settings.path.clone(),
        &preload_offer,
        &preload_checking,
    );

    {
        let progress_box = progress_box.clone();
        let progress_label = progress_label.clone();
        let progress_bar = progress_bar.clone();
        let play_button = play_button.clone();
        let preload_button = preload_button.clone();
        let preload_offer = preload_offer.clone();
        let preload_checking = preload_checking.clone();
        let game_id = game.id;
        let path = initial_settings.path.clone();
        let weak_button = play_button.downgrade();
        let mut resumable = downloads::has_incomplete_download(&path);
        let mut was_active = active;
        let mut shown_button = None;
        let mut ticks = 0u8;
        let mut last_offer_check = Instant::now();
        let mut disk = PreloadDiskState::read(&game, &path);
        glib::timeout_add_local(std::time::Duration::from_millis(250), move || {
            let Some(play_button) = weak_button.upgrade() else {
                return glib::ControlFlow::Break;
            };
            if !play_button.is_mapped() {
                return glib::ControlFlow::Continue;
            }
            let active = downloads::is_downloading(game_id);
            // The incomplete-download markers live on disk: check them about once a second, and
            // right away when an operation has just ended. The cached value is from before the
            // operation removed its markers; trusting it would show "Resume" for a game that
            // has just finished.
            ticks = ticks.wrapping_add(1);
            let just_finished = was_active && !active;
            if just_finished || ticks.is_multiple_of(4) {
                resumable = downloads::has_incomplete_download(&path);
                disk = PreloadDiskState::read(&game, &path);
            }
            if just_finished {
                // The operation changed what is installed or cached: forget the old answer
                // (the button stays hidden meanwhile) and ask again.
                *preload_offer.borrow_mut() = None;
                last_offer_check = Instant::now();
                refresh_preload_offer(game, path.clone(), &preload_offer, &preload_checking);
            } else if !active && last_offer_check.elapsed() >= PRELOAD_RECHECK {
                // A new pre-download can be published while the launcher stays open.
                last_offer_check = Instant::now();
                refresh_preload_offer(game, path.clone(), &preload_offer, &preload_checking);
            }
            was_active = active;
            let state = downloads::ui_state(game_id);
            let paused = state.map(|state| state.paused).unwrap_or(false);
            let paused_preload = state.is_some_and(|state| state.preload);
            let offered = preload_offer.borrow().clone();
            preload_button.set_visible(
                !paused_preload
                    && !active
                    && disk.fully_downloaded
                    && offered
                        .as_deref()
                        .is_some_and(|version| disk.needs_preload(version)),
            );
            if let Some(state) = state {
                progress_bar.set_fraction(state.progress.fraction());
                progress_label.set_text(&progress_text(&state));
                progress_box.set_visible(active || state.paused);
            } else {
                progress_box.set_visible(active);
            }
            // Only touch the button when its state changes.
            let button = if downloads::is_patching(game_id) {
                Some(ActionMode::Applying)
            } else if active && !paused {
                Some(ActionMode::Pause)
            } else if active || resumable || paused {
                Some(ActionMode::Resume)
            } else {
                None
            };
            if button.is_none() {
                // Leaving a download state (pause / resume) with nothing left to resume: the
                // operation finished, so the button goes back to what the game on disk allows.
                // Never touched otherwise: it may be showing "Starting" or "Running".
                if shown_button.take().is_some() {
                    if downloads::is_fully_downloaded(&game, &path) {
                        set_game_action_button(
                            &play_button,
                            &fl!("game-play"),
                            "media-playback-start-symbolic",
                        );
                        play_button.set_tooltip_text(Some(&fl!("game-launch")));
                    } else {
                        set_game_action_button(
                            &play_button,
                            &fl!("game-install"),
                            "folder-download-symbolic",
                        );
                        play_button.set_tooltip_text(Some(&fl!("game-action-tooltip")));
                    }
                    play_button.set_sensitive(true);
                }
            } else if button != shown_button {
                if shown_button == Some(ActionMode::Applying) {
                    // Patching is over (or the update fell back to a download): unlock.
                    play_button.set_sensitive(true);
                }
                shown_button = button;
                match button {
                    Some(ActionMode::Applying) => {
                        // Applying patches is not a download: there is nothing to pause or
                        // resume, so the button shows "play" but stays locked until it ends.
                        set_game_action_button(
                            &play_button,
                            &fl!("game-applying"),
                            "media-playback-start-symbolic",
                        );
                        play_button.set_tooltip_text(Some(&fl!("game-applying-tooltip")));
                        play_button.set_sensitive(false);
                    }
                    Some(ActionMode::Pause) => {
                        set_game_action_button(
                            &play_button,
                            &fl!("game-pause"),
                            "media-playback-pause-symbolic",
                        );
                        play_button.set_tooltip_text(Some(&fl!("game-pause-download")));
                    }
                    Some(ActionMode::Resume) | None => {
                        set_game_action_button(
                            &play_button,
                            &fl!("game-resume"),
                            "media-playback-start-symbolic",
                        );
                        play_button.set_tooltip_text(Some(&fl!("game-resume-download")));
                    }
                }
            }
            glib::ControlFlow::Continue
        });
    }

    {
        let service = runner_service.clone();
        let callback_parent = parent.clone();
        let progress_box = progress_box.clone();
        let progress_label = progress_label.clone();
        let progress_bar = progress_bar.clone();
        let update_button_for_download = update_button.clone();
        play_button.connect_clicked(move |button| {
            if !games::is_in_library(game.folder_name) {
                return;
            }
            if downloads::is_patching(game.id) {
                // Locked while patches are applied; nothing to pause or resume.
                return;
            }
            if downloads::is_downloading(game.id) {
                if downloads::pause_game(game.id) {
                    set_game_action_button(
                        button,
                        &fl!("game-resume"),
                        "media-playback-start-symbolic",
                    );
                    button.set_tooltip_text(Some(&fl!("game-resume-download")));
                }
                return;
            }
            let settings = games::load_settings(game.id, games::game_path(game.folder_name));

            // A paused preload owns the primary action: resume it instead of launching the
            // installed version or starting a normal install/update.
            if downloads::ui_state(game.id).is_some_and(|state| state.preload && state.paused) {
                let token = Arc::new(AtomicBool::new(false));
                let (tx, rx) = async_channel::unbounded::<InstallEvent>();
                set_game_action_button(button, &fl!("game-pause"), "media-playback-pause-symbolic");
                progress_box.set_visible(true);
                downloads::preload_game(game, settings.path, token.clone(), tx);
                watch_operation(
                    rx,
                    token,
                    OperationUi {
                        play_button: button.clone(),
                        progress_box: progress_box.clone(),
                        progress_label: progress_label.clone(),
                        progress_bar: progress_bar.clone(),
                        parent: callback_parent.clone(),
                    },
                    OperationTexts {
                        resume_tooltip: fl!("game-resume-update"),
                        error_message: fl!("game-error-update"),
                    },
                );
                return;
            }

            if !downloads::is_fully_downloaded(&game, &settings.path) {
                if downloads::is_downloading(game.id) {
                    return;
                }

                // Starts (or resumes) the install. `voice_packs` is the choice made in the
                // dialog; `None` keeps whatever was chosen before (or the default) when an
                // interrupted install is resumed.
                let start = {
                    let button = button.clone();
                    let progress_box = progress_box.clone();
                    let progress_label = progress_label.clone();
                    let progress_bar = progress_bar.clone();
                    let update_button = update_button_for_download.clone();
                    let parent = callback_parent.clone();
                    let path = settings.path.clone();
                    move |voice_packs: Option<Vec<String>>| {
                        let token = Arc::new(AtomicBool::new(false));
                        let (tx, rx) = async_channel::unbounded::<InstallEvent>();
                        button.set_sensitive(true);
                        set_game_action_button(
                            &button,
                            &fl!("game-pause"),
                            "media-playback-pause-symbolic",
                        );
                        button.set_tooltip_text(Some(&fl!("game-pause-download")));
                        progress_bar.set_fraction(0.0);
                        let status = if downloads::has_incomplete_download(&path) {
                            fl!("game-resuming")
                        } else {
                            fl!("game-preparing")
                        };
                        progress_label.set_text(&status);
                        progress_box.set_visible(true);
                        update_button.set_visible(false);
                        match voice_packs {
                            Some(packs) => downloads::install_game_with_voice_packs(
                                game,
                                path,
                                &packs,
                                token.clone(),
                                tx,
                            ),
                            None => downloads::install_game(game, path, token.clone(), tx),
                        }

                        watch_operation(
                            rx,
                            token,
                            OperationUi {
                                play_button: button,
                                progress_box,
                                progress_label,
                                progress_bar,
                                parent,
                            },
                            OperationTexts {
                                resume_tooltip: fl!("game-resume-download"),
                                error_message: fl!("game-error-download"),
                            },
                        );
                    }
                };

                // A first install of a game with several voice-over packages asks which ones to
                // download. Resuming an interrupted install keeps the choice made at its start.
                let ask = !game.voice_packs().is_empty()
                    && !downloads::has_incomplete_download(&settings.path);
                if ask {
                    // One prompt at a time: the button comes back if the user backs out.
                    button.set_sensitive(false);
                    let button = button.clone();
                    crate::voice_packs_dialog::choose_voice_packs(
                        &callback_parent,
                        game,
                        move |choice| match choice {
                            Some(packs) => start(Some(packs)),
                            None => button.set_sensitive(true),
                        },
                    );
                } else {
                    start(None);
                }
                return;
            }

            let (tx, rx) = async_channel::unbounded::<LaunchEvent>();
            button.set_sensitive(false);
            button.set_label(&fl!("game-starting"));
            {
                let service = service.clone();
                std::thread::spawn(move || {
                    let exit_tx = tx.clone();
                    let started = GameLauncher::launch(&service, &game, &settings, move || {
                        let _ = exit_tx.send_blocking(LaunchEvent::Exited);
                    });
                    let _ = tx
                        .send_blocking(LaunchEvent::Started(started.map_err(|e| format!("{e:#}"))));
                });
            }
            let (button, parent) = (button.clone(), callback_parent.clone());
            let launcher_preferences = berry_core::settings::load_preferences();
            let launcher_behavior = launcher_preferences.launcher_behavior;
            let pause_downloads = launcher_preferences.pause_downloads_on_launch;
            // A game inside the Flatpak sandbox dies when the launcher process exits, so the
            // application is held (alive, even with no window) until the game ends. The hold
            // starts now, not when the game is reported as started, so closing the window right
            // after pressing "Play" cannot quit the launcher in between.
            let mut keep_alive = parent.application().map(|application| application.hold());
            glib::MainContext::default().spawn_local(async move {
                while let Ok(event) = rx.recv().await {
                    match event {
                        LaunchEvent::Started(Ok(_)) => {
                            if !GameLauncher::needs_launcher_alive(game.id) {
                                keep_alive = None;
                            }
                            if pause_downloads {
                                // Resumable: the game page offers to continue them afterwards.
                                for other in berry_games::catalog::GAMES {
                                    if downloads::is_downloading(other.id) {
                                        downloads::pause_game(other.id);
                                    }
                                }
                            }
                            set_game_action_button(
                                &button,
                                &fl!("game-running"),
                                "emblem-ok-symbolic",
                            );
                            button.set_sensitive(false);
                            match launcher_behavior {
                                berry_core::settings::LauncherBehavior::Close => {
                                    // Keep listening for the exit: the hold (if any) is released then.
                                    crate::window::close_without_confirmation(&parent);
                                }
                                berry_core::settings::LauncherBehavior::Hide => {
                                    parent.set_visible(false);
                                }
                                berry_core::settings::LauncherBehavior::KeepOpen => {}
                            }
                        }
                        LaunchEvent::Started(Err(error)) => {
                            set_game_action_button(
                                &button,
                                &fl!("game-play"),
                                "media-playback-start-symbolic",
                            );
                            button.set_sensitive(true);
                            present_notice(
                                &parent,
                                &fl!("game-error-launch"),
                                Some(&error.to_string()),
                            );
                            break;
                        }
                        LaunchEvent::Exited => {
                            if launcher_behavior == berry_core::settings::LauncherBehavior::Hide {
                                parent.present();
                            }
                            set_game_action_button(
                                &button,
                                &fl!("game-play"),
                                "media-playback-start-symbolic",
                            );
                            button.set_sensitive(true);
                            break;
                        }
                    }
                }
                // Releasing the hold lets the application quit when no window is left.
                drop(keep_alive);
            });
        });
    }

    if complete {
        let (tx, rx) = async_channel::bounded::<Result<bool, String>>(1);
        let game_for_check = game;
        let path_for_check = initial_settings.path.clone();
        std::thread::spawn(move || {
            let result = downloads::has_update(&game_for_check, &path_for_check)
                .map_err(|e| format!("{e:#}"));
            let _ = tx.send_blocking(result);
        });
        let update_button_for_check = update_button.clone();
        glib::MainContext::default().spawn_local(async move {
            if let Ok(Ok(true)) = rx.recv().await {
                update_button_for_check.set_visible(true);
            }
        });
    }

    {
        let callback_parent = parent.clone();
        let progress_box = progress_box.clone();
        let progress_label = progress_label.clone();
        let progress_bar = progress_bar.clone();
        let play_button_for_update = play_button.clone();
        let update_button_for_update = update_button.clone();
        update_button.connect_clicked(move |_| {
            if downloads::is_downloading(game.id) {
                return;
            }
            let settings = games::load_settings(game.id, games::game_path(game.folder_name));
            let token = Arc::new(AtomicBool::new(false));
            let (tx, rx) = async_channel::unbounded::<InstallEvent>();
            update_button_for_update.set_sensitive(true);
            update_button_for_update.set_visible(false);
            set_game_action_button(
                &play_button_for_update,
                &fl!("game-pause"),
                "media-playback-pause-symbolic",
            );
            play_button_for_update.set_tooltip_text(Some(&fl!("game-pause-update")));
            progress_box.set_visible(true);
            progress_label.set_text(&fl!("game-preparing-update"));
            progress_bar.set_fraction(0.0);
            downloads::update_game(game, settings.path, token.clone(), tx);
            watch_operation(
                rx,
                token,
                OperationUi {
                    play_button: play_button_for_update.clone(),
                    progress_box: progress_box.clone(),
                    progress_label: progress_label.clone(),
                    progress_bar: progress_bar.clone(),
                    parent: callback_parent.clone(),
                },
                OperationTexts {
                    resume_tooltip: fl!("game-resume-update"),
                    error_message: fl!("game-error-update"),
                },
            );
        });
    }

    {
        let callback_parent = parent.clone();
        let progress_box = progress_box.clone();
        let progress_label = progress_label.clone();
        let progress_bar = progress_bar.clone();
        let play_button_for_preload = play_button.clone();
        preload_button.connect_clicked(move |_| {
            if downloads::is_downloading(game.id) {
                return;
            }
            let settings = games::load_settings(game.id, games::game_path(game.folder_name));
            let token = Arc::new(AtomicBool::new(false));
            let (tx, rx) = async_channel::unbounded::<InstallEvent>();
            set_game_action_button(
                &play_button_for_preload,
                &fl!("game-pause"),
                "media-playback-pause-symbolic",
            );
            play_button_for_preload.set_tooltip_text(Some(&fl!("game-pause-update")));
            progress_box.set_visible(true);
            progress_label.set_text("Preparing pre-download");
            progress_bar.set_fraction(0.0);
            downloads::preload_game(game, settings.path, token.clone(), tx);
            watch_operation(
                rx,
                token,
                OperationUi {
                    play_button: play_button_for_preload.clone(),
                    progress_box: progress_box.clone(),
                    progress_label: progress_label.clone(),
                    progress_bar: progress_bar.clone(),
                    parent: callback_parent.clone(),
                },
                OperationTexts {
                    resume_tooltip: fl!("game-resume-update"),
                    error_message: fl!("game-error-update"),
                },
            );
        });
    }

    bottom_bar.append(&preload_button);
    bottom_bar.append(&update_button);
    bottom_bar.append(&play_button);

    let settings_button = Button::new();
    settings_button.set_child(Some(&symbolic_image("emblem-system-symbolic")));
    settings_button.set_has_frame(true);
    settings_button.add_css_class("circular");
    settings_button.add_css_class("suggested-action");
    settings_button.set_size_request(42, 42);
    settings_button.set_tooltip_text(Some(&fl!("game-settings-tooltip")));
    {
        let parent = parent.clone();
        settings_button.connect_clicked(move |_| open_game_settings(&parent, game));
    }
    bottom_bar.append(&settings_button);
    overlay.add_overlay(&bottom_bar);

    let back_button = Button::from_icon_name("go-previous-symbolic");
    back_button.set_has_frame(true);
    back_button.add_css_class("circular");
    back_button.add_css_class("berry-library-back-button");
    back_button.set_halign(Align::Start);
    back_button.set_valign(Align::Start);
    back_button.set_margin_top(24);
    back_button.set_margin_start(24);
    back_button.set_tooltip_text(Some(&fl!("library-back")));
    {
        let navigation = navigation.clone();
        let header = header.clone();
        back_button.connect_clicked(move |_| {
            header.set_visible(true);
            navigation.pop();
        });
    }
    overlay.add_overlay(&back_button);

    let close_button = Button::from_icon_name("window-close-symbolic");
    close_button.set_has_frame(true);
    close_button.add_css_class("circular");
    close_button.add_css_class("berry-library-close-button");
    close_button.set_halign(Align::End);
    close_button.set_valign(Align::Start);
    close_button.set_margin_top(24);
    close_button.set_margin_end(24);
    close_button.set_tooltip_text(Some(&fl!("menu-close-launcher")));
    {
        let parent = parent.clone();
        close_button.connect_clicked(move |_| parent.close());
    }
    overlay.add_overlay(&close_button);

    let page = adw::NavigationPage::new(&overlay, &game.localized_title());
    page.set_tag(Some(game.id));
    navigation.push(&page);
}

/// How often an open library page asks HoYoverse again whether a pre-download was published.
const PRELOAD_RECHECK: Duration = Duration::from_secs(30 * 60);

/// See `open_library_game`: the published pre-download's version, if any.
type PreloadOffer = Rc<RefCell<Option<String>>>;

/// What the game directory says about pre-downloads (cheap, read about once a second).
struct PreloadDiskState {
    fully_downloaded: bool,
    installed: Option<String>,
    cached: Option<String>,
}

impl PreloadDiskState {
    fn read(game: &GameCatalogEntry, path: &std::path::Path) -> Self {
        Self {
            fully_downloaded: downloads::is_fully_downloaded(game, path),
            installed: downloads::installed_version(path),
            cached: downloads::cached_preload_version(path),
        }
    }

    /// The offered pre-download is still worth downloading: not installed, not downloaded yet.
    fn needs_preload(&self, offered: &str) -> bool {
        self.installed.as_deref() != Some(offered) && self.cached.as_deref() != Some(offered)
    }
}

/// Ask (off the UI thread) which pre-download is published and store the answer. A failed lookup
/// keeps the previous answer; only one lookup runs at a time.
fn refresh_preload_offer(
    game: GameCatalogEntry,
    path: PathBuf,
    offer: &PreloadOffer,
    checking: &Rc<Cell<bool>>,
) {
    if checking.replace(true) {
        return;
    }
    let (tx, rx) = async_channel::bounded::<Result<Option<String>, String>>(1);
    std::thread::spawn(move || {
        let result = downloads::available_preload(&game, &path).map_err(|e| format!("{e:#}"));
        let _ = tx.send_blocking(result);
    });
    let (offer, checking) = (offer.clone(), checking.clone());
    glib::MainContext::default().spawn_local(async move {
        match rx.recv().await {
            Ok(Ok(version)) => *offer.borrow_mut() = version,
            Ok(Err(error)) => {
                tracing::debug!("pre-download lookup for {} failed: {error}", game.id)
            }
            Err(_) => {}
        }
        checking.set(false);
    });
}

/// "phase · downloaded / total · speed/s", or while patching
/// "Applying patches 668/1177 · done / total" (no network speed: nothing is downloaded).
fn progress_line(progress: &downloads::DownloadProgress) -> String {
    if progress.is_patching() {
        let counter = if progress.patch_total > 0 {
            format!(" {}/{}", progress.patch_index, progress.patch_total)
        } else {
            String::new()
        };
        return format!(
            "{}{counter} · {} / {}",
            progress.phase_label(),
            downloads::format_bytes(progress.downloaded),
            downloads::format_bytes(progress.total),
        );
    }
    format!(
        "{} · {} / {} · {}/s",
        progress.phase_label(),
        downloads::format_bytes(progress.downloaded),
        downloads::format_bytes(progress.total),
        downloads::format_bytes(progress.net_speed),
    )
}

pub(crate) fn progress_text(state: &downloads::DownloadUiState) -> String {
    if state.paused {
        format!(
            "{} · {} / {}",
            fl!("progress-paused"),
            downloads::format_bytes(state.progress.downloaded),
            downloads::format_bytes(state.progress.total),
        )
    } else {
        progress_line(&state.progress)
    }
}

/// What the main action button is doing while an operation is on screen.
#[derive(Clone, Copy, PartialEq, Eq)]
enum ActionMode {
    /// Downloading: the button pauses.
    Pause,
    /// Paused or interrupted: the button resumes.
    Resume,
    /// Applying patches: "play", but locked.
    Applying,
}

// Widgets an install or update drives while it runs.
struct OperationUi {
    play_button: Button,
    progress_box: Box,
    progress_label: Label,
    progress_bar: ProgressBar,
    parent: adw::ApplicationWindow,
}

// What differs between an install and an update in the UI.
struct OperationTexts {
    resume_tooltip: String,
    error_message: String,
}

// Shows the progress of an install or update and restores the play button when it ends.
fn watch_operation(
    rx: async_channel::Receiver<InstallEvent>,
    token: Arc<AtomicBool>,
    ui: OperationUi,
    texts: OperationTexts,
) {
    glib::MainContext::default().spawn_local(async move {
        while let Ok(event) = rx.recv().await {
            match event {
                InstallEvent::Status(text) => ui.progress_label.set_text(&text),
                InstallEvent::Progress(progress) => {
                    ui.progress_bar.set_fraction(progress.fraction());
                    ui.progress_label.set_text(&progress_line(&progress));
                }
                InstallEvent::Finished(result) => {
                    // Also undoes the lock the button had while patches were applied.
                    ui.play_button.set_sensitive(true);
                    match result {
                        Ok(()) => {
                            ui.progress_box.set_visible(false);
                            set_game_action_button(
                                &ui.play_button,
                                &fl!("game-play"),
                                "media-playback-start-symbolic",
                            );
                            ui.play_button.set_tooltip_text(Some(&fl!("game-launch")));
                        }
                        // Paused by the user: the partial files stay for a later resume.
                        Err(_) if token.load(Ordering::SeqCst) => {
                            set_game_action_button(
                                &ui.play_button,
                                &fl!("game-resume"),
                                "media-playback-start-symbolic",
                            );
                            ui.play_button.set_tooltip_text(Some(&texts.resume_tooltip));
                        }
                        Err(error) => {
                            set_game_action_button(
                                &ui.play_button,
                                &fl!("game-resume"),
                                "media-playback-start-symbolic",
                            );
                            ui.play_button.set_tooltip_text(Some(&texts.resume_tooltip));
                            present_notice(
                                &ui.parent,
                                &texts.error_message,
                                Some(&error.to_string()),
                            );
                        }
                    }
                    break;
                }
            }
        }
    });
}
