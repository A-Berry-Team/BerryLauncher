# Packaging notes

This page explains how to build Berry Launcher as a Flatpak: the everyday development build, the reproducible offline release build, and the distributable `.flatpak` bundle. It also lists the checks to run before you submit packaging changes.

## Everyday development (GNOME Builder)

Use `app.berrylauncher.berry.json` in the repository root. It builds with network access (Cargo downloads crates), which is fine for development.

## Reproducible release build (offline)

1. Regenerate the vendored Cargo sources:

   ```sh
   build-aux/update-cargo-sources.sh
   ```

   This needs network access once. It rebuilds `build-aux/flatpak/cargo-sources.json` from `Cargo.lock`. Commit the result with every `Cargo.lock` change. `berry-installer` is a path dependency (a git submodule), so it is not vendored here. Initialize submodules before building (`git submodule update --init`).

2. Build and install the release manifest:

   ```sh
   flatpak-builder --user --install --force-clean build-dir build-aux/flatpak/app.berrylauncher.berry.release.json
   ```

The release manifest builds with `-Dbuildtype=release -Doffline=true -Dlocked=true`. Cargo then uses only the vendored sources and refuses to change `Cargo.lock`.

## Distributable bundle (`.flatpak`)

Build the release manifest, export it to a local repository, and bundle it. You must always pass the branch explicitly. The manifest sets `"branch": "stable"`, but `flatpak build-export` and `flatpak build-bundle` do **not** read it. They fall back to `master`, and GNOME Software then labels the app *Beta*.

```sh
flatpak-builder --user --force-clean --ccache --repo=repo \
  build-dir build-aux/flatpak/app.berrylauncher.berry.release.json
flatpak build-bundle repo build-dir/berrylauncher.flatpak app.berrylauncher.berry stable
```

This creates the installable `build-dir/berrylauncher.flatpak` inside the project.

If you export with `flatpak build-export repo build-dir` instead of `--repo=repo`, pass the branch there too:

```sh
flatpak build-export repo build-dir stable
```

### Prerequisites

Install Flatpak, flatpak-builder, the GNOME 51 Platform and SDK, and the Rust stable SDK extension.

### Check the result

After you install the bundle, run:

```sh
flatpak list --app --columns=application,branch,origin
```

The branch must show `stable`. If an older `master` build is installed, uninstall it and delete the local `repo/` folder first. Otherwise, commits from both branches mix.

## Checks

- `meson test -C builddir` runs these checks:
  - desktop file validation
  - AppStream validation
  - GSettings schema validation
  - the version-consistency check (`Cargo.toml` = `meson.build` = newest `<release>`)
  - the bundled-icons check

  Here `builddir` is your Meson build directory. If you used `build`, as in the [README](../README.md#build), run `meson test -C build`.
- Before you submit, run `flatpak-builder-lint manifest <manifest>` and `flatpak-builder-lint appstream data/app.berrylauncher.berry.metainfo.xml.in`.

## Releasing

For the version-bump steps, see [Release a version](../docs/DEVELOPMENT.md#release-a-version).
