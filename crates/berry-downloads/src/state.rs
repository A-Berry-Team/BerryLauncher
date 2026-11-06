use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, PoisonError};
use std::time::{Duration, Instant};

use berry_locale::fl;

/// [`DownloadProgress::phase`] while HDiff / LDiff patches are applied to the game files.
pub const PATCHING_PHASE: u8 = 6;

#[derive(Debug, Clone, Copy, Default)]
pub struct DownloadProgress {
    pub downloaded: u64,
    pub total: u64,
    pub net_speed: u64,
    pub phase: u8,
    /// While patching: 1-based patch being applied and how many there are (`0` until known).
    pub patch_index: u32,
    pub patch_total: u32,
}

impl DownloadProgress {
    pub fn fraction(&self) -> f64 {
        if self.total == 0 {
            0.0
        } else {
            (self.downloaded as f64 / self.total as f64).clamp(0.0, 1.0)
        }
    }

    /// Applying patches: local work that cannot be paused, unlike a transfer.
    pub fn is_patching(&self) -> bool {
        self.phase == PATCHING_PHASE
    }

    pub fn phase_label(&self) -> String {
        match self.phase {
            PATCHING_PHASE => fl!("progress-patching"),
            1 => fl!("progress-verifying"),
            2 => fl!("progress-downloading"),
            3 => fl!("progress-installing"),
            4 => fl!("progress-validating"),
            5 => fl!("progress-finishing"),
            _ => fl!("progress-preparing"),
        }
    }
}

#[derive(Debug)]
pub enum InstallEvent {
    Status(String),
    Progress(DownloadProgress),
    Finished(std::result::Result<(), String>),
}

#[derive(Debug, Clone, Copy, Default)]
pub struct DownloadUiState {
    pub progress: DownloadProgress,
    pub paused: bool,
    /// The resumable operation is a HoYoverse pre-download, not an install/update.
    pub preload: bool,
}

type ActiveMap = HashMap<String, Arc<AtomicBool>>;

static ACTIVE_DOWNLOADS: OnceLock<Mutex<ActiveMap>> = OnceLock::new();
static DOWNLOAD_STATES: OnceLock<Mutex<HashMap<String, DownloadUiState>>> = OnceLock::new();

/// Lock a mutex, recovering the data if a panicking thread poisoned it. The maps only hold
/// plain values, so they are always consistent and one panic must not wedge every later call.
fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

fn active_downloads() -> &'static Mutex<ActiveMap> {
    ACTIVE_DOWNLOADS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn download_states() -> &'static Mutex<HashMap<String, DownloadUiState>> {
    DOWNLOAD_STATES.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Registration of a running operation. Dropping it (normally, on panic, or when the thread
/// could not be spawned) removes only this operation's entry, never a newer one.
pub(super) struct ActiveGuard {
    game_id: String,
    token: Arc<AtomicBool>,
}

impl Drop for ActiveGuard {
    fn drop(&mut self) {
        let mut active = lock(active_downloads());
        if active
            .get(&self.game_id)
            .is_some_and(|t| Arc::ptr_eq(t, &self.token))
        {
            active.remove(&self.game_id);
        }
    }
}

/// Claim the game for an operation; `None` if one is already running.
pub(super) fn register(game_id: &str, token: &Arc<AtomicBool>) -> Option<ActiveGuard> {
    let mut active = lock(active_downloads());
    if active.contains_key(game_id) {
        return None;
    }
    active.insert(game_id.to_owned(), token.clone());
    Some(ActiveGuard {
        game_id: game_id.to_owned(),
        token: token.clone(),
    })
}

pub fn ui_state(game_id: &str) -> Option<DownloadUiState> {
    lock(download_states()).get(game_id).copied()
}

pub(super) fn set_state(game_id: &str, state: DownloadUiState) {
    lock(download_states()).insert(game_id.to_owned(), state);
}

pub(super) fn set_progress(game_id: &str, mut progress: DownloadProgress) {
    let mut states = lock(download_states());
    // Called every ~500 ms per transfer: only allocate the key the first time.
    match states.get_mut(game_id) {
        Some(state) => {
            // Patch counts come from their own events (see `set_patch`): a plain progress
            // report must not wipe them while patching, and must not leak them afterwards.
            if progress.is_patching() {
                progress.patch_index = state.progress.patch_index;
                progress.patch_total = state.progress.patch_total;
            }
            state.progress = progress;
        }
        None => {
            states.insert(
                game_id.to_owned(),
                DownloadUiState {
                    progress,
                    ..Default::default()
                },
            );
        }
    }
}

/// Which patch is being applied (`0, 0` when a patch run starts).
pub(super) fn set_patch(game_id: &str, index: u32, total: u32) {
    let mut states = lock(download_states());
    let state = states.entry(game_id.to_owned()).or_default();
    state.progress.patch_index = index;
    state.progress.patch_total = total;
}

pub(super) fn set_paused(game_id: &str, paused: bool) {
    lock(download_states())
        .entry(game_id.to_owned())
        .or_default()
        .paused = paused;
}

pub(super) fn clear_state(game_id: &str) {
    lock(download_states()).remove(game_id);
}

pub fn is_downloading(game_id: &str) -> bool {
    lock(active_downloads()).contains_key(game_id)
}

/// The game's operation is applying patches right now. That step is not a download: it cannot be
/// paused, and closing the launcher during it deserves its own warning.
pub fn is_patching(game_id: &str) -> bool {
    is_downloading(game_id)
        && lock(download_states())
            .get(game_id)
            .is_some_and(|state| !state.paused && state.progress.is_patching())
}

pub fn pause_game(game_id: &str) -> bool {
    if is_patching(game_id) {
        // Patches are applied without looking at the stop flag: asking would only make the
        // window claim a pause that does not happen.
        tracing::info!("[pause] ignored for {game_id}: patches are being applied");
        return false;
    }
    let token = lock(active_downloads()).get(game_id).cloned();
    match token {
        Some(token) => {
            // The worker logs "paused after ..." when it really stops; the gap is the stop latency.
            tracing::info!("[pause] requested for {game_id}");
            token.store(true, Ordering::SeqCst);
            set_paused(game_id, true);
            true
        }
        None => false,
    }
}

/// Ask every running operation to stop and wait (up to `timeout`) for the workers to
/// leave their files in a resumable state. Call when the application shuts down.
pub fn shutdown(timeout: Duration) {
    for token in lock(active_downloads()).values() {
        token.store(true, Ordering::SeqCst);
    }
    let deadline = Instant::now() + timeout;
    while !lock(active_downloads()).is_empty() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(50));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn progress_reports_do_not_wipe_the_patch_counter_but_other_phases_clear_it() {
        let id = "test-patch-counter";
        let report = |phase| DownloadProgress {
            phase,
            ..Default::default()
        };
        set_progress(id, report(PATCHING_PHASE));
        set_patch(id, 668, 1177);
        set_progress(id, report(PATCHING_PHASE));
        let progress = ui_state(id).unwrap().progress;
        assert!(progress.is_patching());
        assert_eq!((progress.patch_index, progress.patch_total), (668, 1177));

        set_progress(id, report(2));
        let progress = ui_state(id).unwrap().progress;
        assert!(!progress.is_patching());
        assert_eq!((progress.patch_index, progress.patch_total), (0, 0));
        clear_state(id);
    }

    #[test]
    fn a_game_with_no_operation_is_never_patching() {
        assert!(!is_patching("test-no-operation"));
    }
}
