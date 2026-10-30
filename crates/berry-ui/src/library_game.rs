use adw::prelude::*;
use gtk::{glib, Align, Box, Button, Label, Orientation, ProgressBar};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use crate::download_details::open_download_details;
use crate::game_settings::open_game_settings;
use crate::media::{cached_background_view, watch_background_update};
use crate::widgets::{set_game_action_button, symbolic_image};
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

    {
        let progress_box = progress_box.clone();
        let progress_label = progress_label.clone();
        let progress_bar = progress_bar.clone();
        let play_button = play_button.clone();
        let game_id = game.id;
        let path = initial_settings.path.clone();
        let weak_button = play_button.downgrade();
        let mut resumable = downloads::has_incomplete_download(&path);
        let mut was_active = active;
        let mut shown_button = None;
        let mut ticks = 0u8;
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
            if (was_active && !active) || ticks % 4 == 0 {
                resumable = downloads::has_incomplete_download(&path);
            }
            was_active = active;
            let state = downloads::ui_state(game_id);
            let paused = state.map(|state| state.paused).unwrap_or(false);
            if let Some(state) = state {
                progress_bar.set_fraction(state.progress.fraction());
                progress_label.set_text(&progress_text(&state));
                progress_box.set_visible(active || state.paused);
            } else {
                progress_box.set_visible(active);
            }
            // Only touch the button when its state changes.
            let button = if active && !paused {
                Some(true)
            } else if active || resumable {
                Some(false)
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
                shown_button = button;
                if button == Some(true) {
                    set_game_action_button(
                        &play_button,
                        &fl!("game-pause"),
                        "media-playback-pause-symbolic",
                    );
                    play_button.set_tooltip_text(Some(&fl!("game-pause-download")));
                } else {
                    set_game_action_button(
                        &play_button,
                        &fl!("game-resume"),
                        "media-playback-start-symbolic",
                    );
                    play_button.set_tooltip_text(Some(&fl!("game-resume-download")));
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
            if downloads::is_downloading(game.id) {
                if downloads::pause_game(game.id) {
                    set_game_action_button(
                        &button,
                        &fl!("game-resume"),
                        "media-playback-start-symbolic",
                    );
                    button.set_tooltip_text(Some(&fl!("game-resume-download")));
                }
                return;
            }
            let settings = games::load_settings(game.id, games::game_path(game.folder_name));

            if !downloads::is_fully_downloaded(&game, &settings.path) {
                if downloads::is_downloading(game.id) {
                    return;
                }

                let token = Arc::new(AtomicBool::new(false));
                let (tx, rx) = async_channel::unbounded::<InstallEvent>();
                button.set_sensitive(true);
                set_game_action_button(button, &fl!("game-pause"), "media-playback-pause-symbolic");
                button.set_tooltip_text(Some(&fl!("game-pause-download")));
                progress_bar.set_fraction(0.0);
                let status = if downloads::has_incomplete_download(&settings.path) {
                    fl!("game-resuming")
                } else {
                    fl!("game-preparing")
                };
                progress_label.set_text(&status);
                progress_box.set_visible(true);
                update_button_for_download.set_visible(false);
                downloads::install_game(game, settings.path.clone(), token.clone(), tx);

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
                        resume_tooltip: fl!("game-resume-download"),
                        error_message: fl!("game-error-download"),
                        reenable_button: true,
                    },
                );
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
                            adw::AlertDialog::builder()
                                .heading(&fl!("game-error-launch"))
                                .body(error.to_string())
                                .build()
                                .present(Some(&parent));
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
                    reenable_button: false,
                },
            );
        });
    }

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
        let game = game;
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

/// "phase · downloaded / total · speed/s"
fn progress_line(progress: &downloads::DownloadProgress) -> String {
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
    reenable_button: bool,
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
                    if texts.reenable_button {
                        ui.play_button.set_sensitive(true);
                    }
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
                            adw::AlertDialog::builder()
                                .heading(&texts.error_message)
                                .body(error.to_string())
                                .build()
                                .present(Some(&ui.parent));
                        }
                    }
                    break;
                }
            }
        }
    });
}
