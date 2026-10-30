//! Shared install / update / repair pipeline.
//!
//! The three user-facing actions only differ in (a) how the plan is resolved,
//! (b) which berry-installer entry point is called per job and (c) a few status strings.
//! Everything else (runtime, job loop, cancellation checks, progress plumbing,
//! metadata) lives here once.

use anyhow::{bail, Context, Result};
use berry_installer::kuro::{KuroInstaller, KuroJob};
use berry_installer::sophon::{SophonInstaller, SophonJob};
use berry_installer::{CancelToken, Http, JobContext, Phase};
use std::fmt;
use std::future::Future;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tracing::info;

use crate::sources::{self, Backend, DownloadJob, InstallPlan};
use berry_games::catalog::GameCatalogEntry;
use berry_locale::fl;

use super::details;
use super::installed_version;
use super::metadata::write_install_metadata;
use super::state::{set_progress, DownloadProgress, InstallEvent};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum OperationKind {
    /// Fresh install, resume, or update (the plan is chosen from the installed version).
    Install,
    Repair,
}

impl OperationKind {
    pub(super) fn name(self) -> &'static str {
        match self {
            Self::Install => "install",
            Self::Repair => "repair",
        }
    }

    fn fetching_status(self) -> String {
        match self {
            Self::Install => fl!("status-fetching-manifest"),
            Self::Repair => fl!("status-fetching-repair-manifest"),
        }
    }

    fn job_status(self, job: &DownloadJob, current: u64, total: u64) -> String {
        match self {
            Self::Install => {
                fl!(
                    "status-installing",
                    name = job.label.as_str(),
                    current = current,
                    total = total
                )
            }
            Self::Repair => {
                fl!(
                    "status-repairing",
                    name = job.label.as_str(),
                    current = current,
                    total = total
                )
            }
        }
    }

    fn resolve_plan(self, game: &GameCatalogEntry, destination: &Path) -> Result<InstallPlan> {
        match (self, installed_version(destination)) {
            (Self::Install, Some(version)) => sources::resolve_update(game, &version),
            _ => sources::resolve(game),
        }
    }
}

/// Typed marker for a user-requested stop, so callers never have to match on message text.
#[derive(Debug)]
pub(super) struct Cancelled(pub OperationKind);

impl fmt::Display for Cancelled {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} cancelled", self.0.name())
    }
}

impl std::error::Error for Cancelled {}

pub(super) fn is_cancelled_error(error: &anyhow::Error) -> bool {
    error.downcast_ref::<Cancelled>().is_some()
}

/// Status line for the progress panel; the details window keeps the last one too.
fn send_status(game_id: &str, sender: &async_channel::Sender<InstallEvent>, status: String) {
    details::record_status(game_id, &status);
    let _ = sender.send_blocking(InstallEvent::Status(status));
}

fn ensure_not_cancelled(kind: OperationKind, cancel: &AtomicBool) -> Result<()> {
    if cancel.load(Ordering::SeqCst) {
        return Err(Cancelled(kind).into());
    }
    Ok(())
}

pub(super) fn run(
    kind: OperationKind,
    game: &GameCatalogEntry,
    destination: &Path,
    cancel: &Arc<AtomicBool>,
    sender: &async_channel::Sender<InstallEvent>,
) -> Result<()> {
    send_status(game.id, sender, kind.fetching_status());
    ensure_not_cancelled(kind, cancel)?;
    let plan = kind.resolve_plan(game, destination)?;
    ensure_not_cancelled(kind, cancel)?;
    details::record_plan(
        game.id,
        &plan.version,
        match plan.backend {
            Backend::Sophon => "Sophon",
            Backend::Kuro => "Kuro",
        },
        plan.jobs.len(),
    );
    info!(
        "[{}] {} version {} via {:?}, {} download job(s)",
        kind.name(),
        game.id,
        plan.version,
        plan.backend,
        plan.jobs.len(),
    );
    std::fs::create_dir_all(destination)
        .with_context(|| format!("creating {}", destination.display()))?;

    let runtime = JobRuntime::new()?;
    // One HTTP client (and connection pool) for the whole operation.
    let http = Http::new(berry_core::http::user_agent()).map_err(|e| anyhow::anyhow!(e))?;
    let total_jobs = plan.jobs.len() as u64;
    for (index, job) in plan.jobs.iter().enumerate() {
        ensure_not_cancelled(kind, cancel)?;
        let current = index as u64 + 1;
        details::record_job(game.id, current as u32, total_jobs as u32, &job.label);
        send_status(game.id, sender, kind.job_status(job, current, total_jobs));
        info!(
            "[{}] job {current}/{total_jobs}: {}",
            kind.name(),
            job.label
        );
        let ok = runtime.block_on(run_job_until_cancelled(
            kind,
            game.id,
            &plan,
            job,
            destination,
            cancel,
            sender,
            &http,
        ));
        if !ok {
            ensure_not_cancelled(kind, cancel)?;
            bail!(
                "berry-installer failed while {} {}",
                kind.progressive(),
                job.label
            );
        }
    }
    // Every job succeeded: whatever is left in the staging folders (journals of voice packs no
    // longer in the plan, files from older launchers) would keep the game "unfinished".
    berry_installer::game::clear_staging(destination)?;
    // Only reached when every job succeeded; a failure here keeps the in-progress marker.
    write_install_metadata(game, destination, &plan.version)?;
    Ok(())
}

impl OperationKind {
    fn progressive(self) -> &'static str {
        match self {
            Self::Install => "installing",
            Self::Repair => "repairing",
        }
    }
}

const STOP_POLL: Duration = Duration::from_millis(50);
/// How long runtime teardown may wait for stragglers once an operation is over.
const TEARDOWN_GRACE: Duration = Duration::from_secs(2);

/// Owns the operation's tokio runtime and, when dropped, shuts it down with a deadline.
/// Dropping a runtime normally waits for every task to reach an await point; after a pause
/// or failure that is exactly the "keeps downloading for a few seconds" the user sees.
struct JobRuntime(Option<tokio::runtime::Runtime>);

impl JobRuntime {
    fn new() -> Result<Self> {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .context("creating async runtime")?;
        Ok(Self(Some(runtime)))
    }

    fn block_on<F: Future>(&self, future: F) -> F::Output {
        self.0
            .as_ref()
            .expect("runtime is alive until dropped")
            .block_on(future)
    }
}

impl Drop for JobRuntime {
    fn drop(&mut self) {
        if let Some(runtime) = self.0.take() {
            runtime.shutdown_timeout(TEARDOWN_GRACE);
        }
    }
}

/// Run a job, but stop waiting for it as soon as the user cancels. berry-installer checks the token
/// cooperatively, which can take long (hashing big partial files, retries, waiting for
/// permits). Dropping the job future here, then tearing the runtime down, bounds the stop
/// latency independently of what berry-installer is doing at that moment.
#[allow(clippy::too_many_arguments)]
async fn run_job_until_cancelled(
    kind: OperationKind,
    game_id: &str,
    plan: &InstallPlan,
    job: &DownloadJob,
    destination: &Path,
    cancel: &Arc<AtomicBool>,
    sender: &async_channel::Sender<InstallEvent>,
    http: &Http,
) -> bool {
    let critical = Arc::new(AtomicBool::new(false));
    let stop = async {
        loop {
            if cancel.load(Ordering::SeqCst) && !critical.load(Ordering::SeqCst) {
                break;
            }
            tokio::time::sleep(STOP_POLL).await;
        }
    };
    tokio::select! {
        ok = run_job(kind, game_id, plan, job, destination, cancel, sender, critical.clone(), http) => ok,
        () = stop => false,
    }
}

#[allow(clippy::too_many_arguments)]
async fn run_job(
    kind: OperationKind,
    game_id: &str,
    plan: &InstallPlan,
    job: &DownloadJob,
    destination: &Path,
    cancel: &Arc<AtomicBool>,
    sender: &async_channel::Sender<InstallEvent>,
    critical: Arc<AtomicBool>,
    http: &Http,
) -> bool {
    match plan.backend {
        Backend::Sophon => {
            run_sophon_job(
                kind,
                game_id,
                job,
                destination,
                cancel,
                sender,
                critical,
                http,
            )
            .await
        }
        Backend::Kuro => {
            run_kuro_job(
                kind,
                game_id,
                job,
                destination,
                cancel,
                sender,
                critical,
                http,
            )
            .await
        }
    }
}

/// Progress plumbing shared by both engines: converts berry-installer's `Progress` into the
/// values the UI and the download state use.
fn job_context(
    game_id: &str,
    cancel: &Arc<AtomicBool>,
    sender: &async_channel::Sender<InstallEvent>,
    critical: Arc<AtomicBool>,
) -> JobContext {
    let game_id = game_id.to_owned();
    let sender = sender.clone();
    JobContext::new()
        .with_cancel(CancelToken::from_flag(cancel.clone()))
        .with_progress(move |p| {
            critical.store(p.phase.is_critical(), Ordering::SeqCst);
            details::record_progress(&game_id, p);
            let progress = DownloadProgress {
                downloaded: p.current,
                total: p.total,
                net_speed: p.speed,
                phase: ui_phase(p.phase),
            };
            set_progress(&game_id, progress);
            let _ = sender.try_send(InstallEvent::Progress(progress));
        })
}

#[allow(clippy::too_many_arguments)]
async fn run_kuro_job(
    kind: OperationKind,
    game_id: &str,
    job: &DownloadJob,
    destination: &Path,
    cancel: &Arc<AtomicBool>,
    sender: &async_channel::Sender<InstallEvent>,
    critical: Arc<AtomicBool>,
    http: &Http,
) -> bool {
    let ctx = job_context(game_id, cancel, sender, critical);
    let kuro_job = KuroJob {
        index_url: job.manifest.clone(),
        base_url: job.base_url.clone(),
    };
    let installer = KuroInstaller::new(http.clone());
    let result = match kind {
        OperationKind::Install => installer.install(destination, &kuro_job, &ctx).await,
        OperationKind::Repair => installer.repair(destination, &kuro_job, &ctx).await,
    };
    match result {
        Ok(o) => {
            info!(
                "[{}] {}: {} file(s) downloaded, {} verified on disk, {} trusted, {} removed",
                kind.name(),
                job.label,
                o.files_fetched,
                o.files_verified,
                o.files_trusted,
                o.files_pruned,
            );
            true
        }
        Err(error) => {
            if !error.is_cancelled() {
                tracing::error!("[{}] {}: {error}", kind.name(), job.label);
            }
            false
        }
    }
}

/// Stable per-job name for the stored manifest / journal: `game`, or the voice-pack language.
fn sophon_slot(job: &DownloadJob) -> String {
    match job.label.strip_suffix(" voice pack") {
        Some(language) => language.to_owned(),
        None => "game".to_owned(),
    }
}

fn ui_phase(phase: Phase) -> u8 {
    match phase {
        Phase::Resolving => 0,
        Phase::Verifying => 1,
        Phase::Downloading => 2,
        Phase::Extracting => 3,
        Phase::Finalizing => 5,
    }
}

#[allow(clippy::too_many_arguments)]
async fn run_sophon_job(
    kind: OperationKind,
    game_id: &str,
    job: &DownloadJob,
    destination: &Path,
    cancel: &Arc<AtomicBool>,
    sender: &async_channel::Sender<InstallEvent>,
    critical: Arc<AtomicBool>,
    http: &Http,
) -> bool {
    let ctx = job_context(game_id, cancel, sender, critical);
    let sophon_job = SophonJob {
        slot: sophon_slot(job),
        manifest_url: job.manifest.clone(),
        chunk_base: job.base_url.clone(),
    };
    let installer = SophonInstaller::new(http.clone());
    let result = match kind {
        OperationKind::Install => installer.install(destination, &sophon_job, &ctx).await,
        OperationKind::Repair => installer.repair(destination, &sophon_job, &ctx).await,
    };
    match result {
        Ok(outcome) => {
            info!(
                "[{}] {}: {} chunk(s) downloaded, {} reused, {} from pre-download cache",
                kind.name(),
                job.label,
                outcome.chunks_fetched,
                outcome.chunks_reused,
                outcome.chunks_from_cache,
            );
            true
        }
        Err(error) => {
            if !error.is_cancelled() {
                tracing::error!("[{}] {}: {error}", kind.name(), job.label);
            }
            false
        }
    }
}
