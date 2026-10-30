use anyhow::Result;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use berry_games::catalog::GameCatalogEntry;
pub use berry_installer::GameState;
mod details;
mod metadata;
mod operation;
pub mod sources;
mod state;
mod worker;

pub use details::{
    download_details, phase_index, phase_title, DetailKind, DownloadDetails, LogEntry, Outcome,
    PhaseStat, StepState, MAX_SAMPLES, PHASES,
};
pub use metadata::installed_version;
pub use state::{
    is_downloading, pause_game, shutdown, ui_state, DownloadProgress, DownloadUiState, InstallEvent,
};

use operation::OperationKind;

/// Complete and launchable / resumable / not installed, from one definition (see
/// [`berry_installer::game`]).
pub fn game_state(game: &GameCatalogEntry, destination: &Path) -> GameState {
    berry_installer::game::inspect(destination, game.is_installed(destination))
}

pub fn has_incomplete_download(destination: &Path) -> bool {
    berry_installer::game::unfinished_work(destination).is_some()
}

pub fn is_fully_downloaded(game: &GameCatalogEntry, destination: &Path) -> bool {
    game_state(game, destination).is_ready()
}

pub fn has_update(game: &GameCatalogEntry, destination: &Path) -> Result<bool> {
    if !is_fully_downloaded(game, destination) {
        return Ok(false);
    }
    let Some(installed) = installed_version(destination) else {
        return Ok(false);
    };
    sources::update_available(game, &installed)
}

/// Updates share the install pipeline: the plan is picked from the installed version.
pub fn update_game(
    game: GameCatalogEntry,
    destination: PathBuf,
    cancel: Arc<AtomicBool>,
    sender: async_channel::Sender<InstallEvent>,
) {
    install_game(game, destination, cancel, sender);
}

pub fn repair_game(
    game: GameCatalogEntry,
    destination: PathBuf,
    cancel: Arc<AtomicBool>,
    sender: async_channel::Sender<InstallEvent>,
) {
    worker::start(game, destination, cancel, sender, OperationKind::Repair);
}

pub fn format_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    format!("{value:.1} {}", UNITS[unit])
}

pub fn install_game(
    game: GameCatalogEntry,
    destination: PathBuf,
    cancel: Arc<AtomicBool>,
    sender: async_channel::Sender<InstallEvent>,
) {
    worker::start(game, destination, cancel, sender, OperationKind::Install);
}
