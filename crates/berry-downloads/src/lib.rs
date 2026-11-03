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
mod voice;
mod worker;

pub use details::{
    download_details, phase_index, phase_title, DetailKind, DownloadDetails, LogEntry, Outcome,
    PhaseStat, StepState, MAX_SAMPLES, PHASES,
};
pub use metadata::installed_version;
pub use state::{
    is_downloading, pause_game, shutdown, ui_state, DownloadProgress, DownloadUiState, InstallEvent,
};
pub use voice::{save_voice_packs, selected_voice_packs};

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

/// Version of the HoYoverse pre-download currently published for the installed game, or `None`
/// when there is none (also when it is already installed or already live). Needs the network.
pub fn available_preload(game: &GameCatalogEntry, destination: &Path) -> Result<Option<String>> {
    if !is_fully_downloaded(game, destination) {
        return Ok(None);
    }
    let Some(installed) = installed_version(destination) else {
        return Ok(None);
    };
    sources::preload_version(game, &installed)
}

/// Version of the pre-download already downloaded and waiting to be applied, if any.
pub fn cached_preload_version(game_dir: &Path) -> Option<String> {
    berry_installer::ldiff::cached_preload_version(game_dir)
}

/// Download the current HoYoverse pre-download into Sophon's per-component cache.
/// The installed game and its version metadata are left untouched.
pub fn preload_game(
    game: GameCatalogEntry,
    destination: PathBuf,
    cancel: Arc<AtomicBool>,
    sender: async_channel::Sender<InstallEvent>,
) {
    worker::start(game, destination, cancel, sender, OperationKind::Preload);
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

/// Install a game with exactly the voice-over packs in `voice_packs` (codes of
/// [`GameCatalogEntry::voice_packs`]; none at all is valid and lets the game download its own
/// default the first time it runs). The choice is remembered with the game, so updates,
/// pre-downloads and repairs keep using it.
pub fn install_game_with_voice_packs(
    game: GameCatalogEntry,
    destination: PathBuf,
    voice_packs: &[String],
    cancel: Arc<AtomicBool>,
    sender: async_channel::Sender<InstallEvent>,
) {
    if let Err(error) = save_voice_packs(&game, &destination, voice_packs) {
        let _ = sender.send_blocking(InstallEvent::Finished(Err(format!(
            "could not save the voice-over selection: {error:#}"
        ))));
        return;
    }
    install_game(game, destination, cancel, sender);
}
