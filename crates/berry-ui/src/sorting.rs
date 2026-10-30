//! A-Z / Z-A ordering of the game cards, with one independent setting per page.

use std::cell::Cell;
use std::cmp::Ordering;

use gtk::prelude::*;
use gtk::{Align, Button};

use berry_games::catalog::GameCatalogEntry;
use berry_games::library as games;
use berry_locale::fl;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SortOrder {
    /// A to Z (the default).
    Ascending,
    /// Z to A.
    Descending,
}

impl SortOrder {
    fn toggled(self) -> Self {
        match self {
            SortOrder::Ascending => SortOrder::Descending,
            SortOrder::Descending => SortOrder::Ascending,
        }
    }

    fn icon_name(self) -> &'static str {
        match self {
            SortOrder::Ascending => "view-sort-ascending-symbolic",
            SortOrder::Descending => "view-sort-descending-symbolic",
        }
    }

    fn tooltip(self) -> String {
        match self {
            SortOrder::Ascending => fl!("sort-a-z"),
            SortOrder::Descending => fl!("sort-z-a"),
        }
    }

    /// Case-insensitive title comparison in this direction.
    fn compare(self, left: &GameCatalogEntry, right: &GameCatalogEntry) -> Ordering {
        let ordering = left.title.to_lowercase().cmp(&right.title.to_lowercase());
        match self {
            SortOrder::Ascending => ordering,
            SortOrder::Descending => ordering.reverse(),
        }
    }
}

/// The pages that have their own sort button. Each keeps its order separately, for the whole
/// session. Everything runs on the GTK main thread.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SortScope {
    Library,
    Store,
}

thread_local! {
    static LIBRARY_ORDER: Cell<SortOrder> = Cell::new(SortOrder::Ascending);
    static STORE_ORDER: Cell<SortOrder> = Cell::new(SortOrder::Ascending);
}

impl SortScope {
    fn with<R>(self, f: impl FnOnce(&Cell<SortOrder>) -> R) -> R {
        match self {
            SortScope::Library => LIBRARY_ORDER.with(f),
            SortScope::Store => STORE_ORDER.with(f),
        }
    }

    pub(crate) fn order(self) -> SortOrder {
        self.with(|cell| cell.get())
    }

    fn toggle(self) -> SortOrder {
        self.with(|cell| {
            let next = cell.get().toggled();
            cell.set(next);
            next
        })
    }
}

fn show_order(button: &Button, order: SortOrder) {
    button.set_icon_name(order.icon_name());
    button.set_tooltip_text(Some(&order.tooltip()));
}

/// Flat icon button that flips the order of `scope` and then calls `on_change` so the page can
/// rebuild its cards. Its icon and tooltip always show the current order.
pub(crate) fn sort_button(scope: SortScope, on_change: impl Fn() + 'static) -> Button {
    let button = Button::new();
    button.add_css_class("flat");
    button.set_valign(Align::Center);
    show_order(&button, scope.order());
    button.connect_clicked(move |button| {
        show_order(button, scope.toggle());
        on_change();
    });
    button
}

/// `entries` alphabetically in the order of `scope`. With `library_first`, games already in the
/// library form their own list ahead of the rest; both lists keep the alphabetical order.
pub(crate) fn sorted_games(
    entries: &[GameCatalogEntry],
    scope: SortScope,
    library_first: bool,
) -> Vec<GameCatalogEntry> {
    let order = scope.order();
    // Read the library once per game instead of once per comparison.
    let mut keyed: Vec<(bool, GameCatalogEntry)> = entries
        .iter()
        .map(|game| {
            (
                library_first && games::is_in_library(game.folder_name),
                *game,
            )
        })
        .collect();
    keyed.sort_by(|(left_in, left), (right_in, right)| {
        right_in
            .cmp(left_in)
            .then_with(|| order.compare(left, right))
    });
    keyed.into_iter().map(|(_, game)| game).collect()
}
