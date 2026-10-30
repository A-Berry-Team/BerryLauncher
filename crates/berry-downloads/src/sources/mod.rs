use anyhow::{Context, Result};
use reqwest::blocking::Client;
use serde_json::Value;
use std::sync::OnceLock;
use std::time::Duration;

use berry_games::catalog::{GameCatalogEntry, GameSource};

mod hoyo;
mod kuro;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Backend {
    Sophon,
    Kuro,
}

#[derive(Debug, Clone)]
pub struct DownloadJob {
    pub label: String,
    pub manifest: String,
    pub base_url: String,
}

#[derive(Debug, Clone)]
pub struct InstallPlan {
    pub backend: Backend,
    pub version: String,
    pub jobs: Vec<DownloadJob>,
}

const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
const MAX_ATTEMPTS: u32 = 3;

static CLIENT: OnceLock<Client> = OnceLock::new();

/// One blocking client for all manifest lookups. Every blocking `Client` owns a hidden runtime
/// thread and connection pool, so building one per call (the library checks several games at
/// once) wasted threads and threw away keep-alive connections.
fn client() -> Result<&'static Client> {
    if let Some(client) = CLIENT.get() {
        return Ok(client);
    }
    let built = Client::builder()
        .user_agent(berry_core::http::user_agent())
        .connect_timeout(CONNECT_TIMEOUT)
        .timeout(REQUEST_TIMEOUT)
        .build()
        .context("creating HTTP client")?;
    Ok(CLIENT.get_or_init(|| built))
}

fn is_transient(error: &reqwest::Error) -> bool {
    error.is_timeout()
        || error.is_connect()
        || error
            .status()
            .is_some_and(|s| s.is_server_error() || s.as_u16() == 429)
}

/// GET a JSON document, retrying transient network failures (timeouts, resets, 5xx, 429).
/// Non-transient errors (4xx, malformed JSON) fail immediately.
pub(super) fn get_json(client: &Client, url: &str) -> Result<Value> {
    let mut attempt = 1;
    loop {
        let result = client.get(url).send().and_then(|r| r.error_for_status());
        match result {
            Ok(response) => {
                return response
                    .json()
                    .with_context(|| format!("parsing JSON from {url}"));
            }
            Err(error) if attempt < MAX_ATTEMPTS && is_transient(&error) => {
                tracing::warn!("[sources] {url}: attempt {attempt} failed ({error}), retrying");
                std::thread::sleep(Duration::from_millis(500 * u64::from(attempt)));
                attempt += 1;
            }
            Err(error) => {
                return Err(error).with_context(|| format!("requesting {url}"));
            }
        }
    }
}

/// Percent-encode a value for use inside a URL query string (RFC 3986 unreserved characters
/// are kept), so values taken from remote JSON can never add or alter query parameters.
pub(super) fn encode_component(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            out.push(char::from(byte));
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

/// Download locations announced by remote manifests must use TLS.
pub(super) fn is_https(url: &str) -> bool {
    url.get(..8)
        .is_some_and(|scheme| scheme.eq_ignore_ascii_case("https://"))
}

pub(super) fn str_at<'a>(value: &'a Value, path: &[&str]) -> Option<&'a str> {
    let mut current = value;
    for key in path {
        current = current.get(*key)?;
    }
    current.as_str()
}

pub fn resolve(game: &GameCatalogEntry) -> Result<InstallPlan> {
    resolve_with_client(client()?, game)
}

pub fn resolve_update(game: &GameCatalogEntry, installed_version: &str) -> Result<InstallPlan> {
    let client = client()?;
    let main = resolve_with_client(&client, game)?;
    if let GameSource::Hoyo { game_id, biz } = game.source {
        // A missing pre-download is normal; a failing lookup must not block the main update.
        let pre = hoyo::resolve_predownload(&client, game_id, biz).unwrap_or_else(|error| {
            tracing::warn!("[sources] pre-download lookup failed: {error:#}");
            None
        });
        if let Some(pre) = pre {
            if prefer_predownload(&pre.version, installed_version, &main.version) {
                return Ok(pre);
            }
        }
    }
    Ok(main)
}

/// A pre-download is only chosen when it is a real, newer version than both the installed
/// and the currently published one.
fn prefer_predownload(pre: &str, installed: &str, main: &str) -> bool {
    pre != "unknown" && pre != installed && pre != main
}

/// Is a newer version published? Answers from the branch listing alone (one request) when that
/// is conclusive, and only builds the full plan when a pre-download might apply.
pub fn update_available(game: &GameCatalogEntry, installed_version: &str) -> Result<bool> {
    if let GameSource::Hoyo { game_id, biz } = game.source {
        let hint = hoyo::branch_hint(client()?, game_id, biz)?;
        if let Some(answer) = decide_from_hint(&hint, installed_version) {
            return Ok(answer);
        }
    }
    let plan = resolve_update(game, installed_version)?;
    Ok(plan.version != "unknown" && plan.version != installed_version)
}

/// `None` means the hint is not enough and the full plan must be resolved.
fn decide_from_hint(hint: &hoyo::BranchHint, installed: &str) -> Option<bool> {
    if hint.main_version == "unknown" {
        return None;
    }
    if hint.main_version != installed {
        return Some(true);
    }
    // Same as installed: only a pre-download could still offer something newer.
    (!hint.has_predownload).then_some(false)
}

fn resolve_with_client(client: &Client, game: &GameCatalogEntry) -> Result<InstallPlan> {
    match game.source {
        GameSource::Hoyo { game_id, biz } => hoyo::resolve(client, game_id, biz),
        GameSource::Kuro { index_url } => kuro::resolve(client, index_url),
    }
}
