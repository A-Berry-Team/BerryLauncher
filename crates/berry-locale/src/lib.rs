use std::sync::OnceLock;

use i18n_embed::fluent::FluentLanguageLoader;
use i18n_embed::unic_langid::LanguageIdentifier;
use i18n_embed::LanguageLoader;
use rust_embed::RustEmbed;
use tracing::warn;

pub use fluent::FluentValue;

/// Preference value that follows the system locale.
pub const SYSTEM: &str = "system";

/// Name of the Fluent files (`i18n/<code>/berrylauncher.ftl`). Kept as the original domain so the
/// translation files did not need to be renamed when this crate was split out.
const DOMAIN: &str = "berrylauncher";
const FALLBACK_LANGUAGE: &str = "en";

// Fluent files embedded from `i18n/<code>/berrylauncher.ftl`.
#[derive(RustEmbed)]
#[folder = "i18n/"]
struct Localizations;

struct State {
    loader: FluentLanguageLoader,
    /// True when the interface language is one the launcher actually has: either picked in
    /// Preferences, or the system language when a translation for it exists. False only when
    /// the system language has no translation and the interface fell back to English.
    resolved: bool,
    available: Vec<LanguageIdentifier>,
}

static STATE: OnceLock<State> = OnceLock::new();

fn new_loader() -> FluentLanguageLoader {
    FluentLanguageLoader::new(
        DOMAIN,
        FALLBACK_LANGUAGE.parse().expect("valid fallback language"),
    )
}

/// Translates a message id: `fl!("game-play")` or `fl!("library-count", count = 3u64)`.
///
/// Ids are resolved at runtime; an id that is missing from `i18n/en` renders as the id itself.
#[macro_export]
macro_rules! fl {
    ($message_id:literal) => {
        $crate::translate($message_id)
    };
    ($message_id:literal, $($key:ident = $value:expr),+ $(,)?) => {{
        let mut args = ::std::collections::HashMap::new();
        $( args.insert(stringify!($key), $crate::FluentValue::from($value)); )+
        $crate::translate_args($message_id, args)
    }};
}

/// Message `id` in the active language.
pub fn translate(id: &str) -> String {
    loader().get(id)
}

/// Message `id` with arguments, in the active language.
pub fn translate_args(id: &str, args: std::collections::HashMap<&str, FluentValue<'_>>) -> String {
    loader().get_args_concrete(id, args)
}

fn code_of(language: &LanguageIdentifier) -> String {
    language.to_string().to_lowercase()
}

/// `es` for `es`, `es-ES` or `es_ES`.
fn primary_language(language: &LanguageIdentifier) -> String {
    code_of(language)
        .split(['-', '_'])
        .next()
        .unwrap_or_default()
        .to_owned()
}

fn language_name(language: &LanguageIdentifier) -> String {
    let loader = new_loader();
    match loader.load_languages(&Localizations, &[language.clone()]) {
        Ok(()) => loader.get("language-name"),
        Err(_) => language.to_string(),
    }
}

fn system_languages() -> Vec<LanguageIdentifier> {
    glib::language_names()
        .iter()
        .filter_map(|name| {
            let locale = name.split(['.', '@']).next()?;
            if locale.is_empty() || locale == "C" || locale == "POSIX" {
                return None;
            }
            locale.replace('_', "-").parse().ok()
        })
        .collect()
}

impl State {
    fn load(preference: &str) -> Self {
        let loader = new_loader();
        let mut available = loader
            .available_languages(&Localizations)
            .unwrap_or_default();
        available.sort_by_key(code_of);
        if available.is_empty() {
            warn!("[i18n] no translations are embedded in this build");
        }

        let chosen = (preference != SYSTEM)
            .then(|| {
                available
                    .iter()
                    .find(|language| code_of(language) == preference)
            })
            .flatten();
        let requested = match chosen {
            Some(language) => vec![language.clone()],
            None => system_languages(),
        };

        if let Err(error) = i18n_embed::select(&loader, &Localizations, &requested) {
            warn!("[i18n] could not load the interface language: {error}");
            let _ = loader.load_fallback_language(&Localizations);
        }
        // Isolation marks around arguments show up as stray glyphs in GTK widgets.
        loader.set_use_isolating(false);

        // With "System" the interface uses the first system language that has a translation, so
        // that is the language the launcher is in, just as if it had been picked by hand.
        let resolved = chosen.is_some()
            || requested.iter().any(|wanted| {
                available
                    .iter()
                    .any(|language| primary_language(language) == primary_language(wanted))
            });

        Self {
            loader,
            resolved,
            available,
        }
    }
}

fn state() -> &'static State {
    STATE.get_or_init(|| State::load(SYSTEM))
}

pub fn init(preference: &str) {
    let _ = STATE.set(State::load(preference));
}

pub fn loader() -> &'static FluentLanguageLoader {
    &state().loader
}

pub fn choices() -> Vec<(String, String)> {
    let mut choices = vec![(SYSTEM.to_string(), translate("preferences-language-system"))];
    // Names are resolved here, not at startup: only the preferences page needs them.
    choices.extend(
        state()
            .available
            .iter()
            .map(|l| (code_of(l), language_name(l))),
    );
    choices
}

pub fn background_language() -> String {
    translate("hoyo-language")
}

pub fn play_locale() -> (String, String) {
    (translate("play-language"), translate("play-country"))
}

/// Locale for the Wine/Proton prefix (`LANG`): the language the launcher is shown in, whether it
/// was picked in Preferences or comes from the system (`System`). A prefix therefore follows the
/// launcher's language, not the system's when the two differ. `None` only when the system
/// language has no translation, so the prefix keeps the system locale instead of English.
pub fn wine_locale() -> Option<String> {
    state().resolved.then(|| translate("wine-locale"))
}
