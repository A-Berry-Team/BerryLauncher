//! Translated texts for the types of `berry-games`.
//!
//! The crate only knows Fluent message ids; the application owns the translations. The traits keep
//! the call sites (`value.label()`, `game.localized_description()`) unchanged.

use berry_games::catalog::{GameCatalogEntry, GameTag};
use berry_games::library::{
    DxvkHud, FsrQuality, GamescopeFilter, GamescopeMode, GraphicsApi, SyncMode,
};

/// Drop-down label of a settings choice.
pub trait Labeled {
    fn label(self) -> String;
}

impl Labeled for GameTag {
    fn label(self) -> String {
        berry_locale::loader().get(self.label_id())
    }
}

macro_rules! labeled {
    ($($ty:ty),+ $(,)?) => {
        $(impl Labeled for $ty {
            fn label(self) -> String {
                berry_locale::loader().get(self.label_id())
            }
        })+
    };
}

labeled!(
    DxvkHud,
    GraphicsApi,
    FsrQuality,
    SyncMode,
    GamescopeMode,
    GamescopeFilter
);

/// Store description of a catalogue entry.
pub trait CatalogText {
    fn localized_description(&self) -> String;
}

impl CatalogText for GameCatalogEntry {
    fn localized_description(&self) -> String {
        self.description_key()
            .map(|key| berry_locale::loader().get(key))
            .unwrap_or_default()
    }
}
