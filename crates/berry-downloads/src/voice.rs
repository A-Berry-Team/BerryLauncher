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

/// Voice-over codes to install, update or repair `game` with in `destination`.
pub fn selected_voice_packs(game: &GameCatalogEntry, destination: &Path) -> Vec<String> {
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
}
