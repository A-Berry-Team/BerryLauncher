use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, PoisonError};
use std::time::{Duration, Instant};

use berry_locale::fl;

#[derive(Debug, Clone, Copy, Default)]
pub struct DownloadProgress {
    pub downloaded: u64,
    pub total: u64,
    pub net_speed: u64,
    pub phase: u8,
}

impl DownloadProgress {
    pub fn fraction(&self) -> f64 {
        if self.total == 0 {
            0.0
        } else {
            (self.downloaded as f64 / self.total as f64).clamp(0.0, 1.0)
        }
    }

    pub fn phase_label(&self) -> String {
        match self.phase {
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

pub(super) fn set_progress(game_id: &str, progress: DownloadProgress) {
    let mut states = lock(download_states());
    // Called every ~500 ms per transfer: only allocate the key the first time.
    match states.get_mut(game_id) {
        Some(state) => state.progress = progress,
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

pub fn pause_game(game_id: &str) -> bool {
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
