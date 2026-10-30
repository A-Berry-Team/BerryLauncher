# berry-ui

The GTK 4 / libadwaita interface: the main window, the library and store pages, game details and settings, runner management, and the preferences menu.

## Modules

- `labels`: translations for the types of `berry-games` (`Labeled`, `CatalogText`).
- `application`: `BerryLauncherApplication`, which is the `adw::Application`. `lib.rs` also exports `MainWindow`.

The root `berrylauncher` package only holds `main.rs` and the Meson-generated `config.rs`.

## Dependencies

`berry-ui` depends on every other `berry-*` crate except `berry-installer`. No other crate in `crates/` depends on it. Only the root `berrylauncher` package does.
