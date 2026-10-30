use anyhow::{anyhow, Result};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Instant;
use tracing::{error, info, warn};

use berry_games::catalog::GameCatalogEntry;

use super::details::{self, DetailKind, Outcome};
use super::installed_version;
use super::metadata::{begin_install, finish_install};
use super::operation::{is_cancelled_error, run, OperationKind};
use super::state::{
    clear_state, register, set_paused, set_state, ActiveGuard, DownloadUiState, InstallEvent,
};

pub(super) fn start(
    game: GameCatalogEntry,
    destination: PathBuf,
    cancel: Arc<AtomicBool>,
    sender: async_channel::Sender<InstallEvent>,
    kind: OperationKind,
) {
    let game_id = game.id.to_string();
    // A game may have only one active operation. Never silently replace its token.
    let Some(guard) = register(&game_id, &cancel) else {
        let _ = sender.send_blocking(InstallEvent::Finished(Err(
            "an operation is already active for this game".to_owned(),
        )));
        return;
    };
    set_state(&game_id, DownloadUiState::default());
    // Same rule as `OperationKind::resolve_plan`: an install over an installed version is an update.
    let detail_kind = match kind {
        OperationKind::Repair => DetailKind::Repair,
        OperationKind::Install if installed_version(&destination).is_some() => DetailKind::Update,
        OperationKind::Install => DetailKind::Install,
    };
    details::begin(&game_id, detail_kind);

    let worker_sender = sender.clone();
    let spawned = std::thread::Builder::new()
        .name(format!("berry-{}-{}", kind.name(), game_id))
        .spawn(move || execute(game, destination, cancel, worker_sender, kind, guard));
    if let Err(error) = spawned {
        // The closure (and with it the registration guard) was dropped by `spawn`.
        clear_state(&game_id);
        details::finish(
            &game_id,
            Outcome::Failed(format!("could not start worker thread: {error}")),
        );
        let _ = sender.send_blocking(InstallEvent::Finished(Err(format!(
            "could not start worker thread: {error}"
        ))));
    }
}

fn execute(
    game: GameCatalogEntry,
    destination: PathBuf,
    cancel: Arc<AtomicBool>,
    sender: async_channel::Sender<InstallEvent>,
    kind: OperationKind,
    guard: ActiveGuard,
) {
    let started = Instant::now();
    info!("[{}] {} -> {}", kind.name(), game.id, destination.display());

    let result: Result<()> =
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| -> Result<()> {
            if kind == OperationKind::Install {
                begin_install(&destination)?;
            }
            run(kind, &game, &destination, &cancel, &sender)?;
            if kind == OperationKind::Install {
                finish_install(&destination)?;
            }
            Ok(())
        }))
        .unwrap_or_else(|payload| {
            Err(anyhow!(
                "worker aborted unexpectedly: {}",
                panic_message(&*payload)
            ))
        });

    let cancelled = cancel.load(Ordering::SeqCst);
    match &result {
        Ok(()) => {
            clear_state(game.id);
            details::finish(game.id, Outcome::Completed);
            info!(
                "[{}] {} finished in {:.0?}",
                kind.name(),
                game.id,
                started.elapsed()
            );
        }
        Err(error) if cancelled || is_cancelled_error(error) => {
            // Partial files and the in-progress marker stay on disk so the transfer can resume.
            set_paused(game.id, true);
            details::finish(game.id, Outcome::Paused);
            info!(
                "[{}] {} paused after {:.0?}",
                kind.name(),
                game.id,
                started.elapsed()
            );
        }
        Err(error) => {
            // Real failure: don't leave a phantom "paused" entry; the marker still allows a retry.
            clear_state(game.id);
            details::finish(game.id, Outcome::Failed(format!("{error:#}")));
            error!("[{}] {} failed: {error:#}", kind.name(), game.id);
        }
    }

    // Release the registration before notifying, so the UI never observes a finished
    // operation that still reports itself as active.
    drop(guard);
    if sender
        .send_blocking(InstallEvent::Finished(result.map_err(|e| format!("{e:#}"))))
        .is_err()
    {
        warn!("[{}] {}: UI receiver is gone", kind.name(), game.id);
    }
}

fn panic_message(payload: &(dyn std::any::Any + Send)) -> String {
    payload
        .downcast_ref::<&str>()
        .map(|s| (*s).to_owned())
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "unknown panic".to_owned())
}
