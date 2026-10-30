//! Layout values shared by every window that shows content.
//!
//! The game settings window is the reference: its `AdwPreferencesPage` clamps the content to
//! [`PREFS_CONTENT_WIDTH`] px inside an 800 px window, which leaves 100 px on each side. Every
//! other content window uses the same [`PAGE_PADDING`] instead of choosing its own margins.
//! To change the spacing of the whole launcher, change [`PAGE_PADDING`] only.

use adw::prelude::*;

/// Space between the window edge and the content, on the left and on the right.
pub(crate) const PAGE_PADDING: i32 = 100;

/// Width libadwaita clamps the content of an `AdwPreferencesPage` to.
pub(crate) const PREFS_CONTENT_WIDTH: i32 = 600;

/// Window width at which an `AdwPreferencesPage` shows exactly [`PAGE_PADDING`] on each side.
/// Windows made of preferences pages (game settings, gamescope, launcher preferences) use it as
/// their width so they match the windows that apply [`PAGE_PADDING`] as a plain margin.
pub(crate) const PADDED_WINDOW_WIDTH: i32 = PREFS_CONTENT_WIDTH + 2 * PAGE_PADDING;

/// Default height of the windows that sit next to the game settings (game settings, operation
/// details). Together with [`PADDED_WINDOW_WIDTH`] it gives them all the same size.
pub(crate) const SETTINGS_WINDOW_HEIGHT: i32 = 580;

/// Give `content` the standard left and right padding.
pub(crate) fn apply_page_padding(content: &impl IsA<gtk::Widget>) {
    content.set_margin_start(PAGE_PADDING);
    content.set_margin_end(PAGE_PADDING);
}
