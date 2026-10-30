use anyhow::{bail, Context, Result};
use reqwest::blocking::Client;
use reqwest::header::{HeaderMap, HeaderValue, CONTENT_TYPE, RANGE, REFERER};
use serde::Deserialize;
use std::collections::HashSet;
use std::fs::{self, File};
use std::io::{BufWriter, Read, Write};
use std::path::PathBuf;
use std::sync::{Arc, Condvar, Mutex, OnceLock};
use std::time::{Duration, Instant};

use berry_games::catalog::GameCatalogEntry;

const HOYOVERSE_LAUNCHER_ID: &str = "VYTpXlbWo8";

const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
/// Whole-transfer limit of the shared client; videos take far longer than a JSON answer.
const TRANSFER_TIMEOUT: Duration = Duration::from_secs(300);
const HOYO_INFO_TIMEOUT: Duration = Duration::from_secs(30);
/// How long the game list of the HoYoverse API is reused instead of being requested again.
const HOYO_INFO_TTL: Duration = Duration::from_secs(10 * 60);
const MAX_IMAGE_BYTES: u64 = 64 * 1024 * 1024;
const MAX_VIDEO_BYTES: u64 = 1024 * 1024 * 1024;

/// One client (and connection pool) for every artwork download: each blocking client owns a
/// hidden runtime thread, so building one per update wasted threads and connections.
fn http_client() -> Result<&'static Client> {
    static CLIENT: OnceLock<Client> = OnceLock::new();
    if let Some(client) = CLIENT.get() {
        return Ok(client);
    }
    let built = Client::builder()
        .user_agent(berry_core::http::user_agent())
        .connect_timeout(CONNECT_TIMEOUT)
        .timeout(TRANSFER_TIMEOUT)
        .build()
        .context("creating HTTP client")?;
    Ok(CLIENT.get_or_init(|| built))
}

/// Last answer of the HoYoverse game list with the language it was requested in. Every HoYoverse
/// game reads the same document, so it is fetched once for all of them.
static HOYO_INFO: Mutex<Option<(Instant, String, Arc<HoyoResponse>)>> = Mutex::new(None);

// The window prefetches backgrounds while a newly opened library page may request the same
// assets. Keep at most one updater per game active: concurrent downloads can otherwise race while
// replacing the same files and needlessly duplicate HTTP requests. This lock is only held while
// checking/updating the set; never hold it during network or filesystem work.
static ACTIVE_UPDATES: OnceLock<(Mutex<HashSet<String>>, Condvar)> = OnceLock::new();

struct UpdateLease(String);

impl UpdateLease {
    /// Returns whether another caller completed the update while this caller was waiting.
    fn acquire(game_id: &str) -> (Option<Self>, bool) {
        let (active, changed) =
            ACTIVE_UPDATES.get_or_init(|| (Mutex::new(HashSet::new()), Condvar::new()));
        let mut active = active
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        if active.contains(game_id) {
            while active.contains(game_id) {
                active = changed
                    .wait(active)
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
            }
            // The previous updater has completed. Reuse its cache instead of issuing a second
            // metadata request or racing another replacement of the same files.
            return (None, true);
        }

        active.insert(game_id.to_owned());
        (Some(Self(game_id.to_owned())), false)
    }
}

impl Drop for UpdateLease {
    fn drop(&mut self) {
        let (active, changed) =
            ACTIVE_UPDATES.get_or_init(|| (Mutex::new(HashSet::new()), Condvar::new()));
        let mut active = active
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        active.remove(&self.0);
        changed.notify_all();
    }
}

#[derive(Debug, Clone, Default)]
pub struct BackgroundAssets {
    pub static_path: Option<PathBuf>,
    pub video_path: Option<PathBuf>,
    pub overlay_path: Option<PathBuf>,
}

#[derive(Debug, Deserialize)]
struct HoyoResponse {
    data: HoyoData,
}

#[derive(Debug, Deserialize)]
struct HoyoData {
    game_info_list: Vec<HoyoGameInfo>,
}

#[derive(Debug, Clone, Deserialize)]
struct HoyoGameInfo {
    game: Option<HoyoGame>,
    backgrounds: Vec<HoyoBackground>,
}

#[derive(Debug, Clone, Deserialize)]
struct HoyoGame {
    biz: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct HoyoBackground {
    #[serde(rename = "type")]
    background_type: Option<String>,
    background: Option<HoyoImage>,
    video: Option<HoyoImage>,
    theme: Option<HoyoImage>,
}

#[derive(Debug, Clone, Deserialize)]
struct HoyoImage {
    url: Option<String>,
}

pub fn background_dir(game_id: &str) -> PathBuf {
    berry_core::settings::data_dir()
        .join("resources")
        .join(game_id)
}

pub fn background_path(game_id: &str) -> PathBuf {
    background_dir(game_id).join("background.webp")
}

pub fn video_path(game_id: &str) -> PathBuf {
    background_dir(game_id).join("background.mp4")
}

pub fn overlay_path(game_id: &str) -> PathBuf {
    background_dir(game_id).join("background-overlay.webp")
}

fn metadata_path(game_id: &str) -> PathBuf {
    background_dir(game_id).join("background.url")
}

fn video_metadata_path(game_id: &str) -> PathBuf {
    background_dir(game_id).join("background.video.url")
}

fn overlay_metadata_path(game_id: &str) -> PathBuf {
    background_dir(game_id).join("background-overlay.url")
}

const WUWA_PAGE_URL: &str = "https://wutheringwaves.kurogames.com/";
const WUWA_VIDEO_FILE: &str = "bg-video.mp4";
// The video URL is signed (auth_key) and expires, so it cannot be cached. The downloaded file is
// identified by what the server reports about it instead, and re-checked at most this often.
const WUWA_VIDEO_RECHECK: Duration = Duration::from_secs(24 * 60 * 60);

fn video_url_override(game_id: &str) -> Option<String> {
    let text = fs::read_to_string(background_dir(game_id).join("video-url.txt")).ok()?;
    let text = text.trim();
    text.starts_with("https://").then(|| text.to_owned())
}

fn external_background_urls(game_id: &str) -> (Option<&'static str>, Option<&'static str>) {
    match game_id {
        "wuthering-waves" => (
            Some(
                "https://hw-pcdownload-qcloud.aki-game.net/launcher/clientUpload/cxol3urv2yjpftavjv.webp",
            ),
            None,
        ),
        _ => (None, None),
    }
}

fn has_signed_video(game_id: &str) -> bool {
    game_id == "wuthering-waves"
}

fn wuwa_client() -> Result<Client> {
    let mut headers = HeaderMap::new();
    headers.insert(REFERER, HeaderValue::from_static(WUWA_PAGE_URL));
    Ok(Client::builder()
        .user_agent(berry_core::http::BROWSER_USER_AGENT)
        .default_headers(headers)
        .connect_timeout(CONNECT_TIMEOUT)
        .timeout(TRANSFER_TIMEOUT)
        .build()?)
}

// The page scripts build the signed URL, so the page has to be rendered to read it.
fn resolve_wuwa_video_url() -> Result<String> {
    super::webview::find_signed_url(WUWA_PAGE_URL, WUWA_VIDEO_FILE)
}

// ETag, Last-Modified and size of the video, read with a one-byte range request.
fn remote_fingerprint(client: &Client, url: &str) -> Result<String> {
    let response = client.get(url).header(RANGE, "bytes=0-0").send()?;
    let status = response.status();
    if !status.is_success() {
        bail!("video request returned HTTP {status}");
    }
    let header = |name: &str| {
        response
            .headers()
            .get(name)
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default()
    };
    let length = if status.as_u16() == 206 {
        header("content-range")
            .rsplit('/')
            .next()
            .unwrap_or_default()
    } else {
        header("content-length")
    };
    let fingerprint = format!("{}|{}|{length}", header("etag"), header("last-modified"));
    Ok(if fingerprint == "||" {
        url.split('?').next().unwrap_or(url).to_owned()
    } else {
        fingerprint
    })
}

fn update_signed_video(game_id: &str) -> Result<()> {
    let path = video_path(game_id);
    let metadata = video_metadata_path(game_id);
    let cached = path.is_file();
    let checked_recently = fs::metadata(&metadata)
        .and_then(|meta| meta.modified())
        .ok()
        .and_then(|time| time.elapsed().ok())
        .is_some_and(|age| age < WUWA_VIDEO_RECHECK);
    if cached && checked_recently {
        return Ok(());
    }

    let client = wuwa_client()?;
    let url = match video_url_override(game_id) {
        Some(url) => url,
        None => resolve_wuwa_video_url()?,
    };
    let fingerprint = remote_fingerprint(&client, &url)?;
    if !(cached && current_metadata(&metadata).as_deref() == Some(fingerprint.as_str())) {
        download_file(&client, &url, &path, MAX_VIDEO_BYTES)?;
    }
    // Also refreshes the modification time used by the recheck interval.
    fs::write(&metadata, &fingerprint)
        .with_context(|| format!("writing {}", metadata.display()))?;
    Ok(())
}

fn hoyoverse_language() -> String {
    berry_locale::background_language()
}

/// The HoYoverse game list for `language`, from the cache when it is recent enough. The lock is
/// held during the request on purpose: games that ask at the same time wait for one answer.
fn hoyoverse_game_info(language: &str) -> Result<Arc<HoyoResponse>> {
    let mut cache = HOYO_INFO
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some((fetched_at, cached_language, response)) = cache.as_ref() {
        if cached_language == language && fetched_at.elapsed() < HOYO_INFO_TTL {
            return Ok(Arc::clone(response));
        }
    }
    let mut url = reqwest::Url::parse(
        "https://sg-hyp-api.hoyoverse.com/hyp/hyp-connect/api/getAllGameBasicInfo",
    )?;
    url.query_pairs_mut()
        .append_pair("launcher_id", HOYOVERSE_LAUNCHER_ID)
        .append_pair("language", language);
    let response: HoyoResponse = http_client()?
        .get(url)
        .timeout(HOYO_INFO_TIMEOUT)
        .send()?
        .error_for_status()?
        .json()?;
    let response = Arc::new(response);
    *cache = Some((Instant::now(), language.to_owned(), Arc::clone(&response)));
    Ok(response)
}

fn hoyoverse_background_urls(
    game: &GameCatalogEntry,
) -> Result<(Option<String>, Option<String>, Option<String>)> {
    let response = hoyoverse_game_info(&hoyoverse_language())?;

    let biz = match game.source {
        berry_games::catalog::GameSource::Hoyo { biz, .. } => biz,
        _ => return Ok((None, None, None)),
    };

    let game_info = response.data.game_info_list.iter().find(|info| {
        info.game
            .as_ref()
            .and_then(|game| game.biz.as_deref())
            .is_some_and(|value| {
                value == biz || value.starts_with(biz.split('_').next().unwrap_or(biz))
            })
    });

    let Some(game_info) = game_info else {
        return Ok((None, None, None));
    };

    let background = game_info
        .backgrounds
        .iter()
        .find(|background| {
            background.background.is_some()
                && (background.background_type.as_deref() == Some("BACKGROUND_TYPE_VIDEO")
                    || background.video.is_some())
        })
        .or_else(|| game_info.backgrounds.first())
        .cloned();

    let Some(background) = background else {
        return Ok((None, None, None));
    };

    let static_url = background
        .background
        .and_then(|image| image.url)
        .filter(|url| !url.is_empty());
    let video_url = background
        .video
        .and_then(|image| image.url)
        .filter(|url| !url.is_empty());
    let overlay_url = background
        .theme
        .and_then(|image| image.url)
        .filter(|url| !url.is_empty());
    Ok((static_url, video_url, overlay_url))
}

fn current_metadata(path: &PathBuf) -> Option<String> {
    fs::read_to_string(path)
        .ok()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

fn metadata_matches(path: &PathBuf, url: &str) -> bool {
    let expected = format!("{}\n{}", hoyoverse_language(), url);
    current_metadata(path).as_deref() == Some(expected.as_str())
}

fn write_metadata(path: &PathBuf, url: &str) -> Result<()> {
    let expected = format!("{}\n{}", hoyoverse_language(), url);
    fs::write(path, expected).with_context(|| format!("writing {}", path.display()))?;
    Ok(())
}

/// Downloads `url` to `path` through a temporary file, refusing anything that is not HTTPS or
/// that is larger than `max_bytes`.
fn download_file(client: &Client, url: &str, path: &PathBuf, max_bytes: u64) -> Result<()> {
    if !url.starts_with("https://") {
        bail!("refusing to download {url}: not an HTTPS address");
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
    }

    let mut response = client.get(url).send()?.error_for_status()?;
    let is_html = response
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.to_ascii_lowercase().contains("text/html"));
    if is_html {
        bail!("{url} returned HTML instead of the expected file");
    }
    if response
        .content_length()
        .is_some_and(|length| length > max_bytes)
    {
        bail!("{url} is larger than the {max_bytes} bytes allowed");
    }
    let temp = path.with_extension("download");
    let file = File::create(&temp).with_context(|| format!("creating {}", temp.display()))?;
    // The buffered writer's capacity doubles as the copy chunk size.
    let mut file = BufWriter::with_capacity(256 * 1024, file);
    // One byte over the limit tells a file that is exactly `max_bytes` from one that exceeds it.
    let copied = std::io::copy(&mut (&mut response).take(max_bytes + 1), &mut file);
    let outcome = match copied {
        Ok(copied) if copied > max_bytes => Err(anyhow::anyhow!("{url} exceeds {max_bytes} bytes")),
        Ok(_) => file
            .flush()
            .with_context(|| format!("writing {}", temp.display())),
        Err(error) => Err(error).with_context(|| format!("downloading {url}")),
    };
    drop(file);
    if let Err(error) = outcome {
        let _ = fs::remove_file(&temp);
        return Err(error);
    }
    fs::rename(&temp, path).with_context(|| format!("replacing {}", path.display()))?;
    Ok(())
}

fn download_if_needed(
    client: &Client,
    url: Option<&str>,
    path: &PathBuf,
    metadata: &PathBuf,
    localized: bool,
    max_bytes: u64,
) -> Result<()> {
    let Some(url) = url else {
        return Ok(());
    };
    let up_to_date = if localized {
        metadata_matches(metadata, url)
    } else {
        current_metadata(metadata).as_deref() == Some(url)
    };
    if path.is_file() && up_to_date {
        return Ok(());
    }

    download_file(client, url, path, max_bytes)?;
    if localized {
        write_metadata(metadata, url)?;
    } else {
        fs::write(metadata, url).with_context(|| format!("writing {}", metadata.display()))?;
    }
    Ok(())
}

pub fn cached_backgrounds(game_id: &str) -> BackgroundAssets {
    BackgroundAssets {
        static_path: background_path(game_id)
            .is_file()
            .then(|| background_path(game_id)),
        video_path: video_path(game_id).is_file().then(|| video_path(game_id)),
        overlay_path: overlay_path(game_id)
            .is_file()
            .then(|| overlay_path(game_id)),
    }
}

pub fn update_background(game: GameCatalogEntry) -> Result<BackgroundAssets> {
    // A startup prefetch and a page-level refresh can target the same game. The second caller
    // should use the current cache rather than race the first caller's writes.
    let (lease, another_update_finished) = UpdateLease::acquire(game.id);
    if another_update_finished {
        return Ok(cached_backgrounds(game.id));
    }
    let _lease = lease.expect("a game without an active update must acquire its lease");

    let localized = matches!(game.source, berry_games::catalog::GameSource::Hoyo { .. });
    let (static_url, video_url, overlay_url) = match game.source {
        berry_games::catalog::GameSource::Hoyo { .. } => hoyoverse_background_urls(&game)?,
        _ => {
            let (static_url, overlay_url) = external_background_urls(game.id);
            (
                static_url.map(str::to_owned),
                video_url_override(game.id),
                overlay_url.map(str::to_owned),
            )
        }
    };

    let client = http_client()?;

    download_if_needed(
        &client,
        static_url.as_deref(),
        &background_path(game.id),
        &metadata_path(game.id),
        localized,
        MAX_IMAGE_BYTES,
    )?;

    let use_video = berry_core::settings::load_preferences().animated_background;
    if use_video {
        let result = if has_signed_video(game.id) {
            update_signed_video(game.id)
        } else {
            download_if_needed(
                &client,
                video_url.as_deref(),
                &video_path(game.id),
                &video_metadata_path(game.id),
                localized,
                MAX_VIDEO_BYTES,
            )
        };
        // A failed update keeps the video that is already cached.
        if let Err(error) = result {
            tracing::warn!(
                "[background] video download failed for {}: {error:#}",
                game.id
            );
        }
    } else {
        let _ = fs::remove_file(video_path(game.id));
        let _ = fs::remove_file(video_metadata_path(game.id));
    }

    download_if_needed(
        &client,
        overlay_url.as_deref(),
        &overlay_path(game.id),
        &overlay_metadata_path(game.id),
        localized,
        MAX_IMAGE_BYTES,
    )?;

    if !use_video || (video_url.is_none() && !has_signed_video(game.id)) {
        let _ = fs::remove_file(video_path(game.id));
        let _ = fs::remove_file(video_metadata_path(game.id));
        let _ = fs::remove_file(background_dir(game.id).join("background.webm"));
    }
    if overlay_url.is_none() {
        let _ = fs::remove_file(overlay_path(game.id));
        let _ = fs::remove_file(overlay_metadata_path(game.id));
    }

    Ok(cached_backgrounds(game.id))
}
