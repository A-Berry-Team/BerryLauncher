//! Links the synchronization choice (NTSync / FSync / ESync / none) to the game log:
//! the header says what was *selected*, and this watcher records what the runner announces
//! it *actually* uses once the game is starting.

use std::io::{Read, Seek, SeekFrom};
use std::path::PathBuf;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::time::{Duration, Instant};

use berry_games::library::SyncMode;

use super::{report, GameLauncher};

/// How long to wait for the runner to announce its synchronization primitive.
const WATCH_TIMEOUT: Duration = Duration::from_secs(90);

/// Looks for the lines Wine / Proton print when a synchronization primitive starts
/// (`fsync: up and running.`, `esync: up and running.`, and the ntsync equivalent).
/// Returns the most capable primitive announced in `text`.
pub(super) fn detect_announced_sync(text: &str) -> Option<SyncMode> {
    let mut found: Option<SyncMode> = None;
    for line in text.lines() {
        let line = line.to_ascii_lowercase();
        let announced = line.contains("up and running")
            || line.contains("is enabled")
            || line.contains("enabled.")
            || line.contains("in use")
            // Wine's own ntsync message: "wine: using fast synchronization."
            || line.contains("fast synchronization");
        if !announced {
            continue;
        }
        let mode = if line.contains("ntsync") || line.contains("fast synchronization") {
            SyncMode::NtSync
        } else if line.contains("fsync") {
            SyncMode::FSync
        } else if line.contains("esync") {
            SyncMode::ESync
        } else {
            continue;
        };
        // NtSync < FSync < ESync in the option list, so a lower index is the better primitive.
        if found.is_none_or(|current| mode.index() < current.index()) {
            found = Some(mode);
        }
    }
    found
}

impl GameLauncher {
    /// Starts a background watcher over the game log. `start_offset` skips the diagnostic header
    /// (which mentions ntsync/fsync/esync itself) so only the game's own output is inspected.
    pub(super) fn watch_sync_usage(
        log_path: PathBuf,
        start_offset: u64,
        selected: SyncMode,
        alive: Arc<AtomicBool>,
    ) {
        std::thread::spawn(move || {
            let deadline = Instant::now() + WATCH_TIMEOUT;
            let mut offset = start_offset;
            // Only text that has not been inspected yet: complete lines are dropped after each
            // scan, so the cost of a poll does not grow with the size of the log.
            let mut output = String::new();
            let mut delay = Duration::from_millis(250);
            let event = |message: &str| report::append_event(&log_path, "sync", message);

            while alive.load(Ordering::Acquire) && Instant::now() < deadline {
                if let Ok(mut file) = std::fs::File::open(&log_path) {
                    if file.seek(SeekFrom::Start(offset)).is_ok() {
                        let mut bytes = Vec::new();
                        if let Ok(read) = file.read_to_end(&mut bytes) {
                            offset += read as u64;
                            output.push_str(&String::from_utf8_lossy(&bytes));
                        }
                    }
                }
                if let Some(actual) = detect_announced_sync(&output) {
                    if actual == selected {
                        event(&format!(
                            "selected: {}; the game is actually using: {} (announced by the runner)",
                            selected.display(),
                            actual.display(),
                        ));
                    } else {
                        event(&format!(
                            "selected: {}; the game is actually using: {} (announced by the runner) \
                             - the runner fell back because {} is not available for it",
                            selected.display(),
                            actual.display(),
                            selected.display(),
                        ));
                    }
                    return;
                }
                // Lines already scanned announced nothing; keep only the unfinished last line.
                let keep_from = output.rfind('\n').map_or(0, |index| index + 1);
                output.drain(..keep_from);
                std::thread::sleep(delay);
                delay = (delay * 2).min(Duration::from_secs(2));
            }
            if alive.load(Ordering::Acquire) {
                event(&format!(
                    "selected: {}; the runner did not announce which primitive it uses within {} s \
                     (it may stay silent, or the wineserver of this prefix was already running)",
                    selected.display(),
                    WATCH_TIMEOUT.as_secs(),
                ));
            } else {
                event(&format!(
                    "selected: {}; the game ended before the runner announced a primitive",
                    selected.display(),
                ));
            }
        });
    }
}
