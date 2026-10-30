# Architecture

This page explains how Berry Launcher is organized: which crate does what, how they depend on each other, and which UI conventions the app follows. It is written for new contributors. For build and release steps, see [DEVELOPMENT.md](DEVELOPMENT.md).

## Application diagram

[![Architecture diagram of a-berry-team/berrylauncher](https://gitdiagram.com/a-berry-team/berrylauncher/diagram.png)](https://gitdiagram.com/a-berry-team/berrylauncher?utm_source=readme&utm_medium=picture)

## How the application flows

The project is a Cargo workspace. A *crate* is one Rust package. Each crate has one job:

- The UI hands catalogue and library state to `berry-games`.
- It hands network and install jobs to `berry-downloads` and the shared `berry-installer` engine.
- It hands Proton/Wine and Steam Linux Runtime management to `berry-runners`.
- `berry-launcher` coordinates launching a game. It builds the runner environment, the optional Gamescope and FPS controls, and the diagnostics.
- `berry-media` supplies artwork, backgrounds and screenshots.
- `berry-locale` owns the English and Spanish Fluent catalogues.

The root `berrylauncher` package only holds `main.rs` and the Meson-generated `config.rs`.

### Crates at a glance

| Crate | Purpose | Internal dependencies |
| --- | --- | --- |
| [`berry-core`](../crates/berry-core/README.md) | Shared foundation: paths, preferences, host probes | none |
| [`berry-games`](../crates/berry-games/README.md) | Game catalogue and per-game library data | `berry-core` |
| [`berry-locale`](../crates/berry-locale/README.md) | Localization | none |
| [`berry-runners`](../crates/berry-runners/README.md) | Wine/Proton runners and Steam Linux Runtime | `berry-core`, `berry-installer` |
| [`berry-downloads`](../crates/berry-downloads/README.md) | Install, update and repair jobs | `berry-core`, `berry-games`, `berry-locale`, `berry-installer` |
| [`berry-launcher`](../crates/berry-launcher/README.md) | Everything that happens when you press Play | `berry-core`, `berry-games`, `berry-runners`, `berry-installer`, `berry-downloads`, `berry-locale` |
| [`berry-media`](../crates/berry-media/README.md) | Game artwork and images | `berry-core`, `berry-games`, `berry-locale` |
| [`berry-ui`](../crates/berry-ui/README.md) | GTK 4 / libadwaita interface | every `berry-*` crate above except `berry-installer` |

No other crate in `crates/` depends on `berry-ui`. Only the root `berrylauncher` package does.

## Window and dialog conventions

- The main window is an `adw::ApplicationWindow`.
- Preferences use an `adw::PreferencesDialog`.
- Game settings, Gamescope settings, download details and the full runner list use `adw::Dialog`. They are presented inside the main window, so they are not separate top-level windows.
- Confirmation and error prompts use `adw::AlertDialog`, not GTK top-level alert dialogs.
- Transient utility views are modal and are destroyed with their parent window. Download details follow the same convention as game settings.
- Dialogs that edit settings save when their `closed` signal fires. The save path therefore follows the dialog lifecycle, not a window close request.

## Launch behavior preference

The launch behavior preference is stored as one of three values:

| Value | Meaning |
| --- | --- |
| `hide` (default) | After a successful launch, the main window is hidden. It is shown again when the game exits. |
| `keep-open` | Keep the main window open. |
| `close` | Close the main window. |

When you change this preference or its UI, keep the enum order and index mapping in `crates/berry-core/src/settings.rs` aligned with `crates/berry-ui/src/menu.rs`. Update both Fluent catalogs as well. See [DEVELOPMENT.md](DEVELOPMENT.md#launch-behavior-and-translations).

## Application icon

For the icon file, its size and how it is installed, see the [`data/` README](../data/README.md#application-icon).

## Dependencies

All internal crates and shared third-party versions are declared once, in `[workspace.dependencies]` of the root `Cargo.toml`. Member crates use `name.workspace = true`.

`berry-installer` is the exception. It is a git submodule (`crates/berry-installer`, from `github.com/A-Berry-Team/berry-installer`) and it manages its own versions. For example, it uses reqwest 0.13, while the workspace uses reqwest 0.12. To update it, see [DEVELOPMENT.md](DEVELOPMENT.md#the-berry-installer-dependency).
