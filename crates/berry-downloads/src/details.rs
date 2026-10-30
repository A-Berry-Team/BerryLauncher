//! Observable record of the running (or last) operation of each game.
//!
//! [`crate::state`] only keeps the numbers the progress bar needs. The details window shows more:
//! which step the operation is in (fetch info, verify files, download, extract, finalize), how
//! each step went, the speed over time, the job being processed and a short event log. All of it
//! is fed from what berry-installer reports ([`Progress`]) plus the plan the operation resolved,
//! so no backend needs to know about this module.
//!
//! The record of a game is replaced when its next operation starts and is kept after the
//! operation ends, so a paused or failed operation can still be inspected.

use std::collections::{HashMap, VecDeque};
use std::sync::{Mutex, MutexGuard, OnceLock, PoisonError};
use std::time::{Duration, Instant};

use berry_installer::{Phase, Progress};
use berry_locale::fl;

/// Every phase berry-installer can report, in the order they normally happen.
pub const PHASES: [Phase; 5] = [
    Phase::Resolving,
    Phase::Verifying,
    Phase::Downloading,
    Phase::Extracting,
    Phase::Finalizing,
];

/// Interval between two speed samples (one point of the speed graph).
const SAMPLE_INTERVAL: Duration = Duration::from_millis(500);
/// Samples kept: two minutes of history at [`SAMPLE_INTERVAL`].
pub const MAX_SAMPLES: usize = 240;
const MAX_LOG_ENTRIES: usize = 200;

/// Position of a phase in [`PHASES`].
pub fn phase_index(phase: Phase) -> usize {
    match phase {
        Phase::Resolving => 0,
        Phase::Verifying => 1,
        Phase::Downloading => 2,
        Phase::Extracting => 3,
        Phase::Finalizing => 4,
    }
}

/// Name of a step as shown in the details window.
pub fn phase_title(phase: Phase) -> String {
    match phase {
        Phase::Resolving => fl!("details-step-resolving"),
        Phase::Verifying => fl!("details-step-verifying"),
        Phase::Downloading => fl!("details-step-downloading"),
        Phase::Extracting => fl!("details-step-extracting"),
        Phase::Finalizing => fl!("details-step-finalizing"),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DetailKind {
    Install,
    Update,
    Repair,
}

impl DetailKind {
    pub fn label(self) -> String {
        match self {
            Self::Install => fl!("details-kind-install"),
            Self::Update => fl!("details-kind-update"),
            Self::Repair => fl!("details-kind-repair"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    Running,
    Completed,
    /// Stopped by the user; partial files stay for a later resume.
    Paused,
    Failed(String),
}

/// How a step of the operation looks right now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StepState {
    /// Not reached yet.
    Pending,
    /// In progress.
    Active,
    Done,
    /// The operation moved past it without needing it (nothing to extract, ...).
    Skipped,
    /// The operation stopped (paused or failed) while in this step.
    Interrupted,
}

/// What one phase has reported during the current job.
#[derive(Debug, Clone, Copy, Default)]
pub struct PhaseStat {
    pub seen: bool,
    /// Bytes of this phase (see [`Progress`]).
    pub current: u64,
    /// `0` when unknown.
    pub total: u64,
    /// Time spent in the phase, not counting the stretch in progress.
    spent: Duration,
}

#[derive(Debug, Clone)]
pub struct LogEntry {
    /// Time since the operation started.
    pub at: Duration,
    pub text: String,
}

#[derive(Debug, Clone)]
pub struct DownloadDetails {
    pub kind: DetailKind,
    pub outcome: Outcome,
    /// Last status line of the operation ("Installing game (1/2)").
    pub status: String,
    /// Version the operation is moving the game to.
    pub version: Option<String>,
    pub backend: Option<&'static str>,
    /// 1-based; `0` until the first job starts.
    pub job_index: u32,
    pub job_total: u32,
    pub job_label: String,
    /// Phase reported last.
    pub phase: Option<Phase>,
    /// Per phase of [`PHASES`], for the current job.
    pub stats: [PhaseStat; 5],
    /// Current network speed in bytes per second (`0` outside of the download step).
    pub speed: u64,
    pub peak_speed: u64,
    /// Speed history, oldest first, one point per [`SAMPLE_INTERVAL`] spent downloading.
    pub samples: VecDeque<u64>,
    pub log: VecDeque<LogEntry>,
    started: Instant,
    ended: Option<Instant>,
    phase_since: Instant,
    last_sample: Instant,
    speed_sum: u64,
    speed_count: u64,
    /// Bytes downloaded by the jobs that already finished.
    carried_bytes: u64,
}

impl DownloadDetails {
    fn new(kind: DetailKind) -> Self {
        let now = Instant::now();
        let mut details = Self {
            kind,
            outcome: Outcome::Running,
            status: String::new(),
            version: None,
            backend: None,
            job_index: 0,
            job_total: 0,
            job_label: String::new(),
            phase: None,
            stats: Default::default(),
            speed: 0,
            peak_speed: 0,
            samples: VecDeque::new(),
            log: VecDeque::new(),
            started: now,
            ended: None,
            phase_since: now,
            last_sample: now,
            speed_sum: 0,
            speed_count: 0,
            carried_bytes: 0,
        };
        let text = match kind {
            DetailKind::Install => fl!("details-log-started-install"),
            DetailKind::Update => fl!("details-log-started-update"),
            DetailKind::Repair => fl!("details-log-started-repair"),
        };
        details.push_log(text);
        details
    }

    /// Time since the operation started (frozen once it ended).
    pub fn elapsed(&self) -> Duration {
        self.ended
            .unwrap_or_else(Instant::now)
            .duration_since(self.started)
    }

    pub fn is_running(&self) -> bool {
        self.outcome == Outcome::Running
    }

    /// Time spent in `phase` during the current job, including the stretch in progress.
    pub fn phase_elapsed(&self, phase: Phase) -> Duration {
        let stat = &self.stats[phase_index(phase)];
        let running = if self.ended.is_none() && self.phase == Some(phase) {
            self.phase_since.elapsed()
        } else {
            Duration::ZERO
        };
        stat.spent + running
    }

    pub fn stat(&self, phase: Phase) -> &PhaseStat {
        &self.stats[phase_index(phase)]
    }

    pub fn step_state(&self, phase: Phase) -> StepState {
        let index = phase_index(phase);
        if self.phase == Some(phase) {
            return match self.outcome {
                Outcome::Running => StepState::Active,
                Outcome::Completed => StepState::Done,
                Outcome::Paused | Outcome::Failed(_) => StepState::Interrupted,
            };
        }
        if self.stats[index].seen {
            return StepState::Done;
        }
        let later_seen = self.stats[index + 1..].iter().any(|stat| stat.seen);
        if later_seen || self.outcome == Outcome::Completed {
            StepState::Skipped
        } else {
            StepState::Pending
        }
    }

    /// Bytes fetched from the network so far, over every job of the operation.
    pub fn downloaded_bytes(&self) -> u64 {
        self.carried_bytes + self.stats[phase_index(Phase::Downloading)].current
    }

    /// Mean of the speed samples, in bytes per second.
    pub fn average_speed(&self) -> u64 {
        if self.speed_count == 0 {
            0
        } else {
            self.speed_sum / self.speed_count
        }
    }

    /// Time left for the download step at the current speed.
    pub fn eta(&self) -> Option<Duration> {
        if !self.is_running() || self.phase != Some(Phase::Downloading) || self.speed == 0 {
            return None;
        }
        let stat = self.stat(Phase::Downloading);
        if stat.total <= stat.current {
            return None;
        }
        Some(Duration::from_secs(
            (stat.total - stat.current) / self.speed,
        ))
    }

    fn push_log(&mut self, text: String) {
        let at = self.elapsed();
        self.log.push_back(LogEntry { at, text });
        while self.log.len() > MAX_LOG_ENTRIES {
            self.log.pop_front();
        }
    }

    fn on_plan(&mut self, version: &str, backend: &'static str, jobs: usize) {
        self.version = Some(version.to_owned());
        self.backend = Some(backend);
        self.job_total = u32::try_from(jobs).unwrap_or(u32::MAX);
        let text = fl!(
            "details-log-plan",
            version = version,
            backend = backend,
            jobs = jobs as u64
        );
        self.push_log(text);
    }

    fn on_job(&mut self, index: u32, total: u32, label: &str) {
        // Each job walks through the phases again: keep only the bytes it downloaded.
        self.carried_bytes += self.stats[phase_index(Phase::Downloading)].current;
        self.stats = Default::default();
        self.phase = None;
        self.phase_since = Instant::now();
        self.speed = 0;
        self.job_index = index;
        self.job_total = total;
        self.job_label = label.to_owned();
        let text = fl!(
            "details-log-job",
            name = label,
            current = u64::from(index),
            total = u64::from(total)
        );
        self.push_log(text);
    }

    fn on_progress(&mut self, progress: Progress) {
        let now = Instant::now();
        let index = phase_index(progress.phase);
        if self.phase != Some(progress.phase) {
            if let Some(previous) = self.phase {
                let spent = now.duration_since(self.phase_since);
                self.stats[phase_index(previous)].spent += spent;
            }
            self.phase = Some(progress.phase);
            self.phase_since = now;
            // Backends may bounce between verifying and downloading: log each step once per job.
            if !self.stats[index].seen {
                self.stats[index].seen = true;
                let text = fl!("details-log-step", step = phase_title(progress.phase));
                self.push_log(text);
            }
        }
        self.stats[index].current = progress.current;
        self.stats[index].total = progress.total;

        if progress.phase == Phase::Downloading {
            self.speed = progress.speed;
            if now.duration_since(self.last_sample) >= SAMPLE_INTERVAL {
                self.last_sample = now;
                self.samples.push_back(progress.speed);
                while self.samples.len() > MAX_SAMPLES {
                    self.samples.pop_front();
                }
                self.peak_speed = self.peak_speed.max(progress.speed);
                self.speed_sum += progress.speed;
                self.speed_count += 1;
            }
        } else {
            self.speed = 0;
        }
    }

    fn on_finish(&mut self, outcome: Outcome) {
        let now = Instant::now();
        if let Some(phase) = self.phase {
            let spent = now.duration_since(self.phase_since);
            self.stats[phase_index(phase)].spent += spent;
        }
        self.ended = Some(now);
        self.speed = 0;
        let text = match &outcome {
            Outcome::Running => String::new(),
            Outcome::Completed => fl!("details-log-completed"),
            Outcome::Paused => fl!("details-log-paused"),
            Outcome::Failed(error) => fl!("details-log-failed", error = error.as_str()),
        };
        self.outcome = outcome;
        if !text.is_empty() {
            self.push_log(text);
        }
    }
}

static DETAILS: OnceLock<Mutex<HashMap<String, DownloadDetails>>> = OnceLock::new();

/// Lock the map, recovering the data if a panicking thread poisoned it (see `state::lock`).
fn store() -> MutexGuard<'static, HashMap<String, DownloadDetails>> {
    DETAILS
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
}

fn update(game_id: &str, change: impl FnOnce(&mut DownloadDetails)) {
    if let Some(details) = store().get_mut(game_id) {
        change(details);
    }
}

/// Snapshot of the running (or last) operation of a game.
pub fn download_details(game_id: &str) -> Option<DownloadDetails> {
    store().get(game_id).cloned()
}

pub(crate) fn begin(game_id: &str, kind: DetailKind) {
    store().insert(game_id.to_owned(), DownloadDetails::new(kind));
}

pub(crate) fn record_plan(game_id: &str, version: &str, backend: &'static str, jobs: usize) {
    update(game_id, |details| details.on_plan(version, backend, jobs));
}

pub(crate) fn record_job(game_id: &str, index: u32, total: u32, label: &str) {
    update(game_id, |details| details.on_job(index, total, label));
}

pub(crate) fn record_status(game_id: &str, status: &str) {
    update(game_id, |details| status.clone_into(&mut details.status));
}

pub(crate) fn record_progress(game_id: &str, progress: Progress) {
    update(game_id, |details| details.on_progress(progress));
}

pub(crate) fn finish(game_id: &str, outcome: Outcome) {
    update(game_id, |details| details.on_finish(outcome));
}
