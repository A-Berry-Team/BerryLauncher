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

use crate::sources::{self, Backend, DownloadJob, DownloadProtocol, InstallPlan};
use berry_games::catalog::GameCatalogEntry;
use berry_locale::fl;

use super::details;
use super::installed_version;
use super::metadata::write_install_metadata;
use super::state::{
    set_patch, set_progress, ui_state, DownloadProgress, InstallEvent, PATCHING_PHASE,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum OperationKind {
    /// Fresh install, resume, or update (the plan is chosen from the installed version).
    Install,
    Repair,
    Preload,
}

impl OperationKind {
    pub(super) fn name(self) -> &'static str {
        match self {
            Self::Install => "install",
            Self::Repair => "repair",
            Self::Preload => "preload",
        }
    }

    fn fetching_status(self) -> String {
        match self {
            Self::Install => fl!("status-fetching-manifest"),
            Self::Repair => fl!("status-fetching-repair-manifest"),
            Self::Preload => "Resolving HoYoverse pre-download".to_owned(),
        }
    }

    /// Status line while job `current` of `total` is being fetched. An update words it as
    /// "Downloading" (the patch step has its own line, see [`patch_status`]); a fresh install keeps
    /// "Installing".
    fn job_status(self, updating: bool, job: &DownloadJob, current: u64, total: u64) -> String {
        match self {
            Self::Install if updating => fl!(
                "status-downloading",
                name = job.label.as_str(),
                current = current,
                total = total
            ),
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
            Self::Preload => format!("Pre-downloading {} ({current}/{total})", job.label),
        }
    }

    fn resolve_plan(
        self,
        game: &GameCatalogEntry,
        destination: &Path,
        voices: &[String],
    ) -> Result<InstallPlan> {
        match self {
            Self::Preload => {
                let plan = sources::resolve_preload(
                    game,
                    installed_version(destination).as_deref(),
                    voices,
                )?
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "no pre-download is published for the installed version in Berry's game manifests"
                    )
                })?;
                // Never fall back to downloading the full game: a pre-download is an HDiff or nothing.
                if !sources::is_hdiff_preload(&plan) {
                    bail!("the published pre-download has no HDiff for the installed version");
                }
                Ok(plan)
            }
            Self::Install => match installed_version(destination) {
                Some(version) => sources::resolve_update(game, &version, voices),
                None => sources::resolve(game, voices),
            },
            Self::Repair => sources::resolve(game, voices),
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

/// Status line while the LDiff patches of job `current` of `total` are applied. It shares the
/// job's position with its download line, so the details read "Downloading game data (1/3)",
/// "Patching game data (1/3)", "Downloading ja-jp voice pack (2/3)", ...
fn patch_status(job: &DownloadJob, current: u64, total: u64) -> String {
    fl!(
        "status-patching",
        name = job.label.as_str(),
        current = current,
        total = total
    )
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
    // The voice-over packs chosen when the game was installed (English for older installs).
    let voices = super::selected_voice_packs(game, destination);
    if !game.voice_packs().is_empty() {
        info!(
            "[{}] {}: voice-over packs: {}",
            kind.name(),
            game.id,
            if voices.is_empty() {
                "none (the game picks its own)".to_owned()
            } else {
                voices.join(", ")
            }
        );
    }
    let plan = kind.resolve_plan(game, destination, &voices)?;
    ensure_not_cancelled(kind, cancel)?;
    let backend_label = if plan
        .jobs
        .iter()
        .any(|job| job.protocol == DownloadProtocol::Ldiff)
    {
        "Sophon + LDiff"
    } else {
        match plan.backend {
            Backend::Sophon => "Sophon",
            Backend::Kuro => "Kuro",
        }
    };
    details::record_plan(game.id, &plan.version, backend_label, plan.jobs.len());
    info!(
        "[{}] {} version {} via {}, {} download job(s)",
        kind.name(),
        game.id,
        plan.version,
        backend_label,
        plan.jobs.len(),
    );
    std::fs::create_dir_all(destination)
        .with_context(|| format!("creating {}", destination.display()))?;
    if kind == OperationKind::Preload {
        // A "complete" marker left by an earlier pre-download (another version, or one that is
        // being redone) must not survive while this one is incomplete.
        berry_installer::ldiff::begin_preload(destination, &plan.version)
            .map_err(|error| anyhow::anyhow!(error))?;
    }

    let runtime = JobRuntime::new()?;
    // One HTTP client (and connection pool) for the whole operation.
    let http = Http::new(berry_core::http::user_agent()).map_err(|e| anyhow::anyhow!(e))?;
    let total_jobs = plan.jobs.len() as u64;
    // An update over an installed version words its steps as downloading / patching.
    let updating = kind == OperationKind::Install && installed_version(destination).is_some();
    // Jobs run strictly one after the other, and an LDiff job is patched as soon as it has been
    // downloaded, before the next job downloads anything: patching always takes priority over
    // fetching more content. A later job failing can therefore never undo or invalidate the work
    // of an earlier one (the game stays patched when only a voice pack needs its fallback).
    //
    // The only operation that downloads without patching is the HoYoverse pre-download. The update
    // that follows it finds every LDiff job already in the cache (`ldiff_job_is_cached`), so it
    // skips the downloads and applies the patches straight away.
    for (index, job) in plan.jobs.iter().enumerate() {
        ensure_not_cancelled(kind, cancel)?;
        let current = index as u64 + 1;
        details::record_job(game.id, current as u32, total_jobs as u32, &job.label);
        info!(
            "[{}] job {current}/{total_jobs}: {}",
            kind.name(),
            job.label
        );
        let patches_here =
            kind == OperationKind::Install && job.protocol == DownloadProtocol::Ldiff;
        let already_downloaded = patches_here && ldiff_job_is_cached(destination, job);
        if already_downloaded {
            info!(
                "[install] {}: patches already downloaded (pre-download), applying them without downloading",
                job.label
            );
            details::record_note(
                game.id,
                fl!("details-log-cached", name = job.label.as_str()),
            );
        } else {
            send_status(
                game.id,
                sender,
                kind.job_status(updating, job, current, total_jobs),
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
                if patches_here {
                    tracing::warn!(
                        "[install] LDiff download failed for {}; using the full Sophon route",
                        job.label
                    );
                    let recovery = LdiffRecovery {
                        game,
                        destination,
                        voices: &voices,
                        cancel,
                        sender,
                        runtime: &runtime,
                        http: &http,
                        current,
                        total: total_jobs,
                    };
                    if recovery.run(job)? == Recovered::WholePlan {
                        break;
                    }
                    continue;
                }
                bail!(
                    "berry-installer failed while {} {}",
                    kind.progressive(),
                    job.label
                );
            }
        }
        if patches_here {
            ensure_not_cancelled(kind, cancel)?;
            send_status(game.id, sender, patch_status(job, current, total_jobs));
            match apply_ldiff_job(&runtime, game.id, cancel, sender, destination, job) {
                Ok(applied) => {
                    info!("[install] {}: applied {applied} LDiff patch(es)", job.label);
                }
                Err(error) => {
                    tracing::warn!(
                        "[install] LDiff application failed for {} ({error:#}); using the full Sophon route",
                        job.label
                    );
                    let recovery = LdiffRecovery {
                        game,
                        destination,
                        voices: &voices,
                        cancel,
                        sender,
                        runtime: &runtime,
                        http: &http,
                        current,
                        total: total_jobs,
                    };
                    if recovery.run(job)? == Recovered::WholePlan {
                        break;
                    }
                }
            }
        }
    }
    if kind != OperationKind::Preload {
        // Every job succeeded: clear leftover backend journals and stale temporary files.
        berry_installer::game::clear_staging(destination)?;
        // Whatever route the update took (LDiff, Sophon, fallback), the cached pre-download
        // belongs to a version that is now installed or superseded. Leaving it would hide the
        // button of the next pre-download. A repair keeps the installed version, so it keeps it.
        if kind == OperationKind::Install {
            berry_installer::ldiff::clear_preload_cache(destination)
                .map_err(|error| anyhow::anyhow!(error))?;
        }
        // Only reached when every job succeeded; a failure here keeps the in-progress marker.
        write_install_metadata(game, destination, &plan.version)?;
    }
    if kind == OperationKind::Preload {
        berry_installer::ldiff::mark_preload_complete(destination, &plan.version)
            .map_err(|error| anyhow::anyhow!(error))?;
    }
    Ok(())
}

/// Language code of a voice-pack job (`None` for the game itself).
fn voice_code(job: &DownloadJob) -> Option<&str> {
    job.label.strip_suffix(" voice pack")
}

/// Whether the patches of an LDiff job are already on disk, typically because the game was
/// pre-downloaded. Answered locally: nothing is requested from the network.
fn ldiff_job_is_cached(destination: &Path, job: &DownloadJob) -> bool {
    job.from_version.as_deref().is_some_and(|from_version| {
        berry_installer::ldiff::is_cached_for_url(destination, &job.manifest, from_version)
    })
}

/// How far a recovery from a failed LDiff job reached.
#[derive(Debug, PartialEq, Eq)]
enum Recovered {
    /// Only that voice pack was redone through its full manifest; later jobs still have to run.
    VoicePack,
    /// The whole plan was redone through the full Sophon manifests; nothing is left to run.
    WholePlan,
}

/// What a failed LDiff job (its download or its patching) needs to fall back on the full Sophon
/// route. A voice pack falls back on its own, so the game keeps whatever was already patched; a
/// failure of the game's own LDiff redoes everything (voice packs included) through Sophon.
struct LdiffRecovery<'a> {
    game: &'a GameCatalogEntry,
    destination: &'a Path,
    voices: &'a [String],
    cancel: &'a Arc<AtomicBool>,
    sender: &'a async_channel::Sender<InstallEvent>,
    runtime: &'a JobRuntime,
    http: &'a Http,
    /// Position of the failed job in the plan.
    current: u64,
    total: u64,
}

impl LdiffRecovery<'_> {
    fn run(&self, job: &DownloadJob) -> Result<Recovered> {
        match voice_code(job) {
            Some(code) => {
                run_voice_sophon_fallback(
                    self.game,
                    self.destination,
                    code,
                    (self.current, self.total),
                    self.cancel,
                    self.sender,
                    self.runtime,
                    self.http,
                )?;
                Ok(Recovered::VoicePack)
            }
            None => {
                run_sophon_fallback(
                    self.game,
                    self.destination,
                    self.voices,
                    self.cancel,
                    self.sender,
                    self.runtime,
                    self.http,
                )?;
                Ok(Recovered::WholePlan)
            }
        }
    }
}

/// Apply one downloaded LDiff job from the cache (a download of this same operation, or an earlier
/// pre-download). Any failure is an `Err` so the caller can pick the matching fallback.
fn apply_ldiff_job(
    runtime: &JobRuntime,
    game_id: &str,
    cancel: &Arc<AtomicBool>,
    sender: &async_channel::Sender<InstallEvent>,
    destination: &Path,
    job: &DownloadJob,
) -> Result<u64> {
    let from_version = job
        .from_version
        .clone()
        .ok_or_else(|| anyhow::anyhow!("LDiff update job is missing installed version"))?;
    let critical = Arc::new(AtomicBool::new(false));
    // Every patch the installer announces becomes one step of the details window's patch graph
    // and updates the "patch N of M" counter of the progress panel.
    let patch_game_id = game_id.to_owned();
    let patch_sender = sender.clone();
    let ctx =
        job_context(game_id, cancel, sender, critical.clone()).with_patch_progress(move |step| {
            details::record_patch(&patch_game_id, step);
            set_patch(&patch_game_id, step.index, step.total);
            if let Some(state) = ui_state(&patch_game_id) {
                let _ = patch_sender.try_send(InstallEvent::Progress(state.progress));
            }
        });
    // A new patch run (the game, then each voice pack) starts counting from zero.
    set_patch(game_id, 0, 0);
    ctx.reporter
        .report_now(berry_installer::progress::Progress::new(
            Phase::Patching,
            0,
            0,
        ));
    critical.store(true, Ordering::SeqCst);
    let destination_owned = destination.to_path_buf();
    let manifest_url = job.manifest.clone();
    let apply_ctx = ctx.clone();
    let result = run_blocking(runtime, move || {
        berry_installer::ldiff::apply_cached_manifest_for_url_with(
            &destination_owned,
            &manifest_url,
            &from_version,
            &apply_ctx,
        )
    })
    .context("waiting for LDiff patch worker");
    critical.store(false, Ordering::SeqCst);
    result?.map_err(|error| anyhow::anyhow!(error))
}

/// Download one voice pack through its full Sophon manifest. Sophon verifies what is already on
/// disk and only fetches missing chunks, so this never redownloads an installed voice pack.
#[allow(clippy::too_many_arguments)]
fn run_voice_sophon_fallback(
    game: &GameCatalogEntry,
    destination: &Path,
    code: &str,
    position: (u64, u64),
    cancel: &Arc<AtomicBool>,
    sender: &async_channel::Sender<InstallEvent>,
    runtime: &JobRuntime,
    http: &Http,
) -> Result<()> {
    let full = sources::resolve(game, &[code.to_owned()])
        .with_context(|| format!("resolving the full {code} voice pack"))?;
    let job = full
        .jobs
        .iter()
        .find(|job| voice_code(job) == Some(code) && job.protocol == DownloadProtocol::Sophon)
        .ok_or_else(|| anyhow::anyhow!("no full Sophon manifest for the {code} voice pack"))?;
    let (current, total) = position;
    // Same position as the LDiff job it replaces, so the details keep naming the right job.
    details::record_job(game.id, current as u32, total as u32, &job.label);
    send_status(
        game.id,
        sender,
        format!("Fallback: installing {} ({current}/{total})", job.label),
    );
    let ok = runtime.block_on(run_job_until_cancelled(
        OperationKind::Install,
        game.id,
        &full,
        job,
        destination,
        cancel,
        sender,
        http,
    ));
    if !ok {
        ensure_not_cancelled(OperationKind::Install, cancel)?;
        bail!("full Sophon fallback failed for {}", job.label);
    }
    Ok(())
}

/// Run the regular full Sophon plan when an incremental LDiff route cannot be downloaded
/// or applied. Sophon verifies existing files/chunks, so this is safe even if some LDiff
/// files were already patched before a later patch failed.
fn run_sophon_fallback(
    game: &GameCatalogEntry,
    destination: &Path,
    voices: &[String],
    cancel: &Arc<AtomicBool>,
    sender: &async_channel::Sender<InstallEvent>,
    runtime: &JobRuntime,
    http: &Http,
) -> Result<()> {
    let fallback = sources::resolve(game, voices).context("resolving full Sophon fallback")?;
    if fallback
        .jobs
        .iter()
        .any(|job| job.protocol != DownloadProtocol::Sophon)
    {
        bail!("full fallback unexpectedly contains a non-Sophon job");
    }
    info!(
        "[install] using full Sophon fallback: {} job(s)",
        fallback.jobs.len()
    );
    details::record_plan(
        game.id,
        &fallback.version,
        "Sophon fallback",
        fallback.jobs.len(),
    );
    for (index, job) in fallback.jobs.iter().enumerate() {
        ensure_not_cancelled(OperationKind::Install, cancel)?;
        let current = index as u64 + 1;
        details::record_job(
            game.id,
            current as u32,
            fallback.jobs.len() as u32,
            &job.label,
        );
        send_status(
            game.id,
            sender,
            format!(
                "Fallback: installing {} ({current}/{})",
                job.label,
                fallback.jobs.len()
            ),
        );
        let ok = runtime.block_on(run_job_until_cancelled(
            OperationKind::Install,
            game.id,
            &fallback,
            job,
            destination,
            cancel,
            sender,
            http,
        ));
        if !ok {
            ensure_not_cancelled(OperationKind::Install, cancel)?;
            bail!("full Sophon fallback failed for {}", job.label);
        }
    }
    berry_installer::ldiff::clear_preload_cache(destination)
        .map_err(|error| anyhow::anyhow!(error))?;
    Ok(())
}

impl OperationKind {
    fn progressive(self) -> &'static str {
        match self {
            Self::Install => "installing",
            Self::Repair => "repairing",
            Self::Preload => "pre-downloading",
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

/// Run blocking `work` on `runtime`'s blocking pool and wait for it.
///
/// `tokio::task::spawn_blocking` needs a runtime context, which exists only once `block_on` is
/// running its future; calling it as the argument of `block_on` panics ("there is no reactor
/// running"), so the call lives inside an `async` block.
fn run_blocking<T, F>(
    runtime: &JobRuntime,
    work: F,
) -> std::result::Result<T, tokio::task::JoinError>
where
    T: Send + 'static,
    F: FnOnce() -> T + Send + 'static,
{
    runtime.block_on(async move { tokio::task::spawn_blocking(work).await })
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
    if job.protocol == DownloadProtocol::Ldiff {
        return run_ldiff_job(
            kind,
            game_id,
            job,
            destination,
            cancel,
            sender,
            critical,
            http,
        )
        .await;
    }
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
                ..Default::default()
            };
            set_progress(&game_id, progress);
            // The stored copy also carries the patch counts, which come from their own events.
            let progress = ui_state(&game_id).map_or(progress, |state| state.progress);
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
        OperationKind::Preload => {
            tracing::warn!("[preload] Kuro pre-download is not implemented");
            return false;
        }
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

#[allow(clippy::too_many_arguments)]
async fn run_ldiff_job(
    kind: OperationKind,
    game_id: &str,
    job: &DownloadJob,
    destination: &Path,
    cancel: &Arc<AtomicBool>,
    sender: &async_channel::Sender<InstallEvent>,
    critical: Arc<AtomicBool>,
    http: &Http,
) -> bool {
    if !matches!(kind, OperationKind::Preload | OperationKind::Install) {
        tracing::error!(
            "[{}] {}: LDiff is not supported for this operation",
            kind.name(),
            job.label
        );
        return false;
    }
    let Some(from_version) = job.from_version.as_deref() else {
        tracing::error!(
            "[{}] {}: LDiff job is missing installed version",
            kind.name(),
            job.label
        );
        return false;
    };
    let ctx = job_context(game_id, cancel, sender, critical);
    match berry_installer::ldiff::preload_manifest(
        http,
        &job.manifest,
        &job.base_url,
        destination,
        from_version,
        &ctx,
    )
    .await
    {
        Ok(outcome) => {
            info!(
                "[{}] {}: {} LDiff patch range(s) downloaded ({}), {} reused ({}), {} total from installed version {}",
                kind.name(),
                job.label,
                outcome.patches_downloaded,
                berry_installer::error::format_bytes(outcome.bytes_downloaded),
                outcome.patches_reused,
                berry_installer::error::format_bytes(outcome.bytes_reused),
                outcome.patches_total,
                from_version,
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
        Phase::Patching => PATCHING_PHASE,
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
        OperationKind::Preload => installer.preload(destination, &sophon_job, &ctx).await,
    };
    match result {
        Ok(outcome) => {
            info!(
                "[{}] {}: {} chunk(s) downloaded ({}), {} reused, {} from pre-download cache ({}); {} missing before download",
                kind.name(),
                job.label,
                outcome.chunks_fetched,
                berry_installer::error::format_bytes(outcome.bytes_fetched),
                outcome.chunks_reused,
                outcome.chunks_from_cache,
                berry_installer::error::format_bytes(outcome.bytes_missing),
                outcome.chunks_missing,
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

#[cfg(test)]
mod blocking_tests {
    use super::*;

    #[test]
    fn blocking_work_runs_from_a_plain_thread() {
        // The LDiff patch step runs on the install worker thread, outside any Tokio context.
        let runtime = JobRuntime::new().unwrap();
        assert_eq!(run_blocking(&runtime, || 40 + 2).unwrap(), 42);
    }
}

#[cfg(test)]
mod status_tests {
    use super::*;

    fn job(label: &str, protocol: DownloadProtocol) -> DownloadJob {
        DownloadJob {
            label: label.to_owned(),
            manifest: "https://cdn.example/manifest".to_owned(),
            base_url: "https://cdn.example".to_owned(),
            protocol,
            from_version: Some("5.0.0".to_owned()),
        }
    }

    #[test]
    fn an_update_names_the_download_and_the_patch_of_each_job_with_the_same_position() {
        let game = job("game data", DownloadProtocol::Ldiff);
        let voice = job("ja-jp voice pack", DownloadProtocol::Ldiff);

        let downloading = OperationKind::Install.job_status(true, &game, 1, 2);
        let patching = patch_status(&game, 1, 2);
        assert!(downloading.contains("game data") && downloading.contains("(1/2)"));
        assert!(patching.contains("game data") && patching.contains("(1/2)"));
        assert_ne!(downloading, patching);

        let voice_downloading = OperationKind::Install.job_status(true, &voice, 2, 2);
        let voice_patching = patch_status(&voice, 2, 2);
        assert!(
            voice_downloading.contains("ja-jp voice pack") && voice_downloading.contains("(2/2)")
        );
        assert!(voice_patching.contains("ja-jp voice pack") && voice_patching.contains("(2/2)"));
    }

    #[test]
    fn a_fresh_install_keeps_its_own_wording() {
        let game = job("game data", DownloadProtocol::Sophon);
        assert_ne!(
            OperationKind::Install.job_status(false, &game, 1, 1),
            OperationKind::Install.job_status(true, &game, 1, 1)
        );
    }

    #[test]
    fn ldiff_jobs_without_a_cache_are_not_reported_as_downloaded() {
        let dir = tempfile::tempdir().unwrap();
        let mut game = job("game data", DownloadProtocol::Ldiff);
        assert!(!ldiff_job_is_cached(dir.path(), &game));
        game.from_version = None;
        assert!(!ldiff_job_is_cached(dir.path(), &game));
    }
}
