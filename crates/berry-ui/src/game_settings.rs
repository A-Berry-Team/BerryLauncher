use adw::prelude::*;
use gtk::{gio, glib, Align, Box, Button, Entry, FileDialog, Orientation, Switch};
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use crate::fps_settings::FpsControls;
use crate::labels::Labeled;
use crate::widgets::{selected_string, suffix_row};
use berry_downloads::{self as downloads, InstallEvent};
use berry_games::catalog::GameCatalogEntry;
use berry_games::library as games;
use berry_launcher::GameLauncher;
use berry_locale::fl;
use berry_runners::RunnerProvider;
use berry_runners::RunnerService;

pub(crate) fn open_game_settings(parent: &adw::ApplicationWindow, game: GameCatalogEntry) {
    let service = match RunnerService::new() {
        Ok(service) => service,
        Err(error) => {
            let dialog =
                adw::AlertDialog::new(Some(&fl!("game-error-runners")), Some(&error.to_string()));
            dialog.present(Some(parent));
            return;
        }
    };
    let default_path = games::game_path(game.folder_name);
    let mut settings = games::load_settings(game.id, default_path.clone());
    if settings.runner.as_deref().unwrap_or("").is_empty()
        || service
            .proton_path(settings.runner.as_deref().unwrap_or(""))
            .is_none()
    {
        if let Some(default_runner) = service.default_runner() {
            settings.runner = Some(default_runner);
        }
        let _ = games::save_settings(game.id, &settings);
    }

    let window = adw::Dialog::builder()
        .title(fl!("settings-title"))
        .content_width(crate::layout::PADDED_WINDOW_WIDTH)
        .content_height(crate::layout::SETTINGS_WINDOW_HEIGHT)
        .build();
    let toolbar = adw::ToolbarView::new();
    let header = adw::HeaderBar::new();
    toolbar.add_top_bar(&header);

    // Settings are split into pages so the Runner page only holds what concerns the runner.
    let group = |title: String| {
        let group = adw::PreferencesGroup::new();
        group.set_title(&title);
        group
    };
    let performance = group(fl!("settings-group-performance"));
    let fps_group = group(fl!("settings-group-fps"));
    let graphics = group(fl!("settings-group-graphics"));
    let upscaling = group(fl!("settings-group-upscaling"));
    let gamescope_group = group(fl!("settings-group-gamescope"));
    let sync_group = group(fl!("settings-group-sync"));
    sync_group.set_description(Some(&if GameLauncher::ntsync_available() {
        fl!("settings-sync-detected")
    } else {
        fl!("settings-sync-not-detected")
    }));
    let gamemode = Switch::new();
    let fps_unlocker = Switch::new();
    gamemode.set_active(settings.gamemode);
    fps_unlocker.set_active(settings.fps_unlocker);
    let gm_row = suffix_row(
        &fl!("settings-gamemode-title"),
        Some(&fl!("settings-gamemode-subtitle")),
        &gamemode,
    );
    let dxvk_hud_values = berry_games::library::DxvkHud::options()
        .map(|value| value.label())
        .collect::<Vec<_>>();
    let dxvk_hud_refs = dxvk_hud_values
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>();
    let dxvk_hud_dropdown = gtk::DropDown::from_strings(&dxvk_hud_refs);
    dxvk_hud_dropdown.set_selected(settings.dxvk_hud.index() as u32);
    dxvk_hud_dropdown.set_size_request(220, -1);
    let dxvk_hud_row = suffix_row(
        &fl!("settings-dxvk-hud-title"),
        Some(&fl!("settings-dxvk-hud-subtitle")),
        &dxvk_hud_dropdown,
    );
    let fps_row = suffix_row(
        &fl!("settings-fps-title"),
        Some(&fl!("settings-fps-subtitle")),
        &fps_unlocker,
    );
    performance.add(&gm_row);

    // FSR: on by default; the quality preset only matters while it is enabled.
    let fsr_switch = Switch::new();
    fsr_switch.set_active(settings.fsr);
    let fsr_row = suffix_row(
        &fl!("settings-fsr-title"),
        Some(&fl!("settings-fsr-subtitle")),
        &fsr_switch,
    );
    let fsr_labels = berry_games::library::FsrQuality::options()
        .map(|v| v.label())
        .collect::<Vec<_>>();
    let fsr_refs = fsr_labels.iter().map(String::as_str).collect::<Vec<_>>();
    let fsr_quality_dropdown = gtk::DropDown::from_strings(&fsr_refs);
    fsr_quality_dropdown.set_selected(settings.fsr_quality.index() as u32);
    fsr_quality_dropdown.set_size_request(220, -1);
    let fsr_quality_row = suffix_row(
        &fl!("settings-fsr-quality-title"),
        Some(&fl!("settings-fsr-quality-subtitle")),
        &fsr_quality_dropdown,
    );
    fsr_switch
        .bind_property("active", &fsr_quality_row, "sensitive")
        .sync_create()
        .build();
    upscaling.add(&fsr_row);
    upscaling.add(&fsr_quality_row);

    // "Run in the Flatpak sandbox": Flatpak only, on by default. Turning it off starts the game
    // on the host (`flatpak-spawn --host`), which is what `BERRY_LAUNCH_ON_HOST=1` does too. A
    // native package (rpm, deb...) already runs with the user's own permissions, so there the
    // option means nothing: the row is hidden and the stored value is ignored. Where the game
    // runs decides where gamescope has to come from, so the gamescope row below follows it.
    let host_supported = berry_core::settings::host_launch_supported();
    // `BERRY_LAUNCH_ON_HOST=1` in the environment still forces the host (stored choice aside).
    let host_override = berry_core::settings::launch_on_host_override(&settings.environment);
    let host_forced = host_supported && host_override.is_some();
    let sandbox_switch = Switch::new();
    sandbox_switch.set_active(!host_override.unwrap_or(settings.launch_on_host));
    sandbox_switch.set_sensitive(!host_forced);
    let host_row = suffix_row(
        &fl!("settings-host-title"),
        Some(&if host_forced {
            fl!(
                "settings-host-forced",
                value = if host_override.unwrap_or(false) {
                    "1"
                } else {
                    "0"
                }
            )
        } else {
            fl!("settings-host-subtitle")
        }),
        &sandbox_switch,
    );
    host_row.set_visible(host_supported);
    // Whether the game would be started on the host, given the state of the sandbox switch.
    let effective_host = move |sandbox: bool| host_supported && !sandbox;

    // Gamescope: usable when it exists where the game runs. Natively and on the host that is
    // the system's gamescope; in the Flatpak sandbox only the
    // org.freedesktop.Platform.VulkanLayer.gamescope extension counts.
    let gamescope_state = Rc::new(RefCell::new(settings.gamescope.clone()));
    // What the user asked for; kept even while gamescope is unavailable for the chosen target.
    let gamescope_wanted = Rc::new(Cell::new(settings.gamescope.enabled));
    let gamescope_available = Rc::new(Cell::new(false));
    let gamescope_switch = Switch::new();
    let gamescope_button = Button::from_icon_name("emblem-system-symbolic");
    gamescope_button.add_css_class("flat");
    gamescope_button.set_valign(Align::Center);
    gamescope_button.set_tooltip_text(Some(&fl!("settings-gamescope-configure")));
    {
        let parent_window = window.downgrade();
        let state = gamescope_state.clone();
        gamescope_button.connect_clicked(move |_| {
            if let Some(parent_window) = parent_window.upgrade() {
                crate::gamescope_settings::open_gamescope_settings(&parent_window, state.clone());
            }
        });
    }
    // The settings button sits to the left of the toggle. Inside a Box the switch is no longer
    // centered by `suffix_row`, so it has to be centered here or GTK stretches it vertically.
    gamescope_switch.set_valign(Align::Center);
    gamescope_switch.set_hexpand(false);
    let gamescope_controls = Box::new(Orientation::Horizontal, 6);
    gamescope_controls.append(&gamescope_button);
    gamescope_controls.append(&gamescope_switch);
    let gamescope_row = suffix_row(
        &fl!("settings-gamescope-title"),
        Some(&fl!("settings-gamescope-subtitle")),
        &gamescope_controls,
    );
    gamescope_group.add(&gamescope_row);
    {
        let available = gamescope_available.clone();
        let wanted = gamescope_wanted.clone();
        gamescope_switch.connect_active_notify(move |switch| {
            // Only a real choice counts: refreshing an unavailable row switches it off silently.
            if available.get() {
                wanted.set(switch.is_active());
            }
        });
    }
    let refresh_gamescope = {
        let available = gamescope_available.clone();
        let wanted = gamescope_wanted.clone();
        let switch = gamescope_switch.clone();
        let button = gamescope_button.clone();
        let row = gamescope_row.clone();
        Rc::new(move |on_host: bool| {
            let found = GameLauncher::gamescope_available_for(on_host);
            available.set(found);
            switch.set_sensitive(found);
            button.set_sensitive(found);
            switch.set_active(found && wanted.get());
            row.set_subtitle(&if found {
                fl!("settings-gamescope-subtitle")
            } else if !host_supported {
                fl!("settings-gamescope-missing-native")
            } else if on_host {
                fl!("settings-gamescope-missing-host")
            } else {
                fl!("settings-gamescope-missing-sandbox")
            });
        })
    };
    (*refresh_gamescope)(effective_host(sandbox_switch.is_active()));
    {
        let refresh = refresh_gamescope.clone();
        sandbox_switch
            .connect_active_notify(move |switch| (*refresh)(effective_host(switch.is_active())));
    }

    let sync_labels = berry_games::library::SyncMode::options()
        .map(|v| v.label())
        .collect::<Vec<_>>();
    let sync_refs = sync_labels.iter().map(String::as_str).collect::<Vec<_>>();
    let sync_dropdown = gtk::DropDown::from_strings(&sync_refs);
    sync_dropdown.set_selected(settings.sync.index() as u32);
    sync_dropdown.set_size_request(220, -1);
    sync_group.add(&suffix_row(
        &fl!("settings-sync-title"),
        Some(&fl!("settings-sync-subtitle")),
        &sync_dropdown,
    ));

    // Wuthering Waves is the only game that ships two selectable graphics APIs on PC.
    let graphics_api_dropdown = if game.id == "wuthering-waves" {
        let labels = berry_games::library::GraphicsApi::options()
            .map(|value| value.label())
            .collect::<Vec<_>>();
        let refs = labels.iter().map(String::as_str).collect::<Vec<_>>();
        let dropdown = gtk::DropDown::from_strings(&refs);
        dropdown.set_selected(settings.graphics_api.index() as u32);
        dropdown.set_size_request(220, -1);
        graphics.add(&suffix_row(
            &fl!("settings-graphics-api-title"),
            Some(&fl!("settings-graphics-api-subtitle")),
            &dropdown,
        ));
        Some(dropdown)
    } else {
        None
    };
    graphics.add(&dxvk_hud_row);
    if let Some(dropdown) = &graphics_api_dropdown {
        let hud_dropdown = dxvk_hud_dropdown.clone();
        let hud_row = dxvk_hud_row.clone();
        let normal_subtitle = fl!("settings-dxvk-hud-subtitle");
        let dx12_hint = fl!("settings-dxvk-hud-dx12-hint");
        let refresh = move |selected: u32| {
            let dx12 = berry_games::library::GraphicsApi::from_index(selected as usize)
                == berry_games::library::GraphicsApi::Dx12;
            hud_dropdown.set_sensitive(!dx12);
            hud_row.set_subtitle(if dx12 { &dx12_hint } else { &normal_subtitle });
        };
        refresh(dropdown.selected());
        let refresh = Rc::new(refresh);
        let refresh_for_signal = refresh.clone();
        dropdown.connect_selected_notify(move |dropdown| refresh_for_signal(dropdown.selected()));
    }
    // Games the unlocker cannot do anything for do not get the group at all.
    let fps_controls = game.fps_unlock().map(|support| {
        fps_group.add(&fps_row);
        FpsControls::build(support, &settings, &fps_group)
    });

    let compatibility = adw::PreferencesGroup::new();
    compatibility.set_title(&fl!("settings-compatibility"));
    let runner_names = service.default_runner().into_iter().collect::<Vec<_>>();
    let mut installed_names = Vec::new();
    for provider in RunnerProvider::ALL {
        installed_names.extend(service.installed_for(provider));
    }
    installed_names.sort();
    installed_names.dedup();
    if installed_names.is_empty() {
        installed_names.extend(runner_names);
    }
    let runner_refs = installed_names
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>();
    let runner_dropdown = gtk::DropDown::from_strings(&runner_refs);
    runner_dropdown.set_size_request(220, -1);
    if let Some(selected) = settings.runner.as_deref() {
        if let Some(index) = installed_names.iter().position(|v| v == selected) {
            runner_dropdown.set_selected(index as u32);
        }
    }
    compatibility.add(&suffix_row(&fl!("settings-runner"), None, &runner_dropdown));
    let remake = Button::with_label(&fl!("settings-remake-prefix-button"));
    compatibility.add(&suffix_row(
        &fl!("settings-remake-prefix-title"),
        Some(&fl!("settings-remake-prefix-subtitle")),
        &remake,
    ));
    {
        let game_id = game.id.to_string();
        let runner_dropdown = runner_dropdown.clone();
        remake.connect_clicked(move |button| {
            if let Some(runner) = selected_string(&runner_dropdown) {
                // A prefix holds a whole Windows tree: delete it off the UI thread, and keep the
                // button disabled until it is gone so a launch cannot start in a half-deleted one.
                let prefix = GameLauncher::compat_dir(&game_id, &runner);
                let (sender, receiver) = async_channel::bounded(1);
                button.set_sensitive(false);
                std::thread::spawn(move || {
                    let _ = std::fs::remove_dir_all(prefix);
                    let _ = sender.send_blocking(());
                });
                let button = button.clone();
                glib::MainContext::default().spawn_local(async move {
                    let _ = receiver.recv().await;
                    button.set_sensitive(true);
                });
            }
        });
    }

    let installation = adw::PreferencesGroup::new();
    installation.set_title(&fl!("settings-installation"));
    let path_entry = Entry::new();
    path_entry.set_text(&settings.path.to_string_lossy());
    path_entry.set_editable(false);
    path_entry.set_width_chars(30);
    path_entry.set_max_width_chars(30);
    let browse = Button::from_icon_name("folder-open-symbolic");
    browse.set_valign(Align::Center);
    browse.set_tooltip_text(Some(&fl!("settings-location-choose")));
    let path_box = Box::new(Orientation::Horizontal, 6);
    path_box.append(&path_entry);
    path_box.append(&browse);
    installation.add(&suffix_row(
        &fl!("settings-location-title"),
        None,
        &path_box,
    ));

    let open_game_folder = Button::with_label(&fl!("settings-open-button"));
    installation.add(&suffix_row(
        &fl!("settings-open-game-folder-title"),
        Some(&fl!("settings-open-game-folder-subtitle")),
        &open_game_folder,
    ));
    {
        let window = window.clone();
        let path_entry = path_entry.clone();
        open_game_folder.connect_clicked(move |_| {
            open_folder(
                &window,
                std::path::PathBuf::from(path_entry.text().as_str()),
            );
        });
    }

    let repair_button = Button::with_label(&fl!("settings-repair-title"));
    repair_button.set_icon_name("view-refresh-symbolic");
    repair_button.set_sensitive(downloads::is_fully_downloaded(&game, &settings.path));
    installation.add(&suffix_row(
        &fl!("settings-repair-title"),
        Some(&fl!("settings-repair-subtitle")),
        &repair_button,
    ));
    {
        let parent = parent.clone();
        let game_for_repair = game;
        let repair_button = repair_button.clone();
        repair_button.clone().connect_clicked(move |_| {
            if downloads::is_downloading(game_for_repair.id) {
                return;
            }
            let settings = games::load_settings(
                game_for_repair.id,
                games::game_path(game_for_repair.folder_name),
            );
            let token = Arc::new(AtomicBool::new(false));
            let (tx, rx) = async_channel::unbounded::<InstallEvent>();
            repair_button.set_sensitive(false);
            repair_button.set_label(&fl!("settings-repair-running"));
            downloads::repair_game(game_for_repair, settings.path, token, tx);
            let button = repair_button.clone();
            let parent = parent.clone();
            glib::MainContext::default().spawn_local(async move {
                while let Ok(event) = rx.recv().await {
                    if let InstallEvent::Finished(result) = event {
                        button.set_sensitive(true);
                        button.set_label(&fl!("settings-repair-title"));
                        match result {
                            Ok(()) => adw::AlertDialog::builder()
                                .heading(&fl!("settings-repair-done"))
                                .body(&fl!("settings-repair-done-detail"))
                                .build()
                                .present(Some(&parent)),
                            Err(error) => adw::AlertDialog::builder()
                                .heading(&fl!("settings-repair-failed"))
                                .body(error.to_string())
                                .build()
                                .present(Some(&parent)),
                        }
                        break;
                    }
                }
            });
        });
    }
    {
        let path_entry = path_entry.clone();
        let parent = window.clone();
        browse.connect_clicked(move |_| {
            let dialog = FileDialog::builder()
                .title(fl!("settings-location-dialog"))
                .accept_label("Select")
                .build();
            let entry = path_entry.clone();
            // AdwDialog is a widget inside its parent window, not a GtkWindow itself.
            let root = parent.root();
            let parent_window = root
                .as_ref()
                .and_then(|root| root.downcast_ref::<gtk::Window>());
            dialog.select_folder(parent_window, None::<&gio::Cancellable>, move |result| {
                if let Ok(file) = result {
                    if let Some(path) = file.path() {
                        entry.set_text(&path.to_string_lossy());
                    }
                }
            });
        });
    }

    // Launch: what is handed to the game and to the system when it starts.
    let launch = adw::PreferencesGroup::new();
    launch.set_title(&fl!("settings-launch-group"));
    launch.add(&host_row);
    let launch_args_entry = Entry::new();
    launch_args_entry.set_text(&settings.launch_args);
    launch_args_entry.set_placeholder_text(Some(&fl!("settings-launch-args-placeholder")));
    launch_args_entry.set_width_chars(26);
    launch.add(&suffix_row(
        &fl!("settings-launch-args-title"),
        Some(&fl!("settings-launch-args-subtitle")),
        &launch_args_entry,
    ));
    let wrapper_entry = Entry::new();
    wrapper_entry.set_text(&settings.wrapper);
    wrapper_entry.set_placeholder_text(Some(&fl!("settings-wrapper-placeholder")));
    wrapper_entry.set_width_chars(26);
    launch.add(&suffix_row(
        &fl!("settings-wrapper-title"),
        Some(&fl!("settings-wrapper-subtitle")),
        &wrapper_entry,
    ));
    let dedicated_gpu = Switch::new();
    dedicated_gpu.set_active(settings.dedicated_gpu);
    launch.add(&suffix_row(
        &fl!("settings-dedicated-gpu-title"),
        Some(&fl!("settings-dedicated-gpu-subtitle")),
        &dedicated_gpu,
    ));

    // Maintenance: folders, caches, logs and a way back to the defaults.
    let maintenance = adw::PreferencesGroup::new();
    maintenance.set_title(&fl!("settings-maintenance-group"));
    let selected_runner = {
        let runner_dropdown = runner_dropdown.clone();
        move || selected_string(&runner_dropdown)
    };

    let open_prefix = Button::with_label(&fl!("settings-open-button"));
    maintenance.add(&suffix_row(
        &fl!("settings-open-prefix-title"),
        Some(&fl!("settings-open-prefix-subtitle")),
        &open_prefix,
    ));
    {
        let window = window.clone();
        let game_id = game.id.to_string();
        let selected_runner = selected_runner.clone();
        open_prefix.connect_clicked(move |_| {
            let Some(runner) = selected_runner() else {
                return;
            };
            let prefix = GameLauncher::compat_dir(&game_id, &runner);
            if prefix.is_dir() {
                open_folder(&window, prefix);
            } else {
                adw::AlertDialog::builder()
                    .heading(&fl!("settings-prefix-missing"))
                    .build()
                    .present(Some(&window));
            }
        });
    }

    let open_logs = Button::with_label(&fl!("settings-open-button"));
    maintenance.add(&suffix_row(
        &fl!("settings-open-logs-title"),
        Some(&fl!("settings-open-logs-subtitle")),
        &open_logs,
    ));
    {
        let window = window.clone();
        open_logs.connect_clicked(move |_| {
            let dir = berry_core::settings::logs_dir();
            let _ = std::fs::create_dir_all(&dir);
            open_folder(&window, dir);
        });
    }

    let clear_shaders = Button::with_label(&fl!("settings-shader-cache-button"));
    maintenance.add(&suffix_row(
        &fl!("settings-shader-cache-title"),
        Some(&fl!("settings-shader-cache-subtitle")),
        &clear_shaders,
    ));
    {
        let window = window.clone();
        let game_id = game.id.to_string();
        let selected_runner = selected_runner.clone();
        clear_shaders.connect_clicked(move |button| {
            let Some(runner) = selected_runner() else {
                return;
            };
            let cache = GameLauncher::compat_dir(&game_id, &runner).join("shadercache");
            // A shader cache can hold thousands of files: measure and delete it off the UI thread.
            let (sender, receiver) = async_channel::bounded(1);
            button.set_sensitive(false);
            std::thread::spawn(move || {
                let freed = dir_size(&cache);
                let result = if cache.exists() {
                    std::fs::remove_dir_all(&cache)
                } else {
                    Ok(())
                };
                let _ = sender.send_blocking((freed, result.map_err(|error| error.to_string())));
            });
            let (window, button) = (window.clone(), button.clone());
            glib::MainContext::default().spawn_local(async move {
                let outcome = receiver.recv().await;
                button.set_sensitive(true);
                let Ok((freed, result)) = outcome else {
                    return;
                };
                let dialog = adw::AlertDialog::builder();
                match result {
                    Ok(()) => dialog
                        .heading(&fl!("settings-shader-cache-done"))
                        .body(&fl!(
                            "settings-shader-cache-done-detail",
                            size = format_size(freed)
                        ))
                        .build()
                        .present(Some(&window)),
                    Err(error) => dialog
                        .heading(&fl!("settings-shader-cache-failed"))
                        .body(error.to_string())
                        .build()
                        .present(Some(&window)),
                }
            });
        });
    }

    let proton_log = Switch::new();
    proton_log.set_active(settings.proton_log);
    maintenance.add(&suffix_row(
        &fl!("settings-proton-log-title"),
        Some(&fl!("settings-proton-log-subtitle")),
        &proton_log,
    ));

    // Reset: the close handler below normally saves everything, so a reset must switch it off.
    let skip_save = Rc::new(Cell::new(false));
    let reset_button = Button::with_label(&fl!("settings-reset-button"));
    reset_button.add_css_class("destructive-action");
    maintenance.add(&suffix_row(
        &fl!("settings-reset-title"),
        Some(&fl!("settings-reset-subtitle")),
        &reset_button,
    ));
    {
        let window = window.clone();
        let game_id = game.id.to_string();
        let skip_save = skip_save.clone();
        let path_entry = path_entry.clone();
        let selected_runner = selected_runner.clone();
        reset_button.connect_clicked(move |_| {
            let dialog = adw::AlertDialog::builder()
                .heading(&fl!("settings-reset-confirm"))
                .body(&fl!("settings-reset-confirm-detail"))
                .build();
            let (cancel, reset) = (fl!("settings-reset-cancel"), fl!("settings-reset-button"));
            dialog.add_response("cancel", &cancel);
            dialog.add_response("reset", &reset);
            dialog.set_close_response("cancel");
            dialog.set_default_response(Some("cancel"));
            let window = window.clone();
            let game_id = game_id.clone();
            let skip_save = skip_save.clone();
            // The installation folder and the runner are not "preferences": losing them would
            // leave the game unplayable, so they survive the reset.
            let keep_path = std::path::PathBuf::from(path_entry.text().as_str());
            let keep_runner = selected_runner();
            let window_to_close = window.clone();
            dialog.choose(Some(&window), None::<&gio::Cancellable>, move |choice| {
                if choice.as_str() != "reset" {
                    return;
                }
                let mut fresh = games::GameSettings::default();
                fresh.path = keep_path;
                fresh.runner = keep_runner;
                if let Err(error) = games::save_settings(&game_id, &fresh) {
                    tracing::warn!("Could not reset settings: {error}");
                    return;
                }
                skip_save.set(true);
                window_to_close.close();
            });
        });
    }

    let environment = adw::PreferencesGroup::new();
    environment.set_title(&fl!("settings-env-title"));
    let env_rows: Rc<RefCell<Vec<(Entry, Entry, adw::PreferencesRow)>>> =
        Rc::new(RefCell::new(Vec::new()));
    let add_environment = Button::with_label(&fl!("common-add"));
    environment.add(&suffix_row(
        &fl!("settings-env-add"),
        None,
        &add_environment,
    ));
    for (name, value) in &settings.environment {
        add_environment_row(&environment, &env_rows, name, value);
    }
    {
        let environment = environment.clone();
        let env_rows = env_rows.clone();
        add_environment
            .connect_clicked(move |_| add_environment_row(&environment, &env_rows, "", ""));
    }

    let stack = adw::ViewStack::new();
    let add_page = |name: &str, title: String, icon: &str, groups: &[&adw::PreferencesGroup]| {
        let page = adw::PreferencesPage::new();
        for group in groups {
            page.add(*group);
        }
        stack.add_titled_with_icon(&page, Some(name), &title, icon);
    };
    add_page(
        "general",
        fl!("settings-page-general"),
        "folder-symbolic",
        &[&installation, &launch, &maintenance],
    );
    add_page(
        "runner",
        fl!("settings-page-runner"),
        "system-run-symbolic",
        &[&compatibility, &sync_group],
    );
    add_page(
        "graphics",
        fl!("settings-page-graphics"),
        "video-display-symbolic",
        &[&graphics, &upscaling, &gamescope_group],
    );
    let mut performance_groups = vec![&performance];
    if fps_controls.is_some() {
        performance_groups.push(&fps_group);
    }
    add_page(
        "performance",
        fl!("settings-page-performance"),
        "speedometer-symbolic",
        &performance_groups,
    );
    add_page(
        "advanced",
        fl!("settings-page-advanced"),
        "utilities-terminal-symbolic",
        &[&environment],
    );
    let switcher = adw::ViewSwitcher::builder()
        .stack(&stack)
        .policy(adw::ViewSwitcherPolicy::Wide)
        .build();
    header.set_title_widget(Some(&switcher));
    toolbar.set_content(Some(&stack));
    window.set_child(Some(&toolbar));

    let game_id = game.id.to_string();
    window.connect_closed(move |_| {
        if skip_save.get() {
            return;
        }
        let mut saved = games::load_settings(&game_id, default_path.clone());
        saved.gamemode = gamemode.is_active();
        saved.dxvk_hud =
            berry_games::library::DxvkHud::from_index(dxvk_hud_dropdown.selected() as usize);
        saved.fps_unlocker = fps_controls.is_some() && fps_unlocker.is_active();
        saved.fsr = fsr_switch.is_active();
        saved.fsr_quality =
            berry_games::library::FsrQuality::from_index(fsr_quality_dropdown.selected() as usize);
        saved.sync = berry_games::library::SyncMode::from_index(sync_dropdown.selected() as usize);
        saved.gamescope = gamescope_state.borrow().clone();
        saved.gamescope.enabled = gamescope_wanted.get();
        // Native packages ignore this option, and an environment override is not a choice
        // of the user: in both cases keep what is stored.
        if host_supported && !host_forced {
            saved.launch_on_host = !sandbox_switch.is_active();
        }
        saved.launch_args = launch_args_entry.text().trim().to_string();
        saved.wrapper = wrapper_entry.text().trim().to_string();
        saved.dedicated_gpu = dedicated_gpu.is_active();
        saved.proton_log = proton_log.is_active();
        if let Some(dropdown) = &graphics_api_dropdown {
            saved.graphics_api =
                berry_games::library::GraphicsApi::from_index(dropdown.selected() as usize);
        }
        if let Some(controls) = &fps_controls {
            controls.apply(&mut saved);
        }
        saved.runner = selected_string(&runner_dropdown);
        saved.path = std::path::PathBuf::from(path_entry.text().as_str());
        saved.environment.clear();
        for (name, value, row) in env_rows.borrow().iter() {
            if row.is_visible() && !name.text().is_empty() {
                saved
                    .environment
                    .insert(name.text().to_string(), value.text().to_string());
            }
        }
        if let Err(error) = games::save_settings(&game_id, &saved) {
            tracing::warn!("Could not save settings: {error}");
        }
    });
    window.present(Some(parent));
}

/// Opens a folder in the user's file manager (through the portal inside Flatpak).
fn open_folder(parent: &impl IsA<gtk::Widget>, path: std::path::PathBuf) {
    let launcher = gtk::FileLauncher::new(Some(&gio::File::for_path(&path)));
    let root = parent.root();
    let parent_window = root
        .as_ref()
        .and_then(|root| root.downcast_ref::<gtk::Window>());
    launcher.launch(parent_window, None::<&gio::Cancellable>, move |result| {
        if let Err(error) = result {
            tracing::warn!("Could not open {}: {error}", path.display());
        }
    });
}

/// Total size in bytes of the regular files below `path` (symbolic links are not followed).
fn dir_size(path: &std::path::Path) -> u64 {
    let Ok(entries) = std::fs::read_dir(path) else {
        return 0;
    };
    entries
        .filter_map(Result::ok)
        .map(|entry| match entry.file_type() {
            Ok(kind) if kind.is_dir() => dir_size(&entry.path()),
            Ok(kind) if kind.is_file() => entry.metadata().map(|m| m.len()).unwrap_or(0),
            _ => 0,
        })
        .sum()
}

fn format_size(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["B", "KB", "MB", "GB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

pub(crate) fn add_environment_row(
    group: &adw::PreferencesGroup,
    rows: &Rc<RefCell<Vec<(Entry, Entry, adw::PreferencesRow)>>>,
    name: &str,
    value: &str,
) {
    let row = adw::PreferencesRow::new();
    row.set_activatable(false);
    let box_ = Box::new(Orientation::Horizontal, 8);
    box_.set_margin_top(8);
    box_.set_margin_bottom(8);
    box_.set_margin_start(12);
    box_.set_margin_end(12);
    let name_entry = Entry::new();
    name_entry.set_placeholder_text(Some(&fl!("settings-env-variable")));
    name_entry.set_text(name);
    name_entry.set_hexpand(true);
    name_entry.set_valign(Align::Center);
    let value_entry = Entry::new();
    value_entry.set_placeholder_text(Some(&fl!("settings-env-value")));
    value_entry.set_text(value);
    value_entry.set_hexpand(true);
    value_entry.set_valign(Align::Center);
    let remove = Button::from_icon_name("user-trash-symbolic");
    remove.add_css_class("destructive-action");
    remove.set_valign(Align::Center);
    remove.set_tooltip_text(Some(&fl!("settings-env-remove")));
    box_.append(&name_entry);
    box_.append(&value_entry);
    box_.append(&remove);
    row.set_child(Some(&box_));
    group.add(&row);
    let row_for_remove = row.clone();
    remove.connect_clicked(move |_| row_for_remove.set_visible(false));
    rows.borrow_mut().push((name_entry, value_entry, row));
}
