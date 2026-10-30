use anyhow::Result;
use std::path::Path;

use berry_games::catalog::{GameCatalogEntry, GameSource};

pub use berry_installer::game::installed_version;
use berry_installer::game::{self, write_version};

pub(super) fn hoyo_config_ini(version: &str) -> String {
    format!("[general]\nchannel=1\ncps=hoyoverse_PC\ngame_version={version}\nsub_channel=0\n")
}

/// Persist the installed version. Errors are propagated so a half-written install is never
/// reported as complete (the caller keeps the in-progress marker on failure).
pub(super) fn write_install_metadata(
    game: &GameCatalogEntry,
    destination: &Path,
    version: &str,
) -> Result<()> {
    if matches!(game.source, GameSource::Hoyo { .. }) {
        berry_installer::fsutil::atomic_write(
            &destination.join("config.ini"),
            hoyo_config_ini(version).as_bytes(),
        )?;
    }
    // Written last: it takes precedence in `installed_version`.
    write_version(destination, version)?;
    Ok(())
}

/// Flag an install as in progress. Its presence after a crash makes the game resumable.
pub(super) fn begin_install(destination: &Path) -> Result<()> {
    Ok(game::begin_install(destination)?)
}

pub(super) fn finish_install(destination: &Path) -> Result<()> {
    Ok(game::finish_install(destination)?)
}
