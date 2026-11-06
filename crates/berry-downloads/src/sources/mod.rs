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
    /// LDiff jobs download patch ranges; Sophon jobs download compressed chunks.
    pub protocol: DownloadProtocol,
    /// Installed version key used to select the matching LDiff patch set.
    pub from_version: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DownloadProtocol {
    Sophon,
    Ldiff,
}

#[derive(Debug, Clone)]
pub struct InstallPlan {
    pub backend: Backend,
    pub version: String,
    pub jobs: Vec<DownloadJob>,
}

/// Berry's own game manifests (A-Berry-Team/game-manifests): one `<biz>.json` per game, same
/// structure as TwintailTeam/game-manifests (`latest_version`, `game_versions[]`, optional
/// `extra.preload`).
const GAME_MANIFESTS_RAW: &str =
    "https://raw.githubusercontent.com/A-Berry-Team/game-manifests/main";

/// URL of the manifest of `biz` (`hk4e_global`, `wuwa_global`, ...).
pub(super) fn game_manifest_url(biz: &str) -> String {
    format!("{GAME_MANIFESTS_RAW}/{biz}.json")
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

/// Plan of a full install. `voices` are the voice-over language codes to include (HoYoverse games
/// only; see [`GameCatalogEntry::voice_packs`]).
pub fn resolve(game: &GameCatalogEntry, voices: &[String]) -> Result<InstallPlan> {
    resolve_with_client(client()?, game, voices)
}

pub fn resolve_update(
    game: &GameCatalogEntry,
    installed_version: &str,
    voices: &[String],
) -> Result<InstallPlan> {
    // Prefer the LDiff published in Berry's game manifests for a live version transition. The
    // separate `extra.preload` entry is never used here: only game_versions/latest_version is safe to apply to live files.
    if let GameSource::Hoyo { biz, .. } = game.source {
        if let Some(plan) =
            hoyo::resolve_repository_update(client()?, biz, installed_version, voices)?
        {
            return Ok(plan);
        }
    }
    resolve(game, voices)
}

/// Resolve the HoYoverse pre-download published in Berry's game manifests (never HoYoverse's own
/// branch listing, which has no HDiff to offer). Kuro has no equivalent implementation in Berry.
pub fn resolve_preload(
    game: &GameCatalogEntry,
    installed_version: Option<&str>,
    voices: &[String],
) -> Result<Option<InstallPlan>> {
    match game.source {
        GameSource::Hoyo { biz, .. } => {
            hoyo::resolve_predownload(client()?, biz, installed_version, voices)
        }
        GameSource::Kuro => Ok(None),
    }
}

/// A pre-download is only valid when the game itself is an HDiff for the installed version: there
/// is no full-download fallback for it.
pub(crate) fn is_hdiff_preload(plan: &InstallPlan) -> bool {
    plan.jobs
        .first()
        .is_some_and(|job| job.protocol == DownloadProtocol::Ldiff && job.from_version.is_some())
}

/// Version of the pre-download that can be applied to `installed_version` right now, or `None`
/// when there is nothing to pre-download. Unlike [`resolve_preload`] this never fails for a
/// published-but-unusable preload (no diff for the installed version, malformed metadata): the
/// button must simply stay hidden then.
pub fn preload_version(game: &GameCatalogEntry, installed_version: &str) -> Result<Option<String>> {
    let plan = match resolve_preload(game, Some(installed_version), &[]) {
        Ok(Some(plan)) => plan,
        Ok(None) => return Ok(None),
        Err(error) => {
            tracing::debug!("[sources] {}: preload not usable: {error:#}", game.id);
            return Ok(None);
        }
    };
    if !is_hdiff_preload(&plan) {
        return Ok(None);
    }
    if let GameSource::Hoyo { game_id, biz } = game.source {
        // Once the version is live it is a normal update (the update button's job).
        let hint = hoyo::branch_hint(client()?, game_id, biz)?;
        if !is_pending_preload(&plan.version, installed_version, &hint.main_version) {
            return Ok(None);
        }
    }
    Ok(Some(plan.version))
}

/// A preload is pending only while its version is neither installed nor already the live one.
fn is_pending_preload(preload: &str, installed: &str, live: &str) -> bool {
    preload != installed && preload != live
}

/// Is a newer live version published? Pre-download branches are deliberately excluded.
pub fn update_available(game: &GameCatalogEntry, installed_version: &str) -> Result<bool> {
    if let GameSource::Hoyo { game_id, biz } = game.source {
        let hint = hoyo::branch_hint(client()?, game_id, biz)?;
        if let Some(answer) = decide_from_hint(&hint, installed_version) {
            return Ok(answer);
        }
    }
    // Only the version matters here, so no voice pack is requested.
    let plan = resolve_update(game, installed_version, &[])?;
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
    // A pre-download is a separate operation and must not make the live update button appear.
    Some(false)
}

fn resolve_with_client(
    client: &Client,
    game: &GameCatalogEntry,
    voices: &[String],
) -> Result<InstallPlan> {
    match game.source {
        GameSource::Hoyo { game_id, biz } => hoyo::resolve(client, game_id, biz, voices),
        GameSource::Kuro => kuro::resolve(client),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn job(protocol: DownloadProtocol, from_version: Option<&str>) -> DownloadJob {
        DownloadJob {
            label: "game data".to_owned(),
            manifest: "https://cdn.example/m".to_owned(),
            base_url: "https://cdn.example".to_owned(),
            protocol,
            from_version: from_version.map(str::to_owned),
        }
    }

    #[test]
    fn a_preload_must_be_an_hdiff_of_the_installed_version() {
        let plan = |jobs| InstallPlan {
            backend: Backend::Sophon,
            version: "7.2.0".to_owned(),
            jobs,
        };
        assert!(is_hdiff_preload(&plan(vec![job(
            DownloadProtocol::Ldiff,
            Some("7.1.0")
        )])));
        // A full Sophon download is never a pre-download.
        assert!(!is_hdiff_preload(&plan(vec![job(
            DownloadProtocol::Sophon,
            None
        )])));
        assert!(!is_hdiff_preload(&plan(vec![job(
            DownloadProtocol::Ldiff,
            None
        )])));
        assert!(!is_hdiff_preload(&plan(Vec::new())));
    }

    #[test]
    fn same_live_version_is_not_an_update_even_if_preload_exists() {
        let hint = hoyo::BranchHint {
            main_version: "5.0.0".to_owned(),
        };
        assert_eq!(decide_from_hint(&hint, "5.0.0"), Some(false));
        assert_eq!(decide_from_hint(&hint, "4.9.0"), Some(true));
    }

    #[test]
    fn preload_is_pending_only_before_it_is_installed_or_live() {
        assert!(is_pending_preload("7.2.0", "7.1.0", "7.1.0"));
        // Already applied: the manifest may still list it.
        assert!(!is_pending_preload("7.2.0", "7.2.0", "7.1.0"));
        // Released: now a regular update.
        assert!(!is_pending_preload("7.2.0", "7.1.0", "7.2.0"));
    }
}
