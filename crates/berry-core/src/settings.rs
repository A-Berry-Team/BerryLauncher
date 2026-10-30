use std::fs;
use std::path::PathBuf;

const APP_ID: &str = "app.berrylauncher.berry";

/// Value of the language preference that follows the system locale.
pub const SYSTEM_LANGUAGE: &str = "system";

pub fn data_dir() -> PathBuf {
    glib::user_data_dir().join("BerryLauncher")
}

pub fn launcher_dir() -> PathBuf {
    if in_flatpak() {
        glib::home_dir().join(".var").join("app").join(APP_ID)
    } else {
        data_dir()
    }
}

pub fn prefixes_dir() -> PathBuf {
    data_dir().join("prefixes")
}

pub fn extras_dir() -> PathBuf {
    data_dir().join("extras")
}

pub fn runners_dir() -> PathBuf {
    data_dir().join("runners")
}

pub fn logs_dir() -> PathBuf {
    data_dir().join("logs")
}

pub fn preferences_path() -> PathBuf {
    data_dir().join("preferences.ini")
}

pub fn in_flatpak() -> bool {
    std::path::Path::new("/.flatpak-info").exists()
}

fn env_flag_value(value: &str) -> bool {
    matches!(
        value.trim().to_ascii_lowercase().as_str(),
        "1" | "true" | "yes" | "on"
    )
}

/// An explicit per-game value takes precedence over the launcher's environment. Presence matters:
/// an explicit zero is an override too, not the absence of an override.
pub fn launch_on_host_override(
    game_env: &std::collections::BTreeMap<String, String>,
) -> Option<bool> {
    game_env
        .get("BERRY_LAUNCH_ON_HOST")
        .map(|value| env_flag_value(value))
        .or_else(|| {
            std::env::var("BERRY_LAUNCH_ON_HOST")
                .ok()
                .map(|value| env_flag_value(&value))
        })
}

fn flag_enabled(name: &str, game_env: &std::collections::BTreeMap<String, String>) -> bool {
    game_env
        .get(name)
        .map(|value| env_flag_value(value))
        .or_else(|| std::env::var(name).ok().map(|value| env_flag_value(&value)))
        .unwrap_or(false)
}

/// Whether the "launch on the host" option can mean anything in this build.
///
/// It only exists for the Flatpak package: the sandbox is the only place a game can be kept
/// away from the system. A native package (rpm / deb) already runs with the user's own
/// permissions, so the game always runs on the system and the option is meaningless there.
pub fn host_launch_supported() -> bool {
    in_flatpak()
}

/// Whether the game (and the helper tools around it) must be started on the host through
/// `flatpak-spawn --host`.
///
/// `requested` is the stored per-game choice to leave the sandbox: the "Run in the Flatpak
/// sandbox" switch turned off (that switch is on by default, so this is `false`). By default a
/// Flatpak build runs the game inside its own sandbox, as a child of the launcher (like AAGL or
/// TwinTail do): the game belongs to the launcher's Flatpak instance, so the launcher stays alive without a
/// window until the game ends (see `GameLauncher::needs_launcher_alive`). Turning
/// the switch off spawns on the host instead. `BERRY_LAUNCH_ON_HOST=1` (launcher environment or
/// the game's custom environment variables) still forces it, as an override for scripts.
///
/// Always `false` outside Flatpak, whatever was stored or requested.
pub fn launch_on_host_for(
    requested: bool,
    game_env: &std::collections::BTreeMap<String, String>,
) -> bool {
    if !host_launch_supported() {
        return false;
    }
    launch_on_host_override(game_env).unwrap_or(requested)
}

/// Same as `launch_on_host_for`, looking only at the launcher's own environment.
pub fn launch_on_host() -> bool {
    launch_on_host_for(false, &std::collections::BTreeMap::new())
}

/// Whether Proton is started inside the Steam Linux Runtime container (pressure-vessel).
///
/// Natively and on the host it always is. Inside the Flatpak sandbox pressure-vessel has to
/// ask the Flatpak portal for a sub-sandbox, which needs the app's runtime to be installed in
/// the portal's installation (it is not when running from GNOME Builder)
/// and can reject some environment edits. So there Proton runs directly in
/// the launcher's sandbox, like AAGL does. `BERRY_FLATPAK_USE_RUNTIME=1` forces the container.
pub fn use_steam_runtime_container(
    on_host: bool,
    game_env: &std::collections::BTreeMap<String, String>,
) -> bool {
    !in_flatpak() || on_host || flag_enabled("BERRY_FLATPAK_USE_RUNTIME", game_env)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Theme {
    System,
    Light,
    Dark,
}

impl Theme {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::Light => "light",
            Self::Dark => "dark",
        }
    }

    pub fn from_str(value: &str) -> Self {
        match value {
            "light" => Self::Light,
            "dark" => Self::Dark,
            _ => Self::System,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LauncherBehavior {
    Hide,
    KeepOpen,
    Close,
}

impl LauncherBehavior {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Hide => "hide",
            Self::KeepOpen => "keep-open",
            Self::Close => "close",
        }
    }

    pub fn from_str(value: &str) -> Self {
        match value {
            "keep-open" => Self::KeepOpen,
            "close" => Self::Close,
            _ => Self::Hide,
        }
    }

    pub fn index(self) -> usize {
        match self {
            Self::Hide => 0,
            Self::KeepOpen => 1,
            Self::Close => 2,
        }
    }

    pub fn from_index(index: usize) -> Self {
        match index {
            1 => Self::KeepOpen,
            2 => Self::Close,
            _ => Self::Hide,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Preferences {
    pub theme: Theme,
    pub language: String,
    pub animated_background: bool,
    pub launcher_behavior: LauncherBehavior,
    /// Open the page of the game that was being viewed last time when the launcher starts.
    pub restore_last_game: bool,
    /// Ask before closing the launcher while a game is being installed, updated or repaired.
    pub confirm_quit_while_downloading: bool,
    /// Pause running downloads when a game starts, so they do not compete with it.
    pub pause_downloads_on_launch: bool,
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            theme: Theme::System,
            language: SYSTEM_LANGUAGE.to_string(),
            animated_background: true,
            launcher_behavior: LauncherBehavior::Hide,
            restore_last_game: true,
            confirm_quit_while_downloading: true,
            pause_downloads_on_launch: false,
        }
    }
}

pub fn load_preferences() -> Preferences {
    let path = preferences_path();
    let key_file = glib::KeyFile::new();
    if key_file
        .load_from_file(&path, glib::KeyFileFlags::NONE)
        .is_err()
    {
        return Preferences::default();
    }

    Preferences {
        theme: Theme::from_str(
            key_file
                .string("Preferences", "theme")
                .as_deref()
                .unwrap_or("system"),
        ),
        language: key_file
            .string("Preferences", "language")
            .map(|value| value.to_string())
            .unwrap_or_else(|_| SYSTEM_LANGUAGE.to_string()),
        animated_background: key_file
            .boolean("Preferences", "animated_background")
            .unwrap_or(true),
        launcher_behavior: LauncherBehavior::from_str(
            key_file
                .string("Preferences", "launcher_behavior")
                .as_deref()
                .unwrap_or("hide"),
        ),
        restore_last_game: key_file
            .boolean("Preferences", "restore_last_game")
            .unwrap_or(true),
        confirm_quit_while_downloading: key_file
            .boolean("Preferences", "confirm_quit_while_downloading")
            .unwrap_or(true),
        pause_downloads_on_launch: key_file
            .boolean("Preferences", "pause_downloads_on_launch")
            .unwrap_or(false),
    }
}

pub fn save_preferences(preferences: &Preferences) -> anyhow::Result<()> {
    fs::create_dir_all(data_dir())?;
    let key_file = glib::KeyFile::new();
    key_file.set_string("Preferences", "theme", preferences.theme.as_str());
    key_file.set_string("Preferences", "language", &preferences.language);
    key_file.set_boolean(
        "Preferences",
        "animated_background",
        preferences.animated_background,
    );
    key_file.set_string(
        "Preferences",
        "launcher_behavior",
        preferences.launcher_behavior.as_str(),
    );
    key_file.set_boolean(
        "Preferences",
        "restore_last_game",
        preferences.restore_last_game,
    );
    key_file.set_boolean(
        "Preferences",
        "confirm_quit_while_downloading",
        preferences.confirm_quit_while_downloading,
    );
    key_file.set_boolean(
        "Preferences",
        "pause_downloads_on_launch",
        preferences.pause_downloads_on_launch,
    );
    key_file.save_to_file(preferences_path())?;
    Ok(())
}

// Moves the pre-0.2 layout from the XDG data root into data_dir().
pub fn migrate_legacy_data() -> anyhow::Result<()> {
    fs::create_dir_all(data_dir())?;
    let legacy_root = glib::user_data_dir();

    for name in ["games", "game-settings", "runners", "resources"] {
        let old = legacy_root.join(name);
        let new = data_dir().join(name);
        if !old.exists() || old == new {
            continue;
        }

        if new.exists() {
            merge_directory(&old, &new)?;
            if old.is_dir() {
                fs::remove_dir_all(&old)?;
            }
        } else {
            fs::rename(&old, &new)?;
        }
    }

    Ok(())
}

fn merge_directory(source: &PathBuf, destination: &PathBuf) -> anyhow::Result<()> {
    fs::create_dir_all(destination)?;
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let source_path = entry.path();
        let destination_path = destination.join(entry.file_name());
        if source_path.is_dir() {
            if destination_path.exists() {
                merge_directory(&source_path, &destination_path)?;
                fs::remove_dir_all(&source_path)?;
            } else {
                fs::rename(&source_path, &destination_path)?;
            }
        } else if !destination_path.exists() {
            fs::rename(&source_path, &destination_path)?;
        }
    }
    Ok(())
}
