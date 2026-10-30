use anyhow::{bail, Context, Result};
use flate2::read::GzDecoder;
use liblzma::read::XzDecoder;
use reqwest::blocking::Client;
use serde::Deserialize;
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::Duration;
use tar::Archive;

use crate::model::{Runner, RunnerProvider};
use berry_core::settings;

#[derive(Debug, Deserialize)]
struct Release {
    assets: Vec<Asset>,
}

#[derive(Debug, Deserialize)]
struct Asset {
    name: String,
    browser_download_url: Option<String>,
    download_url: Option<String>,
}

/// Error returned by [`RunnerService::install_with_progress`] when the installation was stopped
/// through its cancel flag. Everything the attempt created has been deleted by then.
#[derive(Debug)]
pub struct InstallCancelled;

impl std::fmt::Display for InstallCancelled {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("runner installation cancelled")
    }
}

impl std::error::Error for InstallCancelled {}

/// Whether `error` is the result of a cancelled installation rather than a failure.
pub fn is_cancelled(error: &anyhow::Error) -> bool {
    error.downcast_ref::<InstallCancelled>().is_some()
}

/// Upper bound for the release-list requests.
const API_TIMEOUT: Duration = Duration::from_secs(30);
/// Upper bound for a whole runner download: the blocking client's default of 30 s would cut off
/// any archive that takes longer than that to arrive.
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(3 * 60 * 60);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
/// Largest amount of data a runner archive may unpack to; protects the disk from archive bombs.
const MAX_UNPACKED_BYTES: u64 = 32 * 1024 * 1024 * 1024;

fn check_cancelled(cancel: &AtomicBool) -> Result<()> {
    if cancel.load(Ordering::Relaxed) {
        return Err(InstallCancelled.into());
    }
    Ok(())
}

pub struct RunnerService {
    /// Short timeouts, for the release lists. Built on first use: every blocking client owns a
    /// background thread, and most services only ever list the installed runners.
    client: OnceLock<Client>,
    /// Long total timeout, for the archives. Built on first use.
    download_client: OnceLock<Client>,
    runners_dir: PathBuf,
}

impl RunnerService {
    pub fn new() -> Result<Arc<Self>> {
        let runners_dir = settings::runners_dir();
        for provider in RunnerProvider::ALL {
            fs::create_dir_all(runners_dir.join(provider.directory_name()))
                .with_context(|| format!("creating runner directory for {provider}"))?;
        }
        Ok(Arc::new(Self {
            client: OnceLock::new(),
            download_client: OnceLock::new(),
            runners_dir,
        }))
    }

    /// Client for the release lists.
    fn api_client(&self) -> Result<&Client> {
        if let Some(client) = self.client.get() {
            return Ok(client);
        }
        let built = Client::builder()
            .user_agent(berry_core::http::user_agent())
            .connect_timeout(CONNECT_TIMEOUT)
            .timeout(API_TIMEOUT)
            .build()
            .context("creating HTTP client")?;
        Ok(self.client.get_or_init(|| built))
    }

    /// Client for the runner archives.
    fn transfer_client(&self) -> Result<&Client> {
        if let Some(client) = self.download_client.get() {
            return Ok(client);
        }
        let built = Client::builder()
            .user_agent(berry_core::http::user_agent())
            .connect_timeout(CONNECT_TIMEOUT)
            .timeout(DOWNLOAD_TIMEOUT)
            .tcp_keepalive(Duration::from_secs(30))
            .build()
            .context("creating HTTP download client")?;
        Ok(self.download_client.get_or_init(|| built))
    }

    pub fn provider_dir(&self, provider: RunnerProvider) -> PathBuf {
        self.runners_dir.join(provider.directory_name())
    }

    pub fn is_installed(&self, provider: RunnerProvider, name: &str) -> bool {
        self.provider_dir(provider).join(name).is_dir()
    }

    pub fn proton_path(&self, name: &str) -> Option<PathBuf> {
        for provider in RunnerProvider::ALL {
            let path = self.provider_dir(provider).join(name).join("proton");
            if path.is_file() {
                return Some(path);
            }
        }
        None
    }

    pub fn installed_count(&self) -> usize {
        RunnerProvider::ALL
            .iter()
            .map(|p| self.installed_for(*p).len())
            .sum()
    }

    pub fn default_runner(&self) -> Option<String> {
        for provider in RunnerProvider::ALL {
            let mut installed = self.installed_for(provider);
            installed.sort_by(|a, b| runner_name_cmp(b, a));
            if let Some(name) = installed.into_iter().next() {
                return Some(name);
            }
        }
        None
    }

    pub fn installed_for(&self, provider: RunnerProvider) -> Vec<String> {
        fs::read_dir(self.provider_dir(provider))
            .ok()
            .into_iter()
            .flatten()
            .filter_map(|e| {
                let e = e.ok()?;
                e.file_type()
                    .ok()?
                    .is_dir()
                    .then(|| e.file_name().to_string_lossy().into_owned())
            })
            .collect()
    }

    pub fn fetch(&self, provider: RunnerProvider) -> Result<Vec<Runner>> {
        let releases: Vec<Release> = self
            .api_client()?
            .get(provider.source_url())
            .header("Accept", "application/json")
            .send()
            .with_context(|| format!("fetching {provider} releases"))?
            .error_for_status()
            .with_context(|| format!("{provider} returned an HTTP error"))?
            .json()
            .with_context(|| format!("parsing {provider} release list"))?;

        let mut runners = Vec::new();
        for release in releases {
            let Some(asset) = release
                .assets
                .into_iter()
                .find(|a| a.name.ends_with("-x86_64.tar.xz") || a.name.ends_with("-x86_64.tar.gz"))
            else {
                continue;
            };
            let Some(url) = asset.browser_download_url.or(asset.download_url) else {
                continue;
            };
            if !is_https(&url) {
                continue;
            }
            let Some(name) = normalize_runner_name(&asset.name) else {
                continue;
            };
            let mut runner = Runner::new(provider, name.clone(), asset.name, url);
            runner.installed = self.is_installed(provider, &name);
            runners.push(runner);
        }
        Ok(runners)
    }

    /// Downloads and installs a runner, reporting the download as a fraction between 0 and 1. `None` means the
    /// size is not known (or the archive is being unpacked) and the progress is indeterminate.
    /// The callback runs on the installing thread and is called only when the percentage changes.
    ///
    /// Setting `cancel` stops the download or the unpacking, whichever is running, and the call
    /// returns an [`InstallCancelled`] error (see [`is_cancelled`]). The staging directory with the
    /// partial archive and the partially unpacked files is deleted, so the runner leaves no trace.
    pub fn install_with_progress(
        &self,
        runner: &Runner,
        cancel: &AtomicBool,
        mut on_progress: impl FnMut(Option<f64>),
    ) -> Result<()> {
        if !is_safe_name(&runner.name) {
            bail!("invalid runner name");
        }
        if !is_https(&runner.download_url) {
            bail!("runner has no HTTPS download URL");
        }
        if !is_safe_name(&runner.filename) {
            bail!("invalid runner archive name");
        }
        if self.is_installed(runner.provider, &runner.name) {
            bail!("runner {} is already installed", runner.name);
        }

        let provider_dir = self.provider_dir(runner.provider);
        let staging = tempfile::Builder::new()
            .prefix(".install-")
            .tempdir_in(&self.runners_dir)
            .context("creating runner staging directory")?;
        let archive_path = staging.path().join(&runner.filename);

        check_cancelled(cancel)?;
        let mut response = self
            .transfer_client()?
            .get(&runner.download_url)
            .send()
            .with_context(|| format!("downloading {}", runner.name))?
            .error_for_status()
            .with_context(|| format!("download failed for {}", runner.name))?;
        let total = response.content_length().filter(|total| *total > 0);
        let mut file = File::create(&archive_path)?;
        let mut buffer = vec![0u8; 128 * 1024];
        let mut received = 0u64;
        let mut last_percent = None;
        on_progress(total.map(|_| 0.0));
        loop {
            check_cancelled(cancel)?;
            let read = response
                .read(&mut buffer)
                .with_context(|| format!("downloading {}", runner.name))?;
            if read == 0 {
                break;
            }
            file.write_all(&buffer[..read])?;
            received += read as u64;
            if let Some(total) = total {
                let percent = (received.min(total) * 100 / total) as u32;
                if last_percent != Some(percent) {
                    last_percent = Some(percent);
                    on_progress(Some(f64::from(percent) / 100.0));
                }
            }
        }
        file.flush()?;
        drop(file);
        check_cancelled(cancel)?;
        // The download is over; unpacking has no size to report.
        on_progress(None);

        let extract_dir = staging.path().join("extracted");
        fs::create_dir(&extract_dir)?;
        extract_archive(&archive_path, &extract_dir, cancel).map_err(|error| {
            if is_cancelled(&error) {
                error
            } else {
                error.context(format!("extracting {}", runner.filename))
            }
        })?;

        // Last chance to stop: after the rename the runner is installed.
        check_cancelled(cancel)?;
        let root = find_extracted_root(&extract_dir)?;
        let final_path = provider_dir.join(&runner.name);
        if final_path.exists() {
            bail!("runner {} is already installed", runner.name);
        }
        fs::rename(&root, &final_path)
            .with_context(|| format!("installing runner {}", runner.name))?;
        crate::steamrt::prefetch_for_runner(final_path);
        Ok(())
    }

    pub fn remove(&self, runner: &Runner) -> Result<()> {
        if !is_safe_name(&runner.name) {
            bail!("invalid runner name");
        }
        if self.installed_count() <= 1 {
            bail!("cannot remove the last installed runner");
        }
        let path = self.provider_dir(runner.provider).join(&runner.name);
        if !path.is_dir() {
            bail!("runner {} is not installed", runner.name);
        }
        fs::remove_dir_all(&path).with_context(|| format!("removing runner {}", runner.name))?;
        Ok(())
    }
}

fn normalize_runner_name(filename: &str) -> Option<String> {
    ["-x86_64.tar.xz", "-x86_64.tar.gz"]
        .iter()
        .find_map(|suffix| filename.strip_suffix(suffix).map(str::to_owned))
}

fn is_https(url: &str) -> bool {
    url.get(..8)
        .is_some_and(|scheme| scheme.eq_ignore_ascii_case("https://"))
}

fn is_safe_name(name: &str) -> bool {
    !name.is_empty()
        && name != "."
        && name != ".."
        && !Path::new(name).is_absolute()
        && !name.contains('/')
        && !name.contains('\\')
}
fn extract_archive(archive_path: &Path, destination: &Path, cancel: &AtomicBool) -> Result<()> {
    let file = File::open(archive_path)?;
    let reader: Box<dyn Read> = if archive_path.to_string_lossy().ends_with(".tar.xz") {
        Box::new(XzDecoder::new(file))
    } else {
        Box::new(GzDecoder::new(file))
    };
    let mut archive = Archive::new(reader);
    let mut unpacked = 0u64;
    for entry in archive.entries()? {
        check_cancelled(cancel)?;
        let mut entry = entry?;
        unpacked = unpacked.saturating_add(entry.size());
        if unpacked > MAX_UNPACKED_BYTES {
            bail!("archive unpacks to more than the allowed size");
        }
        let path = entry.path()?.into_owned();
        if path.components().any(|c| {
            matches!(
                c,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        }) {
            bail!("archive contains an unsafe path: {}", path.display());
        }
        // `unpack_in` also refuses to write through a symlink that leaves `destination`.
        if !entry.unpack_in(destination)? {
            bail!("archive path escapes staging directory: {}", path.display());
        }
    }
    Ok(())
}
fn find_extracted_root(dir: &Path) -> Result<PathBuf> {
    let mut entries = fs::read_dir(dir)?.collect::<io::Result<Vec<_>>>()?;
    if entries.len() == 1 && entries[0].file_type()?.is_dir() {
        return Ok(entries.remove(0).path());
    }
    Ok(dir.to_path_buf())
}

fn runner_name_cmp(left: &str, right: &str) -> std::cmp::Ordering {
    use std::cmp::Ordering;

    let mut left_parts = left
        .split(|c: char| !c.is_ascii_digit())
        .filter(|p| !p.is_empty());
    let mut right_parts = right
        .split(|c: char| !c.is_ascii_digit())
        .filter(|p| !p.is_empty());

    loop {
        match (left_parts.next(), right_parts.next()) {
            (Some(l), Some(r)) => {
                let lv = l.parse::<u64>().unwrap_or(0);
                let rv = r.parse::<u64>().unwrap_or(0);
                match lv.cmp(&rv) {
                    Ordering::Equal => continue,
                    other => return other,
                }
            }
            (Some(_), None) => return Ordering::Greater,
            (None, Some(_)) => return Ordering::Less,
            (None, None) => return left.cmp(right),
        }
    }
}
