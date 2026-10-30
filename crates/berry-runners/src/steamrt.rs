use anyhow::{Context, Result};
use berry_installer::steamrt::{self, Edition, SteamRuntimeManager};
use berry_installer::JobContext;
use std::path::{Path, PathBuf};
use tracing::{info, warn};

/// The two Steam Linux Runtimes a runner can ask for. A thin wrapper over
/// [`berry_installer::steamrt::Edition`] that knows where Berry keeps them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SteamRuntime {
    Sniper,
    SteamRt4,
}

impl SteamRuntime {
    pub const ALL: [SteamRuntime; 2] = [Self::Sniper, Self::SteamRt4];

    fn installer_edition(self) -> Edition {
        match self {
            Self::Sniper => Edition::Sniper,
            Self::SteamRt4 => Edition::SteamRt4,
        }
    }

    fn from_edition(edition: Edition) -> Self {
        match edition {
            Edition::Sniper => Self::Sniper,
            Edition::SteamRt4 => Self::SteamRt4,
        }
    }

    pub fn edition(self) -> &'static str {
        self.installer_edition().repo_name()
    }

    pub fn dir(self) -> PathBuf {
        root_dir().join(self.edition())
    }

    pub fn entry_point(self) -> PathBuf {
        self.dir().join(steamrt::ENTRY_POINT)
    }

    /// Installed completely (marker + entry point). Never touches the network.
    pub fn is_ready(self) -> bool {
        steamrt::state_at(&self.dir()).is_ready()
    }
}

fn root_dir() -> PathBuf {
    berry_core::settings::extras_dir().join("steamrt")
}

fn manager() -> Result<SteamRuntimeManager> {
    let http = berry_installer::Http::new(berry_core::http::user_agent())
        .context("creating the HTTP client")?;
    Ok(SteamRuntimeManager::new(root_dir(), http))
}

pub fn required_runtime(runner_root: &Path) -> Option<SteamRuntime> {
    let manifest = std::fs::read_to_string(runner_root.join("toolmanifest.vdf")).ok()?;
    manifest.lines().find_map(|line| {
        let mut quoted = line.split('"').skip(1).step_by(2);
        match (quoted.next(), quoted.next()) {
            (Some("require_tool_appid"), Some(appid)) => {
                Edition::from_tool_appid(appid).map(SteamRuntime::from_edition)
            }
            _ => None,
        }
    })
}

pub fn runtime_for_runner(runner_root: &Path) -> SteamRuntime {
    required_runtime(runner_root).unwrap_or_else(|| {
        warn!(
            "[steamrt] {} does not name a downloadable Steam Linux Runtime; using sniper",
            runner_root.display()
        );
        SteamRuntime::Sniper
    })
}

/// Make sure the runtime is installed and return its entry point. Returns immediately (no network)
/// when it already is.
pub fn ensure(runtime: SteamRuntime) -> Result<PathBuf> {
    ensure_with(runtime, &JobContext::new())
}

/// [`ensure`] with progress reporting and cancellation, for callers that show a progress bar.
pub fn ensure_with(runtime: SteamRuntime, ctx: &JobContext) -> Result<PathBuf> {
    if runtime.is_ready() {
        return Ok(runtime.entry_point());
    }
    let edition = runtime.edition();
    info!("[steamrt] downloading Steam Linux Runtime ({edition}); this can take a while");

    let manager = manager()?;
    // The transfer is network bound: a small worker pool is enough and keeps the thread count low.
    let tokio_runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .context("creating the download runtime")?;
    let entry_point = tokio_runtime
        .block_on(manager.ensure(runtime.installer_edition(), ctx))
        .with_context(|| {
            format!("could not install the Steam Linux Runtime ({edition}); check your internet connection")
        })?;
    info!("[steamrt] Steam Linux Runtime ({edition}) ready");
    Ok(entry_point)
}

pub fn prefetch_for_runner(runner_root: PathBuf) {
    std::thread::spawn(move || {
        let runtime = runtime_for_runner(&runner_root);
        if let Err(error) = ensure(runtime) {
            warn!("[steamrt] prefetch failed; Play will retry on demand: {error:#}");
        }
    });
}

pub fn prefetch_for_installed_runners() {
    std::thread::spawn(|| {
        let mut needed: Vec<SteamRuntime> = Vec::new();
        for provider in crate::model::RunnerProvider::ALL {
            let root = berry_core::settings::runners_dir().join(provider.directory_name());
            let Ok(entries) = std::fs::read_dir(root) else {
                continue;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                if !path.join("proton").is_file() {
                    continue;
                }
                let runtime = runtime_for_runner(&path);
                if !needed.contains(&runtime) {
                    needed.push(runtime);
                }
            }
        }
        for runtime in needed {
            if let Err(error) = ensure(runtime) {
                warn!("[steamrt] startup prefetch failed; Play will retry on demand: {error:#}");
            }
        }
    });
}
