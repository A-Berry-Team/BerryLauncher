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
    let build = fetch_build(client, main, package_id, branch)?;
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

/// Resolve the HoYoverse pre-download of the installed game. `Ok(None)` means "no pre-download
/// available"; it is not an error.
///
/// A pre-download exists for Berry only once its LDiff manifest is published under `extra.preload`
/// in A-Berry-Team/game-manifests. HoYoverse's own `pre_download` branch and the Sophon `getBuild`
/// endpoint are deliberately never consulted: they describe a full build, and a pre-download has no
/// full-download fallback (it must be an HDiff, otherwise a ~8 GiB preload turns into a 120+ GiB
/// scan). Until the repository publishes it the button stays hidden, even if HoYoverse already
/// announced the branch. A preload that is published but unusable (no diff for the installed
/// version, malformed metadata) is an error.
pub(super) fn resolve_predownload(
    client: &Client,
    biz: &str,
    installed_version: Option<&str>,
    voices: &[String],
) -> Result<Option<InstallPlan>> {
    resolve_repository_preload(client, biz, installed_version, voices)
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

    // The voice packs already installed, never switched during an update. Each one is patched with
    // its LDiff when the manifest publishes one for the installed version, and otherwise falls back
    // to its full Sophon manifest, which reuses whatever chunks the files on disk already provide.
    jobs.extend(voice_jobs(
        version_entry.get("audio"),
        voices,
        Some(installed_version),
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
    // Voice packs follow the same rule as the game: LDiff for the installed version when published,
    // the preload's full Sophon manifest otherwise.
    jobs.extend(voice_jobs(preload.get("audio"), voices, installed_version)?);
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

/// Whether a manifest entry is for voice language `code`. Manifests spell languages `en-us` or
/// `en_us` (HoYoverse's own `diff_list_url` keys).
fn is_language(item: &Value, code: &str) -> bool {
    ["language", "matching_field"]
        .iter()
        .filter_map(|key| str_at(item, &[*key]))
        .any(|language| language.replace('_', "-").eq_ignore_ascii_case(code))
}

/// One job per selected voice language, in the order of the selection. `audio` is the manifest's
/// `audio` object. A language with an `audio.diff` entry for `installed_version` is patched with
/// LDiff (HDiffPatch through berry-patcher); otherwise it uses its `audio.full` Sophon manifest, which
/// only fetches the chunks that are missing. A selected language the manifest publishes in neither
/// list is skipped with a warning; an empty selection yields no voice jobs at all (the game then
/// downloads its own default voice-over the first time it starts).
fn voice_jobs(
    audio: Option<&Value>,
    voices: &[String],
    installed_version: Option<&str>,
) -> Result<Vec<DownloadJob>> {
    let list = |name: &str| {
        audio
            .and_then(|audio| audio.get(name))
            .and_then(Value::as_array)
    };
    let mut jobs = Vec::new();
    for code in voices {
        if let Some(installed) = installed_version {
            let diff = list("diff").and_then(|items| {
                items.iter().find(|item| {
                    is_language(item, code)
                        && str_at(item, &["original_version"]) == Some(installed)
                })
            });
            if let Some(item) = diff {
                match repository_job(
                    item,
                    &voice_label(code),
                    super::DownloadProtocol::Ldiff,
                    Some(installed),
                ) {
                    Ok(job) => {
                        tracing::info!("[hoyo] {code} voice pack: LDiff from {installed}");
                        jobs.push(job);
                        continue;
                    }
                    Err(error) => tracing::warn!(
                        "[hoyo] unusable {code} voice LDiff ({error:#}); using the full manifest"
                    ),
                }
            }
        }
        let full = list("full").and_then(|items| items.iter().find(|item| is_language(item, code)));
        match full {
            Some(item) => {
                tracing::info!("[hoyo] {code} voice pack: full Sophon manifest (chunk reuse)");
                jobs.push(repository_job(
                    item,
                    &voice_label(code),
                    super::DownloadProtocol::Sophon,
                    None,
                )?)
            }
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

/// Query the Sophon build of the live (`main`) branch.
fn fetch_build(client: &Client, main: &Value, package_id: &str, branch: &str) -> Result<Value> {
    let password = str_at(main, &["password"]).unwrap_or_default();
    let mut url = format!(
        "{SOPHON_BUILD_API}?branch={}&package_id={}&password={}",
        encode_component(branch),
        encode_component(package_id),
        encode_component(password)
    );
    if let Some(plat_app) = str_at(main, &["plat_app"]).filter(|v| !v.is_empty()) {
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

    fn with_audio_diff(mut root: serde_json::Value, items: serde_json::Value) -> serde_json::Value {
        root["game_versions"][0]["audio"]["diff"] = items;
        root
    }

    fn audio_diff(language: &str, original: &str) -> serde_json::Value {
        serde_json::json!({
            "language": language,
            "original_version": original,
            "file_url": format!("https://cdn.example/ad/{language}/{original}"),
            "file_path": format!("https://cdn.example/ad/{language}/{original}/"),
        })
    }

    #[test]
    fn installed_voice_packs_are_patched_with_ldiff_when_published() {
        let root = with_audio_diff(
            manifest("7.1.0", &["7.0.0"], None),
            serde_json::json!([audio_diff("zh-cn", "7.0.0"), audio_diff("en_us", "7.0.0")]),
        );
        let plan =
            update_plan_from_manifest(&root, "hk4e_global", "7.0.0", &codes(&["zh-cn", "en-us"]))
                .unwrap()
                .unwrap();
        assert_eq!(plan.jobs.len(), 3);
        for job in &plan.jobs {
            assert_eq!(job.protocol, super::super::DownloadProtocol::Ldiff);
            assert_eq!(job.from_version.as_deref(), Some("7.0.0"));
        }
        assert_eq!(plan.jobs[1].label, "zh-cn voice pack");
        assert_eq!(plan.jobs[1].manifest, "https://cdn.example/ad/zh-cn/7.0.0");
        assert_eq!(plan.jobs[2].manifest, "https://cdn.example/ad/en_us/7.0.0");
    }

    #[test]
    fn a_voice_pack_without_a_diff_for_the_installed_version_uses_the_full_manifest() {
        // Only zh-cn has a diff, and only from another version: both end up as full Sophon jobs.
        let root = with_audio_diff(
            manifest("7.1.0", &["7.0.0"], None),
            serde_json::json!([audio_diff("zh-cn", "6.0.0")]),
        );
        let plan =
            update_plan_from_manifest(&root, "hk4e_global", "7.0.0", &codes(&["zh-cn", "en-us"]))
                .unwrap()
                .unwrap();
        for job in &plan.jobs[1..] {
            assert_eq!(job.protocol, super::super::DownloadProtocol::Sophon);
        }
        assert_eq!(plan.jobs[1].manifest, "https://cdn.example/a/zh");
    }

    #[test]
    fn preload_patches_installed_voice_packs_too() {
        let mut preloaded = preload("7.2.0", &["7.0.0"]);
        preloaded["preload"]["audio"]["diff"] = serde_json::json!([audio_diff("en-us", "7.0.0")]);
        let root = manifest("7.1.0", &["7.0.0"], Some(preloaded));
        let plan = preload_plan_from_manifest(&root, "hk4e_global", Some("7.0.0"), &english())
            .unwrap()
            .unwrap();
        assert_eq!(plan.jobs[1].protocol, super::super::DownloadProtocol::Ldiff);
        assert_eq!(plan.jobs[1].from_version.as_deref(), Some("7.0.0"));
    }
}
