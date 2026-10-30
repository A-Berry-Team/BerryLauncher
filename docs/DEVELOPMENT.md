# Development

This page is for contributors. It covers the checks CI runs, the external `berry-installer` dependency, the release version rules, and the dependency-audit notes. For the project layout, see [ARCHITECTURE.md](ARCHITECTURE.md). For how to build from source, see the [README](../README.md#building-from-source). For Flatpak packaging, see [build-aux/README.md](../build-aux/README.md).

## Run the checks

Run these commands from the repository root. They are the same checks CI runs.

```sh
cargo fmt --all -- --check
cargo build --workspace --locked
cargo clippy --workspace --all-targets --locked
python3 build-aux/check-version.py <meson version> Cargo.toml data/app.berrylauncher.berry.metainfo.xml.in
```

`<meson version>` is the `version:` value in `meson.build` (for example `1.0.0`).

Two shorter commands are handy while you work:

```sh
cargo fmt --check
cargo check --workspace
```

To iterate on a single crate:

```sh
cargo check -p berry-launcher
```

Before a release, run the same checks and also inspect `git diff --check`. Run `cargo tree --workspace` when you audit dependencies. Remove a dependency only after you have checked all feature-gated and target-specific uses of it.

### Generate `src/config.rs`

Meson generates `src/config.rs`. If you build without Meson, create the file by hand:

```sh
printf 'pub static VERSION: &str = "1.0.0";\npub static PKGDATADIR: &str = "/usr/share";\n' > src/config.rs
```

### Known build issue: JavaScriptCoreGTK

`cargo check --workspace --all-targets` can stop before it reaches any project code. It fails while building `javascriptcore6-sys` because `pkg-config` cannot find `javascriptcoregtk-6.0.pc`. Install the matching JavaScriptCoreGTK development package for your distribution, then run the check again. Until then, do not treat the source as compile-verified.

## The `berry-installer` dependency

The install engine lives in its own repository, <https://github.com/A-Berry-Team/berry-installer>. It is linked as a git submodule at `crates/berry-installer`, and the root `Cargo.toml` declares it once, as a path dependency in `[workspace.dependencies]`. The commit recorded in the submodule (the gitlink) is the pin, so builds are reproducible until someone bumps it on purpose.

Clone with `git clone --recurse-submodules`, or run `git submodule update --init` in an existing clone. CI checks out submodules recursively.

To update it, move the submodule to the commit you want and commit the new gitlink:

```sh
git submodule update --remote crates/berry-installer   # take the newest commit of the submodule's tracked branch
git add crates/berry-installer
```

Dependabot (`gitsubmodule` ecosystem) also opens update pull requests for this pin. If the update changes the installer's dependencies, `Cargo.lock` changes too, so refresh the offline Flatpak sources and commit both files:

```sh
build-aux/update-cargo-sources.sh
```

To work on the installer and the launcher together, edit files directly in `crates/berry-installer`: it is a normal git checkout. Commit and push inside the submodule first, then commit the new gitlink in this repository.

## Release a version

The version for this source snapshot is `1.0.0`. It lives in four places, and `check-version.py` fails when they differ:

1. `version` in `meson.build`
2. `version` under `[package]` in the root `Cargo.toml`
3. `version` under `[workspace.package]` in the root `Cargo.toml`
4. The newest `<release>` entry in `data/app.berrylauncher.berry.metainfo.xml.in`

`Cargo.lock` also lists the workspace crates at that version.

To release: bump the version in `meson.build` and `Cargo.toml`, and add a `<release>` entry to the metainfo file. `meson test` fails if they disagree. For the Flatpak steps, see [build-aux/README.md](../build-aux/README.md).

## Launch behavior and translations

The launch behavior preference takes the values `hide` (default), `keep-open` and `close`. See [ARCHITECTURE.md](ARCHITECTURE.md#launch-behavior-preference) for what each one does.

If you add translations or change the preference UI:

- Update both the English and the Spanish Fluent catalogs.
- Keep the enum order and index mapping in `crates/berry-core/src/settings.rs` aligned with `crates/berry-ui/src/menu.rs`.

For the application icon, see the [`data/` README](../data/README.md#application-icon).

## Dependency audit (release preparation)

Run `cargo machete` before you remove a direct dependency. The audit for the 1.0.0 release reported no unused dependencies.

Before you change the manifest or the lockfile, inspect:

```sh
cargo tree --workspace --edges normal
cargo tree --workspace --edges build
cargo tree --workspace --duplicates
```

Duplicate transitive versions are not automatically removable. This is especially true when `berry-installer` pulls them in.

## Visibility backlog

Some items are `pub` but are only used inside their own crate. They are candidates for `pub(crate)`. Tightening them also lets the compiler report them when they become dead code.

| Crate | Items |
| --- | --- |
| `berry-downloads` | `resolve_update`, `update_available` |
| `berry-runners` | `prefetch_for_runner`, `required_runtime` |
| `berry-launcher` | `EXTENSION_ID`, `GamescopeSource` |
| `berry-games` | `ensure_games_dir`, `games_dir`, `settings_path` |
| `berry-media` | `background_dir`, `find_catalog_resource`, `find_signed_url`, `texture_from_bytes`, `webp_dimensions`, `BackgroundAssets` |
| `berry-core` | `preferences_path` |

Types that appear in a public signature (`Backend`, `DownloadJob`, `InstallPlan`) must be tightened together with the functions that return them.
