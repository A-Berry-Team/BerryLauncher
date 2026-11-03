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


## berry-patcher (LDiff pre-downloads and updates)

`berry-installer` applies HoYoverse LDiff patches with
[`berry-patcher`](https://github.com/A-Berry-Team/berry-patcher), our fork of TwintailTeam's
`hdiffpatch-rs`. It is a plain git dependency of `crates/berry-installer/Cargo.toml`, pinned by
`rev`, so there is no adapter crate and no vendoring script any more (`crates/hdiffpatch-rs` and
`scripts/vendor-hdiffpatch.sh` are gone).

To take a newer berry-patcher, bump `rev` in the installer, then refresh the lock file and the
offline Flatpak sources (the git source is part of `Cargo.lock`, so Flathub builds need it):

```bash
# in crates/berry-installer/Cargo.toml: rev = "<new commit>"
cargo update -p berry-patcher
cargo check -p berry-installer
./build-aux/update-cargo-sources.sh  # refresh Flatpak's offline Cargo source cache
```

`crates/berry-installer` is a submodule: commit and push the `Cargo.toml` change inside it first,
then commit the new gitlink here together with `Cargo.lock` and `cargo-sources.json`.

HoYoverse LDiff pre-downloads fetch the LDiff protobuf manifest, then download each required
patch range with HTTP Range requests from `<file_path>/<patch_id>`. Ranges are persisted under
`<game>/.berry-preload/ldiff`; they are not applied during pre-download, so the live installation
remains launchable until the user performs the update. The ranges are streamed to disk and progress is
reported live (bytes counted as they arrive, smoothed network speed every 250 ms, same `SpeedMeter` as the
Sophon engine), so the UI shows real traffic during the pre-download instead of 0 B/s.
`berry-installer::ldiff::apply_cached_manifest_for_url`
provides the verified HDiff application step for the subsequent update flow.

## Game manifests

Which version of a game is current, which LDiff diffs exist for it and (for Wuthering Waves) where
its index lives come from [A-Berry-Team/game-manifests](https://github.com/A-Berry-Team/game-manifests),
one `<biz>.json` per game, fetched from `raw.githubusercontent.com/.../main`. The URL is built in one
place, `berry-downloads/src/sources/mod.rs` (`game_manifest_url`). The files have the same structure
as TwintailTeam's: `latest_version`, `game_versions[]` and an optional `extra.preload`.

| Game | `biz` | Live update | Pre-download |
| --- | --- | --- | --- |
| Genshin Impact | `hk4e_global` | LDiff matching the installed version, else Sophon | `extra.preload`, else HoYoPlay `pre_download` branch |
| Honkai: Star Rail | `hkrpg_global` | same | same |
| Zenless Zone Zero | `nap_global` | same | same |
| Honkai Impact 3rd | `bh3_global` | never has LDiff, so always Sophon | `extra.preload` if published, else HoYoPlay `pre_download` branch |
| Wuthering Waves | `wuwa_global` | Kuro index (`index_file`, `res_list_url`) | none |

All HoYoverse games share one code path (`berry-downloads/src/sources/hoyo.rs`); nothing is
per-game. `extra.preload` is the primary source of a pre-download (its `game.diff` entry matching the installed
version, plus the full voice packs the user selected). A manifest with no `extra.preload` (absent, `null` or `{}`)
simply publishes no pre-download, and only then is HoYoPlay's own `pre_download` branch consulted. The unit tests in `hoyo.rs` cover those shapes without the network.

### Voice-over packs

Genshin Impact, Honkai: Star Rail and Zenless Zone Zero offer four voice-over packs (`en-us`,
`ja-jp`, `ko-kr`, `zh-cn`), listed in `berry-games/src/catalog.rs` (`GameCatalogEntry::voice_packs`).
Honkai Impact 3rd and the Kuro games offer none. Installing a game with voice packs first shows a
dialog with one switch per language (English on by default; all off is allowed and installs no
voice pack, so the game downloads its own default on first start). The choice is stored in
`<game>/.berry-voice-packs` (`berry-downloads/src/voice.rs`) and used by every later install,
resume, update, pre-download and repair; installs without that file keep English.
