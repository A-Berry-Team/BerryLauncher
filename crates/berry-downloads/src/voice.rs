//! The voice-over packs chosen for a game, remembered next to its files.
//!
//! Updates, pre-downloads, repairs and a resumed install must all keep working with the packs the
//! user picked when installing, so the choice lives in the game directory (`.berry-voice-packs`)
//! rather than in the UI. Installs made before the choice existed have no such file; they keep the
//! old behaviour, English only. An existing file with no language in it is a real choice too: it
//! means "none", and the game then downloads its own default voice-over the first time it starts.

use std::fs;
use std::path::Path;

use anyhow::{Context, Result};
use berry_games::catalog::GameCatalogEntry;

pub const SELECTION_FILE: &str = ".berry-voice-packs";
const HEADER: &str = "berry-voice-packs v1";

fn render(codes: &[String]) -> String {
    let mut text = String::from(HEADER);
    text.push('\n');
    for code in codes {
        text.push_str(code);
        text.push('\n');
    }
    text
}

/// `None` when the text is not a selection file (wrong or missing header).
fn parse(text: &str) -> Option<Vec<String>> {
    let mut lines = text.lines();
    if lines.next()?.trim() != HEADER {
        return None;
    }
    Some(
        lines
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .map(str::to_owned)
            .collect(),
    )
}

/// Voice-over languages a game has installed, read from the `Audio_<Language>_pkg_version`
/// manifests every installed voice pack leaves in the game folder (`Audio_Japanese_pkg_version`...).
/// `None` for games without selectable voice packs.
fn installed_voice_packs(game: &GameCatalogEntry, destination: &Path) -> Option<Vec<String>> {
    const AUDIO_MANIFESTS: [(&str, &str); 4] = [
        ("en-us", "Audio_English(US)_pkg_version"),
        ("ja-jp", "Audio_Japanese_pkg_version"),
        ("ko-kr", "Audio_Korean_pkg_version"),
        ("zh-cn", "Audio_Chinese_pkg_version"),
    ];
    if game.voice_packs().is_empty() {
        return None;
    }
    let found: Vec<&str> = AUDIO_MANIFESTS
        .iter()
        .filter(|(_, file)| destination.join(file).is_file())
        .map(|(code, _)| *code)
        .collect();
    Some(game.normalize_voice_packs(&found))
}

/// Voice-over codes to install, update or repair `game` with in `destination`.
///
/// An installed game is described by its audio manifests, which are the source of truth: only the languages
/// it already has are downloaded again, and none at all when it was installed without voice-over
/// (the game fetches its own the first time it starts). English is never assumed for it.
pub fn selected_voice_packs(game: &GameCatalogEntry, destination: &Path) -> Vec<String> {
    if crate::installed_version(destination).is_some() {
        if let Some(installed) = installed_voice_packs(game, destination) {
            return installed;
        }
    }
    match fs::read_to_string(destination.join(SELECTION_FILE))
        .ok()
        .and_then(|text| parse(&text))
    {
        Some(codes) => game.normalize_voice_packs(&codes),
        None => game.default_voice_packs(),
    }
}

/// Remember the packs chosen for `game`. Codes the game does not offer are dropped.
pub fn save_voice_packs(
    game: &GameCatalogEntry,
    destination: &Path,
    codes: &[String],
) -> Result<()> {
    fs::create_dir_all(destination)
        .with_context(|| format!("creating {}", destination.display()))?;
    let codes = game.normalize_voice_packs(codes);
    berry_installer::fsutil::atomic_write(
        &destination.join(SELECTION_FILE),
        render(&codes).as_bytes(),
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use berry_games::catalog::by_id;

    fn codes(list: &[&str]) -> Vec<String> {
        list.iter().map(|code| (*code).to_owned()).collect()
    }

    #[test]
    fn a_selection_survives_a_round_trip() {
        let text = render(&codes(&["en-us", "ja-jp"]));
        assert_eq!(parse(&text), Some(codes(&["en-us", "ja-jp"])));
    }

    #[test]
    fn an_empty_selection_is_a_selection() {
        assert_eq!(parse(&render(&[])), Some(Vec::new()));
    }

    #[test]
    fn foreign_text_is_not_a_selection() {
        assert_eq!(parse(""), None);
        assert_eq!(parse("en-us\n"), None);
    }

    #[test]
    fn without_a_file_english_stays_the_default() {
        let dir = tempfile::tempdir().unwrap();
        let game = by_id("genshin-impact").unwrap();
        assert_eq!(selected_voice_packs(game, dir.path()), ["en-us"]);
    }

    #[test]
    fn the_saved_choice_wins_even_when_it_is_empty() {
        let dir = tempfile::tempdir().unwrap();
        let game = by_id("honkai-star-rail").unwrap();

        save_voice_packs(game, dir.path(), &codes(&["zh-cn", "en-us", "xx-xx"])).unwrap();
        assert_eq!(selected_voice_packs(game, dir.path()), ["en-us", "zh-cn"]);

        save_voice_packs(game, dir.path(), &[]).unwrap();
        assert!(selected_voice_packs(game, dir.path()).is_empty());
    }

    #[test]
    fn games_without_voice_packs_never_get_one() {
        let dir = tempfile::tempdir().unwrap();
        let game = by_id("honkai-impact-3rd").unwrap();
        save_voice_packs(game, dir.path(), &codes(&["en-us"])).unwrap();
        assert!(selected_voice_packs(game, dir.path()).is_empty());
    }

    fn installed_genshin(files: &[&str]) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join(".berry-version"), "6.0.0").unwrap();
        for file in files {
            fs::write(dir.path().join(file), "").unwrap();
        }
        dir
    }

    #[test]
    fn an_update_keeps_only_the_installed_audio_even_without_a_selection_file() {
        let game = by_id("genshin-impact").unwrap();
        let dir = installed_genshin(&["Audio_Japanese_pkg_version"]);
        assert_eq!(selected_voice_packs(game, dir.path()), ["ja-jp"]);
    }

    #[test]
    fn an_update_downloads_every_installed_audio() {
        let game = by_id("genshin-impact").unwrap();
        let dir = installed_genshin(&["Audio_Korean_pkg_version", "Audio_English(US)_pkg_version"]);
        assert_eq!(selected_voice_packs(game, dir.path()), ["en-us", "ko-kr"]);
    }

    #[test]
    fn an_install_without_audio_never_gets_english_on_update() {
        let game = by_id("genshin-impact").unwrap();
        let dir = installed_genshin(&[]);
        // Even a stale selection file asking for English must not win over the disk.
        save_voice_packs(game, dir.path(), &codes(&["en-us"])).unwrap();
        assert!(selected_voice_packs(game, dir.path()).is_empty());
    }

    #[test]
    fn a_fresh_install_still_uses_the_chosen_packs() {
        let game = by_id("genshin-impact").unwrap();
        let dir = tempfile::tempdir().unwrap();
        save_voice_packs(game, dir.path(), &codes(&["ja-jp"])).unwrap();
        assert_eq!(selected_voice_packs(game, dir.path()), ["ja-jp"]);
    }
}
