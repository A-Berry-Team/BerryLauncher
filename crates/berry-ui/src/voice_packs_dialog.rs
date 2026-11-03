//! The "Voice-over languages" prompt shown before a HoYoverse game is installed.
//!
//! Same idea as the an-anime-team launchers: one switch per voice-over package the game offers,
//! only English on by default. Every switch may be turned off, which installs the game without
//! any voice-over package and lets the game download its own default the first time it starts.

use adw::prelude::*;
use gtk::Switch;

use crate::widgets::suffix_row;
use berry_games::catalog::GameCatalogEntry;
use berry_locale::fl;

/// Ask which voice-over packages to download with `game`.
///
/// `on_result` receives the chosen language codes (possibly none) when the user confirms, and
/// `None` when the dialog is cancelled or dismissed, in which case nothing may be started.
pub(crate) fn choose_voice_packs(
    parent: &impl IsA<gtk::Widget>,
    game: GameCatalogEntry,
    on_result: impl FnOnce(Option<Vec<String>>) + 'static,
) {
    let defaults = game.default_voice_packs();
    let group = adw::PreferencesGroup::new();
    let mut switches: Vec<(&'static str, Switch)> = Vec::new();
    for pack in game.voice_packs() {
        let switch = Switch::new();
        switch.set_active(defaults.iter().any(|code| code == pack.code));
        group.add(&suffix_row(
            &berry_locale::translate(pack.label_id),
            None,
            &switch,
        ));
        switches.push((pack.code, switch));
    }

    let dialog = adw::AlertDialog::builder()
        .heading(&fl!("voice-packs-heading"))
        .body(&fl!("voice-packs-body", game = game.title))
        .build();
    dialog.set_extra_child(Some(&group));
    dialog.add_response("cancel", &fl!("common-cancel"));
    dialog.add_response("confirm", &fl!("common-confirm"));
    dialog.set_response_appearance("confirm", adw::ResponseAppearance::Suggested);
    dialog.set_default_response(Some("confirm"));
    dialog.set_close_response("cancel");
    dialog.choose(Some(parent), None::<&gio::Cancellable>, move |response| {
        if response.as_str() != "confirm" {
            on_result(None);
            return;
        }
        let selected = switches
            .iter()
            .filter(|(_, switch)| switch.is_active())
            .map(|(code, _)| (*code).to_owned())
            .collect();
        on_result(Some(selected));
    });
}
