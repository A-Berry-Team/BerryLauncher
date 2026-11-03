use anyhow::{anyhow, bail, Result};
use reqwest::blocking::Client;
use serde_json::Value;

use super::{encode_component, get_json, is_https, str_at};
use super::{Backend, DownloadJob, InstallPlan};

const HYP_LAUNCHER_ID: &str = "VYTpXlbWo8";
const HYP_API: &str = "https://sg-hyp-api.hoyoverse.com/hyp/hyp-connect/api";
const SOPHON_BUILD_API: &str =
    "https://sg-public-api.hoyoverse.com/downloader/sophon_chunk/api/getBuild";

pub(super) fn resolve(
    client: &Client,
    game_id: &str,
    biz: &str,
    voices: &[String],
) -> Result<InstallPlan> {
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
    let jobs = jobs_from_manifests(manifests, true, voices)?;
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
    installed_version: Option<&str>,
    voices: &[String],
) -> Result<Option<InstallPlan>> {
    // Berry's game-manifests repository (A-Berry-Team/game-manifests) can publish the actual
    // preload diff manifest under `extra.preload`. HoYoPlay's getBuild endpoint may otherwise
    // return the live/full manifest even while a preload branch exists, which turns a ~8 GiB
    // preload into a 120+ GiB scan. A game without a published preload (absent, `null` or `{}`)
    // falls through to HoYoPlay's explicit `pre_download` branch below.
    match resolve_repository_preload(client, biz, installed_version, voices) {
        Ok(Some(plan)) => return Ok(Some(plan)),
        Ok(None) => {}
        // Never fall back to getBuild after finding a malformed or incompatible repository
        // preload: that endpoint can silently hand us the full/live manifest again.
        Err(error) => return Err(error),
    }

    let entry = find_branch(client, game_id, biz)?;
    let main = &entry["main"];
    // Falling back to `main` here silently turned an ordinary live build into a
    // "pre-download". A preload exists only when HoYoverse explicitly publishes its branch.
    let Some(target) = predownload_block(&entry) else {
        return Ok(None);
    };

    let package_id = str_at(target, &["package_id"])
        .or_else(|| str_at(main, &["package_id"]))
        .ok_or_else(|| anyhow!("branch has no package_id"))?;
    let branch = str_at(target, &["branch"]).unwrap_or("predownload");
    let version = str_at(target, &["tag"]).unwrap_or("unknown").to_owned();
    // A "pre-download" that is the installed version, or the live version itself, is not one.
    if installed_version == Some(version.as_str())
        || str_at(main, &["tag"]) == Some(version.as_str())
    {
        return Ok(None);
    }

    let Ok(build) = fetch_build(client, target, Some(main), package_id, branch) else {
        return Ok(None);
    };
    let Some(manifests) = build["data"]["manifests"].as_array() else {
        return Ok(None);
    };
    let jobs = jobs_from_manifests(manifests, true, voices)?;
    if jobs.is_empty() {
        return Ok(None);
    }
    Ok(Some(InstallPlan {
        backend: Backend::Sophon,
        version,
        jobs,
    }))
}

/// Resolve a live update from the published version's LDiff manifest. If no matching diff is
/// published, the caller may fall back to the regular Sophon update path.
pub(super) fn resolve_repository_update(
    client: &Client,
    biz: &str,
    installed_version: &str,
    voices: &[String],
) -> Result<Option<InstallPlan>> {
    let root = get_json(client, &super::game_manifest_url(biz))?;
    update_plan_from_manifest(&root, biz, installed_version, voices)
}

/// Pure part of [`resolve_repository_update`] (no network).
fn update_plan_from_manifest(
    root: &Value,
    biz: &str,
    installed_version: &str,
    voices: &[String],
) -> Result<Option<InstallPlan>> {
    let Some(latest) = str_at(root, &["latest_version"]) else {
        return Ok(None);
    };
    let Some(version_entry) = root
        .get("game_versions")
        .and_then(Value::as_array)
        .and_then(|versions| {
            versions
                .iter()
                .find(|entry| str_at(entry, &["metadata", "version"]) == Some(latest))
        })
    else {
        return Ok(None);
    };
    let Some(game_diff) = version_entry
        .get("game")
        .and_then(|game| game.get("diff"))
        .and_then(Value::as_array)
        .and_then(|diffs| {
            diffs
                .iter()
                .find(|item| str_at(item, &["original_version"]) == Some(installed_version))
        })
    else {
        return Ok(None);
    };

    tracing::info!("[hoyo] selected live {biz} LDiff update {latest} from {installed_version}");
    let mut jobs = vec![repository_job(
        game_diff,
        "game data",
        super::DownloadProtocol::Ldiff,
        Some(installed_version),
    )?];

    // The voice packs the user chose (kept as installed, never switched during an update). A
    // full manifest is cheap to verify when already installed.
    jobs.extend(voice_jobs(
        version_entry
            .get("audio")
            .and_then(|audio| audio.get("full")),
        voices,
    )?);

    Ok(Some(InstallPlan {
        backend: Backend::Sophon,
        version: latest.to_owned(),
        jobs,
    }))
}

/// Resolve the explicit preload metadata published in the game's manifest. The game package must
/// use the `diff` entry matching the installed version; using `full` here defeats preloading.
fn resolve_repository_preload(
    client: &Client,
    biz: &str,
    installed_version: Option<&str>,
    voices: &[String],
) -> Result<Option<InstallPlan>> {
    let root = get_json(client, &super::game_manifest_url(biz))?;
    preload_plan_from_manifest(&root, biz, installed_version, voices)
}

/// `extra.preload` is optional: a manifest with no preload carries no `extra`, `"preload": null`
/// or `"preload": {}` (HSR, ZZZ and HI3 publish `{}` while nothing is pre-downloadable).
fn is_blank(value: &Value) -> bool {
    value.is_null() || value.as_object().is_some_and(|object| object.is_empty())
}

/// Pure part of [`resolve_repository_preload`] (no network).
///
/// `Ok(None)` means "this manifest publishes no preload"; a preload that is present but
/// incomplete or without a diff for the installed version is an error, so the caller never falls
/// back to a getBuild response that may be the full live manifest.
fn preload_plan_from_manifest(
    root: &Value,
    biz: &str,
    installed_version: Option<&str>,
    voices: &[String],
) -> Result<Option<InstallPlan>> {
    let Some(preload) = root
        .get("extra")
        .and_then(|extra| extra.get("preload"))
        .filter(|preload| !is_blank(preload))
    else {
        return Ok(None);
    };
    let version = str_at(preload, &["metadata", "version"])
        .unwrap_or("unknown")
        .to_owned();
    // The manifest keeps listing a preload after it has been applied (or released). A game that
    // already runs that version has nothing left to pre-download.
    if installed_version == Some(version.as_str()) {
        return Ok(None);
    }
    let game_diffs = preload
        .get("game")
        .and_then(|v| v.get("diff"))
        .and_then(Value::as_array)
        .ok_or_else(|| anyhow!("preload metadata for {biz} has no game diff list"))?;
    let selected = if let Some(installed) = installed_version {
        game_diffs
            .iter()
            .find(|item| str_at(item, &["original_version"]) == Some(installed))
    } else {
        game_diffs.first()
    };
    let Some(game) = selected else {
        bail!(
            "no preload diff for {biz} matches installed version {:?}",
            installed_version
        );
    };
    tracing::info!(
        "[hoyo] selected {biz} preload {version} from installed version {}",
        str_at(game, &["original_version"]).unwrap_or("unknown")
    );

    let from_version = str_at(game, &["original_version"])
        .ok_or_else(|| anyhow!("selected preload diff has no original_version"))?;
    let mut jobs = vec![repository_job(
        game,
        "game data",
        super::DownloadProtocol::Ldiff,
        Some(from_version),
    )?];
    // Prefer the preload's full voice manifests because a missing voice pack needs its complete
    // contents, not merely the delta from a previous audio installation.
    jobs.extend(voice_jobs(
        preload.get("audio").and_then(|v| v.get("full")),
        voices,
    )?);
    Ok(Some(InstallPlan {
        backend: Backend::Sophon,
        version,
        jobs,
    }))
}

/// Job label of a voice pack. `operation::sophon_slot` derives the pack's slot from it.
fn voice_label(code: &str) -> String {
    format!("{code} voice pack")
}

/// One full Sophon job per selected voice language found in a manifest's `audio.full` list, in
/// the order of the selection. A selected language the manifest does not publish is skipped with
/// a warning; an empty selection yields no voice jobs at all (the game then downloads its own
/// default voice-over the first time it starts).
fn voice_jobs(audio_full: Option<&Value>, voices: &[String]) -> Result<Vec<DownloadJob>> {
    let items = audio_full.and_then(Value::as_array);
    let mut jobs = Vec::new();
    for code in voices {
        let item = items.and_then(|items| {
            items
                .iter()
                .find(|item| str_at(item, &["language"]) == Some(code.as_str()))
        });
        match item {
            Some(item) => jobs.push(repository_job(
                item,
                &voice_label(code),
                super::DownloadProtocol::Sophon,
                None,
            )?),
            None => {
                tracing::warn!("[hoyo] the manifest publishes no {code} voice pack; skipping it")
            }
        }
    }
    Ok(jobs)
}

fn repository_job(
    item: &Value,
    label: &str,
    protocol: super::DownloadProtocol,
    from_version: Option<&str>,
) -> Result<DownloadJob> {
    let manifest = str_at(item, &["file_url"])
        .ok_or_else(|| anyhow!("preload manifest entry is missing file_url"))?;
    let base_url = str_at(item, &["file_path"])
        .ok_or_else(|| anyhow!("preload manifest entry is missing file_path"))?;
    if !is_https(manifest) || !is_https(base_url) {
        bail!("preload manifest entry contains a non-HTTPS URL");
    }
    Ok(DownloadJob {
        label: label.to_owned(),
        manifest: manifest.to_owned(),
        base_url: base_url.trim_end_matches('/').to_owned(),
        protocol,
        from_version: from_version.map(str::to_owned),
    })
}

const PREDOWNLOAD_KEYS: [&str; 3] = ["pre_download", "predownload", "preDownload"];

/// The branch's pre-download block. `null`, `{}` and non-objects mean "nothing published".
fn predownload_block(entry: &Value) -> Option<&Value> {
    PREDOWNLOAD_KEYS
        .iter()
        .find_map(|key| entry.get(*key).filter(|v| v.is_object() && !is_blank(v)))
}

/// What the branch listing alone says about a game (a single request).
#[derive(Debug, PartialEq, Eq)]
pub(super) struct BranchHint {
    pub main_version: String,
}

pub(super) fn branch_hint(client: &Client, game_id: &str, biz: &str) -> Result<BranchHint> {
    Ok(hint_from_entry(&find_branch(client, game_id, biz)?))
}

fn hint_from_entry(entry: &Value) -> BranchHint {
    BranchHint {
        main_version: str_at(&entry["main"], &["tag"])
            .unwrap_or("unknown")
            .to_owned(),
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
/// `strict` reports malformed wanted entries as errors; otherwise they are skipped. Besides the game
/// data, only the voice packs listed in `voices` are kept.
fn jobs_from_manifests(
    manifests: &[Value],
    strict: bool,
    voices: &[String],
) -> Result<Vec<DownloadJob>> {
    let mut jobs = Vec::new();
    for item in manifests {
        let field = str_at(item, &["matching_field"])
            .or_else(|| str_at(item, &["category_name"]))
            .unwrap_or_default();
        if field != "game" && !voices.iter().any(|code| code == field) {
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
                voice_label(field)
            },
            manifest: format!("{}/{id}", manifest_prefix.trim_end_matches('/')),
            base_url: chunk_prefix.trim_end_matches('/').to_owned(),
            protocol: super::DownloadProtocol::Sophon,
            from_version: None,
        });
    }
    // Game data first, then the voice packs in the order they were selected.
    let rank = |job: &DownloadJob| {
        voices
            .iter()
            .position(|code| voice_label(code) == job.label)
            .map_or(0, |index| index + 1)
    };
    jobs.sort_by_key(rank);
    Ok(jobs)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn english() -> Vec<String> {
        vec!["en-us".to_owned()]
    }

    fn codes(codes: &[&str]) -> Vec<String> {
        codes.iter().map(|code| (*code).to_owned()).collect()
    }

    #[test]
    fn repository_preload_game_job_uses_ldiff_and_installed_version() {
        let item = serde_json::json!({
            "file_url": "https://cdn.example/manifest",
            "file_path": "https://cdn.example/diffs/10016",
            "original_version": "7.1.0"
        });
        let job = repository_job(
            &item,
            "game data",
            super::super::DownloadProtocol::Ldiff,
            Some("7.1.0"),
        )
        .unwrap();
        assert_eq!(job.protocol, super::super::DownloadProtocol::Ldiff);
        assert_eq!(job.from_version.as_deref(), Some("7.1.0"));
        assert_eq!(job.base_url, "https://cdn.example/diffs/10016");
    }

    #[test]
    fn predownload_requires_an_explicit_branch() {
        let entry = serde_json::json!({
            "main": { "tag": "1.0.0", "package_id": "live" }
        });
        assert!(predownload_block(&entry).is_none());
    }

    #[test]
    fn predownload_recognizes_supported_api_spellings() {
        let entries = [
            serde_json::json!({"main": {}, "pre_download": {"tag": "1.1.0"}}),
            serde_json::json!({"main": {}, "predownload": {"tag": "1.1.0"}}),
            serde_json::json!({"main": {}, "preDownload": {"tag": "1.1.0"}}),
        ];
        for entry in entries {
            assert!(predownload_block(&entry).is_some());
        }
    }

    // Same shape as A-Berry-Team/game-manifests (and TwintailTeam/game-manifests).
    fn diff_entry(original: &str) -> serde_json::Value {
        serde_json::json!({
            "file_url": format!("https://cdn.example/manifests/{original}"),
            "file_path": format!("https://cdn.example/diffs/{original}/"),
            "original_version": original,
        })
    }

    fn manifest(
        latest: &str,
        diffs: &[&str],
        extra: Option<serde_json::Value>,
    ) -> serde_json::Value {
        let mut root = serde_json::json!({
            "biz": "hk4e_global",
            "latest_version": latest,
            "game_versions": [{
                "metadata": { "version": latest },
                "game": {
                    "full": [],
                    "diff": diffs.iter().map(|v| diff_entry(v)).collect::<Vec<_>>(),
                },
                "audio": {
                    "full": [
                        { "language": "zh-cn", "file_url": "https://cdn.example/a/zh", "file_path": "https://cdn.example/a/zh/" },
                        { "language": "en-us", "file_url": "https://cdn.example/a/en", "file_path": "https://cdn.example/a/en/" },
                    ],
                    "diff": [],
                },
            }],
        });
        if let Some(extra) = extra {
            root["extra"] = extra;
        }
        root
    }

    fn preload(version: &str, diffs: &[&str]) -> serde_json::Value {
        serde_json::json!({ "preload": {
            "metadata": { "version": version },
            "game": { "full": [], "diff": diffs.iter().map(|v| diff_entry(v)).collect::<Vec<_>>() },
            "audio": {
                "full": [{ "language": "en-us", "file_url": "https://cdn.example/p/en", "file_path": "https://cdn.example/p/en/" }],
                "diff": [],
            },
        }})
    }

    #[test]
    fn live_update_uses_the_diff_matching_the_installed_version() {
        let root = manifest("7.1.0", &["7.0.0", "6.7.0"], None);
        let plan = update_plan_from_manifest(&root, "hk4e_global", "6.7.0", &english())
            .unwrap()
            .unwrap();
        assert_eq!(plan.version, "7.1.0");
        assert_eq!(plan.jobs[0].protocol, super::super::DownloadProtocol::Ldiff);
        assert_eq!(plan.jobs[0].from_version.as_deref(), Some("6.7.0"));
        // The English voice pack is a full Sophon job, never a diff.
        assert_eq!(plan.jobs[1].label, "en-us voice pack");
        assert_eq!(
            plan.jobs[1].protocol,
            super::super::DownloadProtocol::Sophon
        );
    }

    #[test]
    fn live_update_without_a_matching_diff_falls_back_to_the_caller() {
        // Honkai Impact 3rd publishes no LDiff at all; older installs have no entry either.
        assert!(update_plan_from_manifest(
            &manifest("9.1.0", &[], None),
            "bh3_global",
            "9.0.0",
            &english()
        )
        .unwrap()
        .is_none());
        assert!(update_plan_from_manifest(
            &manifest("7.1.0", &["7.0.0"], None),
            "hk4e_global",
            "5.0.0",
            &english()
        )
        .unwrap()
        .is_none());
    }

    #[test]
    fn manifest_without_preload_means_no_repository_preload() {
        // Manifests of games with nothing to pre-download carry no `extra`, or an empty preload.
        for extra in [
            None,
            Some(serde_json::json!({})),
            Some(serde_json::json!({ "preload": null })),
            // HSR, ZZZ and HI3 publish an empty object while nothing can be pre-downloaded.
            Some(serde_json::json!({ "preload": {} })),
        ] {
            let root = manifest("4.6.0", &["4.5.0"], extra);
            assert!(
                preload_plan_from_manifest(&root, "hkrpg_global", Some("4.6.0"), &english())
                    .unwrap()
                    .is_none()
            );
        }
    }

    #[test]
    fn preload_selects_the_diff_for_the_installed_version() {
        let root = manifest(
            "7.1.0",
            &["7.0.0"],
            Some(preload("7.2.0", &["7.1.0", "7.0.0"])),
        );
        let plan = preload_plan_from_manifest(&root, "hk4e_global", Some("7.0.0"), &english())
            .unwrap()
            .unwrap();
        assert_eq!(plan.version, "7.2.0");
        assert_eq!(plan.jobs[0].protocol, super::super::DownloadProtocol::Ldiff);
        assert_eq!(plan.jobs[0].from_version.as_deref(), Some("7.0.0"));
        assert_eq!(plan.jobs[1].label, "en-us voice pack");
    }

    #[test]
    fn an_already_installed_preload_version_is_no_preload() {
        // After the preload is applied the manifest may keep listing it: nothing is left to do,
        // and that must not be an error either (the UI would keep probing it).
        let root = manifest("7.1.0", &[], Some(preload("7.2.0", &["7.1.0"])));
        assert!(
            preload_plan_from_manifest(&root, "hk4e_global", Some("7.2.0"), &english())
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn empty_predownload_blocks_are_not_a_branch() {
        for entry in [
            serde_json::json!({"main": {}, "pre_download": null}),
            serde_json::json!({"main": {}, "pre_download": {}}),
            serde_json::json!({"main": {}, "predownload": {}}),
        ] {
            assert!(predownload_block(&entry).is_none());
        }
    }

    #[test]
    fn preload_never_degrades_to_a_full_download() {
        let root = manifest("7.1.0", &[], Some(preload("7.2.0", &["7.1.0"])));
        // No diff for 5.0.0: an error, so the caller does not fall back to getBuild.
        assert!(
            preload_plan_from_manifest(&root, "hk4e_global", Some("5.0.0"), &english()).is_err()
        );
        // Present but incomplete preload (no game diff list) is also an error, not "none".
        let broken = manifest(
            "7.1.0",
            &[],
            Some(serde_json::json!({ "preload": { "metadata": { "version": "7.2.0" } } })),
        );
        assert!(
            preload_plan_from_manifest(&broken, "hk4e_global", Some("7.1.0"), &english()).is_err()
        );
    }

    #[test]
    fn live_update_downloads_every_selected_voice_pack_in_selection_order() {
        let root = manifest("7.1.0", &["7.0.0"], None);
        let plan =
            update_plan_from_manifest(&root, "hk4e_global", "7.0.0", &codes(&["zh-cn", "en-us"]))
                .unwrap()
                .unwrap();
        let labels: Vec<_> = plan.jobs.iter().map(|job| job.label.as_str()).collect();
        assert_eq!(
            labels,
            ["game data", "zh-cn voice pack", "en-us voice pack"]
        );
        assert!(plan.jobs[1..]
            .iter()
            .all(|job| job.protocol == super::super::DownloadProtocol::Sophon));
        assert_eq!(plan.jobs[1].manifest, "https://cdn.example/a/zh");
    }

    #[test]
    fn an_empty_selection_adds_no_voice_pack_so_the_game_picks_its_own() {
        let root = manifest("7.1.0", &["7.0.0"], Some(preload("7.2.0", &["7.0.0"])));
        let update = update_plan_from_manifest(&root, "hk4e_global", "7.0.0", &[])
            .unwrap()
            .unwrap();
        assert_eq!(update.jobs.len(), 1);
        assert_eq!(update.jobs[0].label, "game data");
        let preloaded = preload_plan_from_manifest(&root, "hk4e_global", Some("7.0.0"), &[])
            .unwrap()
            .unwrap();
        assert_eq!(preloaded.jobs.len(), 1);
    }

    #[test]
    fn a_selected_voice_pack_the_manifest_does_not_publish_is_skipped() {
        let root = manifest("7.1.0", &["7.0.0"], None);
        let plan =
            update_plan_from_manifest(&root, "hk4e_global", "7.0.0", &codes(&["ko-kr", "en-us"]))
                .unwrap()
                .unwrap();
        let labels: Vec<_> = plan.jobs.iter().map(|job| job.label.as_str()).collect();
        assert_eq!(labels, ["game data", "en-us voice pack"]);
    }

    fn build_entry(field: &str) -> serde_json::Value {
        serde_json::json!({
            "matching_field": field,
            "manifest": { "id": format!("m-{field}") },
            "manifest_download": { "url_prefix": "https://cdn.example/manifest" },
            "chunk_download": { "url_prefix": "https://cdn.example/chunks" },
        })
    }

    #[test]
    fn fresh_install_keeps_the_game_and_only_the_selected_voice_packs() {
        let manifests: Vec<_> = ["en-us", "game", "ja-jp", "ko-kr", "zh-cn"]
            .into_iter()
            .map(build_entry)
            .collect();
        let jobs = jobs_from_manifests(&manifests, true, &codes(&["ko-kr", "en-us"])).unwrap();
        let labels: Vec<_> = jobs.iter().map(|job| job.label.as_str()).collect();
        assert_eq!(
            labels,
            ["game data", "ko-kr voice pack", "en-us voice pack"]
        );

        let only_game = jobs_from_manifests(&manifests, true, &[]).unwrap();
        assert_eq!(only_game.len(), 1);
        assert_eq!(only_game[0].label, "game data");
    }
}
