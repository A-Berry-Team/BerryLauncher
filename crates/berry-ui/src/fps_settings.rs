//! FPS unlocker rows of the game settings: target FPS (presets or a custom value) and, for the
//! games that read them, the unlocker's refresh / initial delays.

use adw::prelude::*;

use crate::widgets::suffix_row;
use berry_games::catalog::{FpsUnlockSupport, FPS_CUSTOM_MIN};
use berry_games::library::GameSettings;
use berry_locale::fl;

pub(crate) struct FpsControls {
    presets: Vec<u32>,
    dropdown: gtk::DropDown,
    custom: adw::SpinRow,
    refresh: Option<adw::SpinRow>,
    init: Option<adw::SpinRow>,
}

fn spin_row(
    title: String,
    subtitle: String,
    min: f64,
    max: f64,
    step: f64,
    value: f64,
) -> adw::SpinRow {
    let row = adw::SpinRow::with_range(min, max, step);
    row.set_title(&title);
    row.set_subtitle(&subtitle);
    row.set_value(value);
    row
}

impl FpsControls {
    /// Adds the target FPS rows to `group`. The list holds the presets the game can use (up to
    /// its maximum) and "Custom" last; the custom input starts at the last selected preset.
    pub(crate) fn build(
        support: FpsUnlockSupport,
        settings: &GameSettings,
        group: &adw::PreferencesGroup,
    ) -> Self {
        let presets = support.presets();
        let mut labels: Vec<String> = presets.iter().map(u32::to_string).collect();
        labels.push(fl!("settings-fps-custom"));
        let label_refs = labels.iter().map(String::as_str).collect::<Vec<_>>();
        let dropdown = gtk::DropDown::from_strings(&label_refs);
        dropdown.set_size_request(140, -1);

        // A stored value that is not a preset (a custom one, or one from an older version of the
        // list) is shown as custom, keeping its value.
        let preset_index = presets.iter().position(|fps| *fps == settings.fps);
        let custom_index = presets.len() as u32;
        dropdown.set_selected(preset_index.map_or(custom_index, |index| index as u32));
        group.add(&suffix_row(
            &fl!("settings-fps-target-title"),
            Some(&fl!("settings-fps-target-subtitle")),
            &dropdown,
        ));

        let custom = spin_row(
            fl!("settings-fps-custom-title"),
            fl!("settings-fps-custom-subtitle"),
            f64::from(FPS_CUSTOM_MIN),
            f64::from(support.max_custom),
            1.0,
            f64::from(support.clamp(settings.fps)),
        );
        custom.set_visible(preset_index.is_none());
        group.add(&custom);

        let (refresh, init) = if support.uses_delays {
            let refresh = spin_row(
                fl!("settings-fps-refresh-title"),
                fl!("settings-fps-refresh-subtitle"),
                100.0,
                60_000.0,
                100.0,
                f64::from(settings.fps_interval_ms),
            );
            let init = spin_row(
                fl!("settings-fps-init-title"),
                fl!("settings-fps-init-subtitle"),
                0.0,
                600_000.0,
                100.0,
                f64::from(settings.fps_init_delay_ms),
            );
            group.add(&refresh);
            group.add(&init);
            (Some(refresh), Some(init))
        } else {
            (None, None)
        };

        // Picking a preset makes it the custom input's value, so "Custom" starts from the last
        // preset that was selected.
        let preset_values = presets.clone();
        let custom_row = custom.clone();
        dropdown.connect_selected_notify(move |dropdown| {
            match preset_values.get(dropdown.selected() as usize) {
                Some(fps) => {
                    custom_row.set_value(f64::from(*fps));
                    custom_row.set_visible(false);
                }
                None => custom_row.set_visible(true),
            }
        });

        Self {
            presets,
            dropdown,
            custom,
            refresh,
            init,
        }
    }

    /// Writes the chosen values into `settings`.
    pub(crate) fn apply(&self, settings: &mut GameSettings) {
        settings.fps = match self.presets.get(self.dropdown.selected() as usize) {
            Some(fps) => *fps,
            None => self.custom.value().round() as u32,
        };
        if let Some(row) = &self.refresh {
            settings.fps_interval_ms = row.value().round() as u32;
        }
        if let Some(row) = &self.init {
            settings.fps_init_delay_ms = row.value().round() as u32;
        }
    }
}
