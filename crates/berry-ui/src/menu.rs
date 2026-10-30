use adw::prelude::*;
use gtk::{gio, Align, Box, Button, MenuButton, Orientation, Switch, ToggleButton};
use std::rc::Rc;

use berry_locale as i18n;
use berry_locale::fl;

type ThemeSaver = Rc<dyn Fn(berry_core::settings::Theme)>;

pub(crate) fn install_main_menu(window: &adw::ApplicationWindow, header: &adw::HeaderBar) {
    let preferences = berry_core::settings::load_preferences();

    let popover = gtk::Popover::new();
    popover.set_has_arrow(true);

    let content = Box::new(Orientation::Vertical, 6);
    content.set_margin_top(10);
    content.set_margin_bottom(10);
    content.set_margin_start(10);
    content.set_margin_end(10);

    let theme_box = Box::new(Orientation::Horizontal, 8);
    theme_box.set_halign(Align::Center);
    theme_box.set_hexpand(true);

    let system_theme = ToggleButton::new();
    let light_theme = ToggleButton::new();
    let dark_theme = ToggleButton::new();
    system_theme.set_group(Some(&light_theme));
    dark_theme.set_group(Some(&light_theme));

    for button in [&system_theme, &light_theme, &dark_theme] {
        button.set_size_request(44, 44);
        button.set_hexpand(true);
        button.set_vexpand(false);
        button.add_css_class("berry-theme-choice");
        theme_box.append(button);
    }

    system_theme.add_css_class("berry-theme-system");
    light_theme.add_css_class("berry-theme-light");
    dark_theme.add_css_class("berry-theme-dark");
    system_theme.set_tooltip_text(Some(&fl!("preferences-theme-system")));
    light_theme.set_tooltip_text(Some(&fl!("preferences-theme-light")));
    dark_theme.set_tooltip_text(Some(&fl!("preferences-theme-dark")));

    match preferences.theme {
        berry_core::settings::Theme::System => system_theme.set_active(true),
        berry_core::settings::Theme::Light => light_theme.set_active(true),
        berry_core::settings::Theme::Dark => dark_theme.set_active(true),
    }

    let save_theme: ThemeSaver = Rc::new(move |theme: berry_core::settings::Theme| {
        apply_theme(theme);
        let current = berry_core::settings::load_preferences();
        let updated = berry_core::settings::Preferences { theme, ..current };
        if let Err(error) = berry_core::settings::save_preferences(&updated) {
            tracing::warn!("Could not save launcher theme: {error:#}");
        }
    });

    {
        let save_theme = save_theme.clone();
        system_theme.connect_toggled(move |button| {
            if button.is_active() {
                save_theme(berry_core::settings::Theme::System);
            }
        });
    }
    {
        let save_theme = save_theme.clone();
        light_theme.connect_toggled(move |button| {
            if button.is_active() {
                save_theme(berry_core::settings::Theme::Light);
            }
        });
    }
    {
        let save_theme = save_theme.clone();
        dark_theme.connect_toggled(move |button| {
            if button.is_active() {
                save_theme(berry_core::settings::Theme::Dark);
            }
        });
    }

    content.append(&theme_box);
    content.append(&gtk::Separator::new(Orientation::Horizontal));

    let open_folder = Button::with_label(&fl!("menu-open-folder"));
    open_folder.set_halign(Align::Fill);
    open_folder.set_hexpand(true);
    {
        let popover = popover.clone();
        open_folder.connect_clicked(move |_| {
            let folder = berry_core::settings::launcher_dir();
            let uri = gio::File::for_path(folder).uri();
            if let Err(error) =
                gio::AppInfo::launch_default_for_uri(&uri, None::<&gio::AppLaunchContext>)
            {
                tracing::warn!("Could not open launcher folder: {error}");
            }
            popover.popdown();
        });
    }
    content.append(&open_folder);

    let preferences_button = Button::with_label(&fl!("menu-preferences"));
    preferences_button.set_halign(Align::Fill);
    preferences_button.set_hexpand(true);
    {
        let parent = window.clone();
        let popover = popover.clone();
        preferences_button.connect_clicked(move |_| {
            show_launcher_preferences(&parent);
            popover.popdown();
        });
    }
    content.append(&preferences_button);

    popover.set_child(Some(&content));

    let button = MenuButton::builder()
        .icon_name("open-menu-symbolic")
        .tooltip_text(&fl!("menu-main"))
        .build();
    button.set_has_frame(false);
    button.set_popover(Some(&popover));
    header.pack_end(&button);
}

pub(crate) fn show_launcher_preferences(parent: &adw::ApplicationWindow) {
    let preferences = berry_core::settings::load_preferences();
    apply_theme(preferences.theme);

    let dialog = adw::PreferencesDialog::builder()
        .title(fl!("menu-preferences"))
        .content_width(crate::layout::PADDED_WINDOW_WIDTH)
        .content_height(560)
        .build();

    let page = adw::PreferencesPage::new();
    page.set_title(&fl!("menu-preferences"));
    page.set_icon_name(Some("preferences-system-symbolic"));

    let appearance = adw::PreferencesGroup::new();
    appearance.set_title(&fl!("preferences-appearance"));

    let language_choices = i18n::choices();
    let language_labels = language_choices
        .iter()
        .map(|(_, label)| label.as_str())
        .collect::<Vec<_>>();
    let language_dropdown = gtk::DropDown::from_strings(&language_labels);
    let language_index = language_choices
        .iter()
        .position(|(code, _)| *code == preferences.language)
        .unwrap_or(0);
    language_dropdown.set_selected(language_index as u32);
    language_dropdown.set_width_request(140);
    language_dropdown.set_hexpand(false);
    language_dropdown.set_halign(Align::End);
    language_dropdown.set_valign(Align::Center);
    let language_row = adw::ActionRow::new();
    language_row.set_title(&fl!("preferences-language-title"));
    language_row.set_subtitle(&fl!("preferences-language-subtitle"));
    language_row.add_suffix(&language_dropdown);
    appearance.add(&language_row);

    let animated = Switch::new();
    animated.set_active(preferences.animated_background);
    animated.set_valign(Align::Center);
    animated.set_hexpand(false);
    let animated_row = adw::ActionRow::new();
    animated_row.set_title(&fl!("preferences-animated-background-title"));
    animated_row.set_subtitle(&fl!("preferences-animated-background-subtitle"));
    animated_row.add_suffix(&animated);
    appearance.add(&animated_row);

    let launcher_behavior_values = vec![
        fl!("preferences-behavior-hide"),
        fl!("preferences-behavior-keep-open"),
        fl!("preferences-behavior-close"),
    ];
    let launcher_behavior_refs = launcher_behavior_values
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>();
    let launcher_behavior_dropdown = gtk::DropDown::from_strings(&launcher_behavior_refs);
    launcher_behavior_dropdown.set_selected(preferences.launcher_behavior.index() as u32);
    launcher_behavior_dropdown.set_width_request(140);
    launcher_behavior_dropdown.set_hexpand(false);
    launcher_behavior_dropdown.set_halign(Align::End);
    launcher_behavior_dropdown.set_valign(Align::Center);

    let launcher = adw::PreferencesGroup::new();
    launcher.set_title(&fl!("preferences-launcher"));

    let launcher_behavior_row = adw::ActionRow::new();
    launcher_behavior_row.set_title(&fl!("preferences-behavior-title"));
    launcher_behavior_row.set_subtitle(&fl!("preferences-behavior-subtitle"));
    launcher_behavior_row.add_suffix(&launcher_behavior_dropdown);
    launcher.add(&launcher_behavior_row);

    let restore_game = Switch::new();
    restore_game.set_active(preferences.restore_last_game);
    launcher.add(&switch_row(
        &fl!("preferences-restore-game-title"),
        &fl!("preferences-restore-game-subtitle"),
        &restore_game,
    ));

    let downloads_group = adw::PreferencesGroup::new();
    downloads_group.set_title(&fl!("preferences-downloads"));
    let confirm_quit = Switch::new();
    confirm_quit.set_active(preferences.confirm_quit_while_downloading);
    downloads_group.add(&switch_row(
        &fl!("preferences-quit-confirm-title"),
        &fl!("preferences-quit-confirm-subtitle"),
        &confirm_quit,
    ));
    let pause_on_launch = Switch::new();
    pause_on_launch.set_active(preferences.pause_downloads_on_launch);
    downloads_group.add(&switch_row(
        &fl!("preferences-pause-on-launch-title"),
        &fl!("preferences-pause-on-launch-subtitle"),
        &pause_on_launch,
    ));

    page.add(&appearance);
    page.add(&launcher);
    page.add(&downloads_group);
    dialog.add(&page);

    dialog.connect_closed(move |_| {
        let current = berry_core::settings::load_preferences();
        let language = language_choices
            .get(language_dropdown.selected() as usize)
            .map(|(code, _)| code.clone())
            .unwrap_or_else(|| i18n::SYSTEM.to_string());
        let launcher_behavior = berry_core::settings::LauncherBehavior::from_index(
            launcher_behavior_dropdown.selected() as usize,
        );
        let preferences = berry_core::settings::Preferences {
            theme: current.theme,
            language,
            animated_background: animated.is_active(),
            launcher_behavior,
            restore_last_game: restore_game.is_active(),
            confirm_quit_while_downloading: confirm_quit.is_active(),
            pause_downloads_on_launch: pause_on_launch.is_active(),
        };
        if let Err(error) = berry_core::settings::save_preferences(&preferences) {
            tracing::warn!("Could not save launcher preferences: {error:#}");
        }
    });

    dialog.present(Some(parent));
}

/// A preferences row with a switch on the right.
fn switch_row(title: &str, subtitle: &str, switch: &Switch) -> adw::ActionRow {
    switch.set_valign(Align::Center);
    switch.set_hexpand(false);
    let row = adw::ActionRow::new();
    row.set_title(title);
    row.set_subtitle(subtitle);
    row.add_suffix(switch);
    row.set_activatable_widget(Some(switch));
    row
}

pub(crate) fn apply_theme(theme: berry_core::settings::Theme) {
    let manager = adw::StyleManager::default();
    manager.set_color_scheme(match theme {
        berry_core::settings::Theme::System => adw::ColorScheme::Default,
        berry_core::settings::Theme::Light => adw::ColorScheme::ForceLight,
        berry_core::settings::Theme::Dark => adw::ColorScheme::ForceDark,
    });
}
