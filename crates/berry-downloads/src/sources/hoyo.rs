use anyhow::{anyhow, bail, Result};
use reqwest::blocking::Client;
use serde_json::Value;

use super::{encode_component, get_json, is_https, str_at};
use super::{Backend, DownloadJob, InstallPlan};

const HYP_LAUNCHER_ID: &str = "VYTpXlbWo8";
const HYP_API: &str = "https://sg-hyp-api.hoyoverse.com/hyp/hyp-connect/api";
const SOPHON_BUILD_API: &str =
    "https://sg-public-api.hoyoverse.com/downloader/sophon_chunk/api/getBuild";
const HOYO_VOICE_PACK: &str = "en-us";

pub(super) fn resolve(client: &Client, game_id: &str, biz: &str) -> Result<InstallPlan> {
    let entry = find_branch(client, game_id, biz)?;
    let main = &entry["main"];
    let package_id =
        str_at(main, &["package_id"]).ok_or_else(|| anyhow!("branch has no package_id"))?;
    let branch = str_at(main, &["branch"]).unwrap_or("main");
    let version = str_at(main, &["tag"]).unwrap_or("unknown").to_owned();
    let build = fetch_build(client, main, None, package_id, branch)?;
    let manifests = build["data"]["manifests"]
        .as_array()
        .ok_or_else(|| anyhow!("Sophon build has no manifests"))?;
    let jobs = jobs_from_manifests(manifests, true)?;
    if jobs.is_empty() {
        bail!("Sophon build did not contain a game package");
    }
    Ok(InstallPlan {
        backend: Backend::Sophon,
        version,
        jobs,
    })
}

/// `Ok(None)` means "no pre-download available"; it is not an error.
pub(super) fn resolve_predownload(
    client: &Client,
    game_id: &str,
    biz: &str,
) -> Result<Option<InstallPlan>> {
    let entry = find_branch(client, game_id, biz)?;
    let main = &entry["main"];
    let target = predownload_block(&entry).unwrap_or(main);

    let package_id = str_at(target, &["package_id"])
        .or_else(|| str_at(main, &["package_id"]))
        .ok_or_else(|| anyhow!("branch has no package_id"))?;
    let branch = str_at(target, &["branch"]).unwrap_or("predownload");
    let version = str_at(target, &["tag"]).unwrap_or("unknown").to_owned();

    let Ok(build) = fetch_build(client, target, Some(main), package_id, branch) else {
        return Ok(None);
    };
    let Some(manifests) = build["data"]["manifests"].as_array() else {
        return Ok(None);
    };
    let jobs = jobs_from_manifests(manifests, false)?;
    if jobs.is_empty() {
        return Ok(None);
    }
    Ok(Some(InstallPlan {
        backend: Backend::Sophon,
        version,
        jobs,
    }))
}

const PREDOWNLOAD_KEYS: [&str; 3] = ["pre_download", "predownload", "preDownload"];

fn predownload_block(entry: &Value) -> Option<&Value> {
    PREDOWNLOAD_KEYS
        .iter()
        .find_map(|key| entry.get(*key).filter(|v| v.is_object()))
}

/// What the branch listing alone says about a game (a single request).
#[derive(Debug, PartialEq, Eq)]
pub(super) struct BranchHint {
    pub main_version: String,
    pub has_predownload: bool,
}

pub(super) fn branch_hint(client: &Client, game_id: &str, biz: &str) -> Result<BranchHint> {
    Ok(hint_from_entry(&find_branch(client, game_id, biz)?))
}

fn hint_from_entry(entry: &Value) -> BranchHint {
    BranchHint {
        main_version: str_at(&entry["main"], &["tag"])
            .unwrap_or("unknown")
            .to_owned(),
        has_predownload: predownload_block(entry).is_some(),
    }
}

fn find_branch(client: &Client, game_id: &str, biz: &str) -> Result<Value> {
    let url = format!(
        "{HYP_API}/getGameBranches?launcher_id={HYP_LAUNCHER_ID}&game_ids[]={}",
        encode_component(game_id)
    );
    let branches = get_json(client, &url)?;
    branches["data"]["game_branches"]
        .as_array()
        .and_then(|list| list.iter().find(|b| branch_matches(b, game_id, biz)))
        .cloned()
        .ok_or_else(|| anyhow!("HoYoverse did not return a branch for {biz}"))
}

fn branch_matches(branch: &Value, game_id: &str, biz: &str) -> bool {
    str_at(branch, &["game", "biz"]) == Some(biz)
        || str_at(branch, &["game", "id"]) == Some(game_id)
}

/// Query the Sophon build. `target` supplies the values; `fallback` fills in gaps
/// (the pre-download block often omits the password and plat_app of the main branch).
fn fetch_build(
    client: &Client,
    target: &Value,
    fallback: Option<&Value>,
    package_id: &str,
    branch: &str,
) -> Result<Value> {
    let pick =
        |key: &str| str_at(target, &[key]).or_else(|| fallback.and_then(|f| str_at(f, &[key])));
    let password = pick("password").unwrap_or_default();
    let mut url = format!(
        "{SOPHON_BUILD_API}?branch={}&package_id={}&password={}",
        encode_component(branch),
        encode_component(package_id),
        encode_component(password)
    );
    if let Some(plat_app) = pick("plat_app").filter(|v| !v.is_empty()) {
        url.push_str("&plat_app=");
        url.push_str(&encode_component(plat_app));
    }
    get_json(client, &url)
}

/// Turn the manifest list of a Sophon build into download jobs (game data first).
/// `strict` reports malformed wanted entries as errors; otherwise they are skipped.
fn jobs_from_manifests(manifests: &[Value], strict: bool) -> Result<Vec<DownloadJob>> {
    let mut jobs = Vec::new();
    for item in manifests {
        let field = str_at(item, &["matching_field"])
            .or_else(|| str_at(item, &["category_name"]))
            .unwrap_or_default();
        if field != "game" && field != HOYO_VOICE_PACK {
            continue;
        }
        let parts = (
            str_at(item, &["manifest", "id"]),
            str_at(item, &["manifest_download", "url_prefix"]),
            str_at(item, &["chunk_download", "url_prefix"]),
        );
        let (Some(id), Some(manifest_prefix), Some(chunk_prefix)) = parts else {
            if strict {
                bail!("manifest entry for {field} is missing its id or download prefixes");
            }
            continue;
        };
        if !is_https(manifest_prefix) || !is_https(chunk_prefix) {
            if strict {
                bail!("manifest entry for {field} points to a non-HTTPS download location");
            }
            continue;
        }
        jobs.push(DownloadJob {
            label: if field == "game" {
                "game data".into()
            } else {
                format!("{field} voice pack")
            },
            manifest: format!("{}/{id}", manifest_prefix.trim_end_matches('/')),
            base_url: chunk_prefix.trim_end_matches('/').to_owned(),
        });
    }
    jobs.sort_by_key(|job| job.label != "game data");
    Ok(jobs)
}
