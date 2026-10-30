use anyhow::{bail, Context, Result};
use reqwest::blocking::Client;
use reqwest::header::{ACCEPT_LANGUAGE, CONTENT_TYPE};
use serde_json::Value;
use std::collections::HashSet;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

use berry_games::catalog::GameCatalogEntry;

const PLAY_DETAILS_URL: &str = "https://play.google.com/store/apps/details";
const IMAGE_HOST: &str = "https://play-lh.googleusercontent.com/";
const IMAGE_SIZE: &str = "=s1280-rw";
const FALLBACK_LANGUAGE: &str = "en";
const FALLBACK_COUNTRY: &str = "US";
const REFRESH_INTERVAL: Duration = Duration::from_secs(6 * 60 * 60);
// Landscape images only: 16:9 is the target, 4:3 and square sets are discarded.
const MIN_ASPECT: f64 = 1.5;
const MAX_ASPECT: f64 = 2.5;
const MAX_IMAGES: usize = 24;
const PARALLEL_DOWNLOADS: usize = 6;
/// Largest screenshot accepted; anything bigger is not a store screenshot.
const MAX_IMAGE_BYTES: u64 = 16 * 1024 * 1024;

static REFRESH_LOCK: Mutex<()> = Mutex::new(());

#[derive(Debug, Clone, PartialEq, Eq)]
struct Locale {
    language: String,
    country: String,
}

impl Locale {
    fn current() -> Self {
        let (language, country) = berry_locale::play_locale();
        Self {
            language: language.to_lowercase(),
            country: country.to_uppercase(),
        }
    }

    fn fallback() -> Self {
        Self {
            language: FALLBACK_LANGUAGE.to_owned(),
            country: FALLBACK_COUNTRY.to_owned(),
        }
    }

    fn key(&self) -> String {
        format!("{}-{}", self.language, self.country).to_lowercase()
    }
}

#[derive(Debug)]
struct Candidate {
    id: String,
    base: String,
}

enum Verdict {
    Keep,
    Reject,
    Failed,
}

// Ordered ids of the images shown for one locale (`+id`) and the ids discarded for their aspect
// ratio (`-id`), so a discarded image is not downloaded again on every refresh.
#[derive(Debug, Default)]
struct Index {
    kept: Vec<String>,
    rejected: HashSet<String>,
}

impl Index {
    fn load(path: &Path) -> Option<Self> {
        let text = fs::read_to_string(path).ok()?;
        let mut index = Self::default();
        for line in text.lines() {
            if let Some(id) = line.strip_prefix('+') {
                index.kept.push(id.to_owned());
            } else if let Some(id) = line.strip_prefix('-') {
                index.rejected.insert(id.to_owned());
            }
        }
        Some(index)
    }

    fn save(&self, path: &Path) -> Result<()> {
        let mut text = String::new();
        for id in &self.kept {
            text.push('+');
            text.push_str(id);
            text.push('\n');
        }
        for id in &self.rejected {
            text.push('-');
            text.push_str(id);
            text.push('\n');
        }
        let temp = path.with_extension("tmp");
        fs::write(&temp, text).with_context(|| format!("writing {}", temp.display()))?;
        fs::rename(&temp, path).with_context(|| format!("replacing {}", path.display()))?;
        Ok(())
    }
}

fn gallery_dir(game_id: &str) -> PathBuf {
    super::backgrounds::background_dir(game_id).join("screenshots")
}

fn image_path(dir: &Path, id: &str) -> PathBuf {
    dir.join(format!("{id}.img"))
}

fn index_path(dir: &Path, locale: &Locale) -> PathBuf {
    dir.join(format!("index-{}.txt", locale.key()))
}

fn existing_paths(dir: &Path, index: &Index) -> Vec<PathBuf> {
    index
        .kept
        .iter()
        .map(|id| image_path(dir, id))
        .filter(|path| path.is_file())
        .collect()
}

fn indexes(dir: &Path) -> Vec<Index> {
    fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|entry| entry.file_name().to_string_lossy().starts_with("index-"))
        .filter_map(|entry| Index::load(&entry.path()))
        .collect()
}

fn is_fresh(path: &Path) -> bool {
    fs::metadata(path)
        .and_then(|meta| meta.modified())
        .ok()
        .and_then(|time| time.elapsed().ok())
        .is_some_and(|age| age < REFRESH_INTERVAL)
}

fn acceptable(width: u32, height: u32) -> bool {
    height > 0 && (MIN_ASPECT..=MAX_ASPECT).contains(&(width as f64 / height as f64))
}

/// Images already stored for the current language, or `None` if that language was never fetched.
pub fn cached(game: &GameCatalogEntry) -> Option<Vec<PathBuf>> {
    let dir = gallery_dir(game.id);
    let index = Index::load(&index_path(&dir, &Locale::current()))?;
    Some(existing_paths(&dir, &index))
}

/// Brings the stored images up to date with the Play Store listing (current language first,
/// English as fallback) and returns them in display order.
pub fn refresh(game: GameCatalogEntry) -> Result<Vec<PathBuf>> {
    let _guard = REFRESH_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let dir = gallery_dir(game.id);
    let locale = Locale::current();
    let index_file = index_path(&dir, &locale);
    let previous = Index::load(&index_file);
    if let Some(index) = previous.as_ref().filter(|_| is_fresh(&index_file)) {
        return Ok(existing_paths(&dir, index));
    }

    fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;
    let rejected: HashSet<String> = indexes(&dir)
        .into_iter()
        .flat_map(|index| index.rejected)
        .collect();
    let client = Client::builder()
        .user_agent(berry_core::http::BROWSER_USER_AGENT)
        .connect_timeout(Duration::from_secs(15))
        .timeout(Duration::from_secs(45))
        .build()?;

    let mut fetched = fetch(&client, game.play_id, &locale, &dir, &rejected);
    let fallback = Locale::fallback();
    if locale != fallback && fetched.as_ref().map_or(true, |index| index.kept.is_empty()) {
        fetched = fetch(&client, game.play_id, &fallback, &dir, &rejected);
    }

    match fetched {
        Ok(index) => {
            index.save(&index_file)?;
            prune(&dir);
            Ok(existing_paths(&dir, &index))
        }
        Err(error) => match previous {
            Some(index) => {
                tracing::warn!(
                    "[screenshots] {} could not be refreshed: {error:#}",
                    game.id
                );
                Ok(existing_paths(&dir, &index))
            }
            None => Err(error),
        },
    }
}

fn fetch(
    client: &Client,
    play_id: &str,
    locale: &Locale,
    dir: &Path,
    rejected: &HashSet<String>,
) -> Result<Index> {
    let html = client
        .get(PLAY_DETAILS_URL)
        .query(&[
            ("id", play_id),
            ("hl", locale.language.as_str()),
            ("gl", locale.country.as_str()),
        ])
        .header(ACCEPT_LANGUAGE, locale.language.as_str())
        .send()?
        .error_for_status()?
        .text()?;

    let mut candidates = screenshot_candidates(&html, play_id);
    if candidates.is_empty() {
        bail!(
            "no screenshots found on the Play Store page of {play_id} ({})",
            locale.key()
        );
    }
    candidates.truncate(MAX_IMAGES);

    let mut verdicts: Vec<Option<Verdict>> = candidates
        .iter()
        .map(|candidate| {
            if rejected.contains(&candidate.id) {
                Some(Verdict::Reject)
            } else if image_path(dir, &candidate.id).is_file() {
                Some(Verdict::Keep)
            } else {
                None
            }
        })
        .collect();

    let pending: Vec<usize> = (0..candidates.len())
        .filter(|&i| verdicts[i].is_none())
        .collect();
    for batch in pending.chunks(PARALLEL_DOWNLOADS) {
        let results: Vec<(usize, Verdict)> = std::thread::scope(|scope| {
            let handles: Vec<_> = batch
                .iter()
                .map(|&i| {
                    let candidate = &candidates[i];
                    (i, scope.spawn(move || download(client, candidate, dir)))
                })
                .collect();
            handles
                .into_iter()
                .map(|(i, handle)| (i, handle.join().unwrap_or(Verdict::Failed)))
                .collect()
        });
        for (i, verdict) in results {
            verdicts[i] = Some(verdict);
        }
    }

    let mut index = Index::default();
    for (candidate, verdict) in candidates.into_iter().zip(verdicts) {
        match verdict {
            Some(Verdict::Keep) => index.kept.push(candidate.id),
            Some(Verdict::Reject) => {
                index.rejected.insert(candidate.id);
            }
            Some(Verdict::Failed) | None => {}
        }
    }
    Ok(index)
}

fn download(client: &Client, candidate: &Candidate, dir: &Path) -> Verdict {
    match try_download(client, candidate, dir) {
        Ok(verdict) => verdict,
        Err(error) => {
            tracing::warn!(
                "[screenshots] could not download {}: {error:#}",
                candidate.id
            );
            Verdict::Failed
        }
    }
}

fn try_download(client: &Client, candidate: &Candidate, dir: &Path) -> Result<Verdict> {
    let response = client
        .get(format!("{}{IMAGE_SIZE}", candidate.base))
        .send()?
        .error_for_status()?;
    let is_image = response
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.starts_with("image/"));
    if !is_image {
        bail!("the response is not an image");
    }
    if response
        .content_length()
        .is_some_and(|length| length > MAX_IMAGE_BYTES)
    {
        bail!("the image is larger than {MAX_IMAGE_BYTES} bytes");
    }
    let mut bytes =
        Vec::with_capacity(response.content_length().unwrap_or(0).min(MAX_IMAGE_BYTES) as usize);
    response.take(MAX_IMAGE_BYTES + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_IMAGE_BYTES {
        bail!("the image is larger than {MAX_IMAGE_BYTES} bytes");
    }

    let path = image_path(dir, &candidate.id);
    let temp = path.with_extension("download");
    fs::write(&temp, &bytes).with_context(|| format!("writing {}", temp.display()))?;
    let dimensions = crate::images::webp_dimensions(&bytes).or_else(|| {
        gdk_pixbuf::Pixbuf::file_info(&temp).map(|(_, width, height)| (width as u32, height as u32))
    });
    if dimensions.is_some_and(|(width, height)| !acceptable(width, height)) {
        let _ = fs::remove_file(&temp);
        return Ok(Verdict::Reject);
    }
    fs::rename(&temp, &path).with_context(|| format!("replacing {}", path.display()))?;
    Ok(Verdict::Keep)
}

// Drops images no language refers to any more, and leftovers of interrupted downloads.
fn prune(dir: &Path) {
    let referenced: HashSet<String> = indexes(dir)
        .into_iter()
        .flat_map(|index| index.kept)
        .collect();
    for entry in fs::read_dir(dir).into_iter().flatten().flatten() {
        let path = entry.path();
        let stale = match path.extension().and_then(|extension| extension.to_str()) {
            Some("img") => path
                .file_stem()
                .and_then(|stem| stem.to_str())
                .is_some_and(|stem| !referenced.contains(stem)),
            Some("download") => true,
            _ => false,
        };
        if stale {
            let _ = fs::remove_file(path);
        }
    }
}

// Only the images of the app's own listing count: the page also shows icons, thumbnails of other
// games ("similar games") and more, none of which may end up in the gallery.
fn screenshot_candidates(html: &str, play_id: &str) -> Vec<Candidate> {
    let mut found = Vec::new();
    for block in data_blocks(html)
        .into_iter()
        .filter(|block| block.contains(play_id))
    {
        if let Ok(value) = serde_json::from_str::<Value>(block) {
            collect_media(&value, &mut found);
        }
    }
    if found.is_empty() {
        for tag in html.split("<img").skip(1) {
            let tag = tag.split('>').next().unwrap_or_default();
            let alt = attribute(tag, "alt").unwrap_or_default().to_lowercase();
            if !(alt.contains("screenshot") || alt.contains("captura de pantalla")) {
                continue;
            }
            if let Some(candidate) = attribute(tag, "src").and_then(candidate) {
                push_unique(&mut found, candidate);
            }
        }
    }
    found
}

// Screenshot lists are arrays of media entries whose `[3][2]` holds the image URL. Their position
// in the page data shifts between Play Store rollouts, so they are found by shape instead.
fn collect_media(value: &Value, out: &mut Vec<Candidate>) {
    let Value::Array(items) = value else {
        return;
    };
    if items.len() >= 2 && items.iter().all(|item| media_url(item).is_some()) {
        for url in items.iter().filter_map(media_url) {
            if let Some(candidate) = candidate(url) {
                push_unique(out, candidate);
            }
        }
        return;
    }
    for item in items {
        collect_media(item, out);
    }
}

fn media_url(item: &Value) -> Option<&str> {
    item.get(3)?
        .get(2)?
        .as_str()
        .filter(|url| url.starts_with(IMAGE_HOST))
}

fn candidate(url: &str) -> Option<Candidate> {
    let base = url.split('=').next()?;
    let id: String = base
        .strip_prefix(IMAGE_HOST)?
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))
        .take(64)
        .collect();
    (!id.is_empty()).then(|| Candidate {
        id,
        base: base.to_owned(),
    })
}

fn push_unique(out: &mut Vec<Candidate>, candidate: Candidate) {
    if !out.iter().any(|known| known.id == candidate.id) {
        out.push(candidate);
    }
}

fn attribute<'a>(tag: &'a str, name: &str) -> Option<&'a str> {
    let needle = format!("{name}=\"");
    tag[tag.find(&needle)? + needle.len()..].split('"').next()
}

// The JSON payloads of the `AF_initDataCallback({key: 'ds:N', ..., data: [...]})` calls.
fn data_blocks(html: &str) -> Vec<&str> {
    const MARKER: &str = "AF_initDataCallback(";
    let mut blocks = Vec::new();
    let mut rest = html;
    while let Some(at) = rest.find(MARKER) {
        rest = &rest[at + MARKER.len()..];
        let Some(data) = rest.find("data:") else {
            break;
        };
        let after = &rest[data + "data:".len()..];
        let Some(start) = after.find('[') else {
            break;
        };
        let tail = &after[start..];
        match array_len(tail) {
            Some(len) => {
                blocks.push(&tail[..len]);
                rest = &tail[len..];
            }
            None => rest = tail,
        }
    }
    blocks
}

// Length of the JSON array that starts `text`, honouring brackets inside strings.
fn array_len(text: &str) -> Option<usize> {
    let (mut depth, mut in_string, mut escaped) = (0usize, false, false);
    for (offset, byte) in text.bytes().enumerate() {
        if in_string {
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == b'"' {
                in_string = false;
            }
            continue;
        }
        match byte {
            b'"' => in_string = true,
            b'[' => depth += 1,
            b']' => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return Some(offset + 1);
                }
            }
            _ => {}
        }
    }
    None
}
