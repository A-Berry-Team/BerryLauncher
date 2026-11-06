use anyhow::{anyhow, Context, Result};
use reqwest::blocking::Client;
use serde_json::Value;

use super::{get_json, is_https, str_at};
use super::{Backend, DownloadJob, InstallPlan};

const WUWA_BIZ: &str = "wuwa_global";

pub(super) fn resolve(client: &Client) -> Result<InstallPlan> {
    let repo_url = super::game_manifest_url(WUWA_BIZ);
    let manifest_repo = get_json(client, &repo_url)
        .with_context(|| format!("fetching Wuthering Waves manifest from {repo_url}"))?;
    plan_from_manifest(&manifest_repo)
}

/// Pure part of the resolution (no network): pick the latest version entry of the manifest.
fn plan_from_manifest(manifest_repo: &Value) -> Result<InstallPlan> {
    let latest = str_at(manifest_repo, &["latest_version"])
        .ok_or_else(|| anyhow!("Wuthering Waves manifest has no latest_version"))?;
    let versions = manifest_repo["game_versions"]
        .as_array()
        .ok_or_else(|| anyhow!("Wuthering Waves manifest has no game_versions"))?;
    let version_entry = versions
        .iter()
        .find(|entry| str_at(entry, &["metadata", "version"]) == Some(latest))
        .or_else(|| versions.last())
        .ok_or_else(|| anyhow!("Wuthering Waves manifest has no versions"))?;

    let metadata = &version_entry["metadata"];
    let version = str_at(metadata, &["version"]).unwrap_or(latest).to_owned();
    let manifest = str_at(metadata, &["index_file"])
        .ok_or_else(|| anyhow!("Wuthering Waves manifest has no index_file"))?
        .to_owned();
    let base_url = str_at(metadata, &["res_list_url"])
        .ok_or_else(|| anyhow!("Wuthering Waves manifest has no res_list_url"))?
        .trim_end_matches('/')
        .to_owned();

    if !is_https(&manifest) || !is_https(&base_url) {
        anyhow::bail!("Wuthering Waves manifest points to a non-HTTPS download location");
    }

    Ok(InstallPlan {
        backend: Backend::Kuro,
        version,
        jobs: vec![DownloadJob {
            label: "game data".into(),
            manifest,
            base_url,
            protocol: super::DownloadProtocol::Sophon,
            from_version: None,
        }],
    })
}
