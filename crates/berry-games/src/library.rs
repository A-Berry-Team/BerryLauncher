use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DxvkHud {
    None,
    Fps,
    FpsFrametime,
    FpsFrametimeGpu,
    Full,
}

impl DxvkHud {
    pub const OPTIONS: &[Self] = &[
        Self::None,
        Self::Fps,
        Self::FpsFrametime,
        Self::FpsFrametimeGpu,
        Self::Full,
    ];

    pub fn options() -> impl Iterator<Item = Self> {
        Self::OPTIONS.iter().copied()
    }

    /// Fluent message id of the drop-down label (translated by the application).
    pub fn label_id(self) -> &'static str {
        match self {
            Self::None => "settings-dxvk-hud-none",
            Self::Fps => "settings-dxvk-hud-fps",
            Self::FpsFrametime => "settings-dxvk-hud-fps-frametime",
            Self::FpsFrametimeGpu => "settings-dxvk-hud-fps-frametime-gpu",
            Self::Full => "settings-dxvk-hud-full",
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Fps => "fps",
            Self::FpsFrametime => "fps,frametimes",
            Self::FpsFrametimeGpu => "fps,frametimes,devinfo,gpuload",
            Self::Full => "full",
        }
    }

    pub fn from_str(value: &str) -> Self {
        match value {
            "fps" => Self::Fps,
            "fps,frametimes" => Self::FpsFrametime,
            "fps,frametimes,devinfo,gpuload" => Self::FpsFrametimeGpu,
            "full" => Self::Full,
            _ => Self::None,
        }
    }

    pub fn index(self) -> usize {
        Self::OPTIONS
            .iter()
            .position(|value| *value == self)
            .unwrap_or(0)
    }

    pub fn from_index(index: usize) -> Self {
        Self::OPTIONS.get(index).copied().unwrap_or(Self::None)
    }
}

/// Graphics API offered by Wuthering Waves on PC. DirectX 12 is the game's default.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum GraphicsApi {
    #[default]
    Dx12,
    Dx11,
}

impl GraphicsApi {
    pub const OPTIONS: &[Self] = &[Self::Dx12, Self::Dx11];

    pub fn options() -> impl Iterator<Item = Self> {
        Self::OPTIONS.iter().copied()
    }

    /// Fluent message id of the drop-down label (translated by the application).
    pub fn label_id(self) -> &'static str {
        match self {
            Self::Dx12 => "settings-graphics-api-dx12",
            Self::Dx11 => "settings-graphics-api-dx11",
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Dx12 => "dx12",
            Self::Dx11 => "dx11",
        }
    }

    pub fn from_str(value: &str) -> Self {
        match value.trim().to_ascii_lowercase().as_str() {
            "dx11" => Self::Dx11,
            _ => Self::Dx12,
        }
    }

    /// Command-line switch understood by Unreal Engine / Wuthering Waves.
    pub fn launch_arg(self) -> &'static str {
        match self {
            Self::Dx12 => "-dx12",
            Self::Dx11 => "-dx11",
        }
    }

    pub fn index(self) -> usize {
        Self::OPTIONS
            .iter()
            .position(|value| *value == self)
            .unwrap_or(0)
    }

    pub fn from_index(index: usize) -> Self {
        Self::OPTIONS.get(index).copied().unwrap_or_default()
    }
}

/// Declares a small settings enum that is stored as a string and shown in a drop-down.
macro_rules! choice_enum {
    ($(#[$meta:meta])* $name:ident, default $def:ident, { $($var:ident => ($key:literal, $label:tt)),+ $(,)? }) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub enum $name { $($var),+ }

        impl Default for $name {
            fn default() -> Self { Self::$def }
        }

        impl $name {
            pub const OPTIONS: &[Self] = &[$(Self::$var),+];

            pub fn options() -> impl Iterator<Item = Self> {
                Self::OPTIONS.iter().copied()
            }

            /// Fluent message id of the drop-down label (translated by the application).
    pub fn label_id(self) -> &'static str {
                match self { $(Self::$var => $label),+ }
            }

            pub fn as_str(self) -> &'static str {
                match self { $(Self::$var => $key),+ }
            }

            pub fn from_str(value: &str) -> Self {
                match value.trim().to_ascii_lowercase().as_str() {
                    $($key => Self::$var,)+
                    _ => Self::$def,
                }
            }

            pub fn index(self) -> usize {
                Self::OPTIONS.iter().position(|value| *value == self).unwrap_or(0)
            }

            pub fn from_index(index: usize) -> Self {
                Self::OPTIONS.get(index).copied().unwrap_or_default()
            }
        }
    };
}

choice_enum!(
    /// AMD FidelityFX Super Resolution preset of the Wine fullscreen hack (same presets as AAGL).
    FsrQuality, default Balanced, {
        UltraQuality => ("ultra", "settings-fsr-ultra"),
        Quality => ("quality", "settings-fsr-quality"),
        Balanced => ("balanced", "settings-fsr-balanced"),
        Performance => ("performance", "settings-fsr-performance"),
    }
);

choice_enum!(
    /// Wine synchronization primitive. See [`SyncMode::system_default`] for the default.
    SyncMode, default FSync, {
        NtSync => ("ntsync", "settings-sync-ntsync"),
        FSync => ("fsync", "settings-sync-fsync"),
        ESync => ("esync", "settings-sync-esync"),
        None => ("none", "settings-sync-none"),
    }
);

impl SyncMode {
    /// NTSync on systems that expose a usable `/dev/ntsync`, FSync everywhere else.
    pub fn system_default() -> Self {
        if berry_core::system::ntsync_available() {
            Self::NtSync
        } else {
            Self::FSync
        }
    }

    pub fn display(self) -> &'static str {
        match self {
            Self::NtSync => "NTSync",
            Self::FSync => "FSync",
            Self::ESync => "ESync",
            Self::None => "none",
        }
    }
}

choice_enum!(
    GamescopeMode, default Fullscreen, {
        Fullscreen => ("fullscreen", "gamescope-mode-fullscreen"),
        Borderless => ("borderless", "gamescope-mode-borderless"),
        Windowed => ("windowed", "gamescope-mode-windowed"),
    }
);

choice_enum!(
    GamescopeFilter, default Default, {
        Default => ("default", "gamescope-filter-default"),
        Fsr => ("fsr", "gamescope-filter-fsr"),
        Nis => ("nis", "gamescope-filter-nis"),
        Linear => ("linear", "gamescope-filter-linear"),
        Nearest => ("nearest", "gamescope-filter-nearest"),
    }
);

/// Gamescope options. A value of `0` means "let gamescope decide" for every number.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GamescopeSettings {
    pub enabled: bool,
    pub width: u32,
    pub height: u32,
    pub game_width: u32,
    pub game_height: u32,
    pub fps: u32,
    pub fps_unfocused: u32,
    pub mode: GamescopeMode,
    pub filter: GamescopeFilter,
    pub sharpness: u32,
    pub integer_scaling: bool,
    pub hdr: bool,
    pub force_grab_cursor: bool,
    pub extra_args: String,
}

impl Default for GamescopeSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            width: 0,
            height: 0,
            game_width: 0,
            game_height: 0,
            fps: 0,
            fps_unfocused: 0,
            mode: GamescopeMode::Fullscreen,
            filter: GamescopeFilter::Default,
            sharpness: 5,
            integer_scaling: false,
            hdr: false,
            force_grab_cursor: false,
            extra_args: String::new(),
        }
    }
}

impl GamescopeSettings {
    /// Command-line arguments for `gamescope`, without the trailing `--`.
    pub fn args(&self) -> Vec<String> {
        let mut args: Vec<String> = Vec::new();
        if self.width > 0 && self.height > 0 {
            args.extend([
                "-W".into(),
                self.width.to_string(),
                "-H".into(),
                self.height.to_string(),
            ]);
        }
        if self.game_width > 0 && self.game_height > 0 {
            args.extend([
                "-w".into(),
                self.game_width.to_string(),
                "-h".into(),
                self.game_height.to_string(),
            ]);
        }
        if self.fps > 0 {
            args.extend(["-r".into(), self.fps.to_string()]);
        }
        if self.fps_unfocused > 0 {
            args.extend(["-o".into(), self.fps_unfocused.to_string()]);
        }
        match self.mode {
            GamescopeMode::Fullscreen => args.push("-f".into()),
            GamescopeMode::Borderless => args.push("-b".into()),
            GamescopeMode::Windowed => {}
        }
        if self.filter != GamescopeFilter::Default {
            args.extend(["-F".into(), self.filter.as_str().into()]);
            if matches!(self.filter, GamescopeFilter::Fsr | GamescopeFilter::Nis) {
                args.extend(["--sharpness".into(), self.sharpness.min(20).to_string()]);
            }
        }
        if self.integer_scaling {
            args.extend(["-S".into(), "integer".into()]);
        }
        if self.hdr {
            args.push("--hdr-enabled".into());
        }
        if self.force_grab_cursor {
            args.push("--force-grab-cursor".into());
        }
        args.extend(self.extra_args.split_whitespace().map(str::to_owned));
        args
    }
}

/// Default `refresh_delay_ms` of the FPS unlocker: how often it re-applies the FPS value.
pub const DEFAULT_FPS_REFRESH_DELAY_MS: u32 = 2000;
/// Default `init_delay_ms` of the FPS unlocker: wait before it looks for the game's FPS setting.
pub const DEFAULT_FPS_INIT_DELAY_MS: u32 = 600;

#[derive(Debug, Clone)]
pub struct GameSettings {
    pub path: PathBuf,
    pub runner: Option<String>,
    pub gamemode: bool,
    pub dxvk_hud: DxvkHud,
    pub fps_unlocker: bool,
    pub fps: u32,
    /// FPS unlocker `refresh_delay_ms` (stored as `fps_unlocker_interval`).
    pub fps_interval_ms: u32,
    /// FPS unlocker `init_delay_ms`.
    pub fps_init_delay_ms: u32,
    /// Only used by Wuthering Waves.
    pub graphics_api: GraphicsApi,
    /// FSR through the Wine fullscreen hack (enabled by default, like AAGL).
    pub fsr: bool,
    pub fsr_quality: FsrQuality,
    pub sync: SyncMode,
    pub gamescope: GamescopeSettings,
    /// Flatpak only: start the game on the host (`flatpak-spawn --host`) instead of inside the
    /// launcher's sandbox. Off by default (the settings window shows it inverted, as "Run in the
    /// Flatpak sandbox", on by default); ignored by native packages.
    pub launch_on_host: bool,
    /// Extra command-line arguments appended after the game executable.
    pub launch_args: String,
    /// Program (and its arguments) that wraps the whole launch, e.g. `mangohud`.
    pub wrapper: String,
    /// Ask hybrid-graphics machines to render on the dedicated GPU.
    pub dedicated_gpu: bool,
    /// Write a detailed Wine/Proton log (`PROTON_LOG=1`) next to the prefix.
    pub proton_log: bool,
    pub environment: BTreeMap<String, String>,
}

impl Default for GameSettings {
    fn default() -> Self {
        Self {
            path: PathBuf::new(),
            runner: None,
            gamemode: false,
            dxvk_hud: DxvkHud::None,
            fps_unlocker: false,
            fps: 120,
            fps_interval_ms: DEFAULT_FPS_REFRESH_DELAY_MS,
            fps_init_delay_ms: DEFAULT_FPS_INIT_DELAY_MS,
            graphics_api: GraphicsApi::default(),
            fsr: true,
            fsr_quality: FsrQuality::Balanced,
            sync: SyncMode::system_default(),
            gamescope: GamescopeSettings::default(),
            launch_on_host: false,
            launch_args: String::new(),
            wrapper: String::new(),
            dedicated_gpu: false,
            proton_log: false,
            environment: BTreeMap::new(),
        }
    }
}

/// Splits a command line typed by the user into arguments. Single and double quotes group words
/// (`-title "My Game"`), a backslash escapes the next character outside single quotes. No shell is
/// involved, so nothing is expanded.
pub fn split_command(text: &str) -> Vec<String> {
    let mut args = Vec::new();
    let mut current = String::new();
    let mut started = false;
    let mut quote: Option<char> = None;
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (Some('"'), '\\') => match chars.next() {
                Some(next @ ('"' | '\\')) => current.push(next),
                Some(next) => {
                    current.push('\\');
                    current.push(next);
                }
                None => current.push('\\'),
            },
            (Some(_), c) => current.push(c),
            (None, '\'' | '"') => {
                quote = Some(c);
                started = true;
            }
            (None, '\\') => {
                if let Some(next) = chars.next() {
                    current.push(next);
                    started = true;
                }
            }
            (None, c) if c.is_whitespace() => {
                if started {
                    args.push(std::mem::take(&mut current));
                    started = false;
                }
            }
            (None, c) => {
                current.push(c);
                started = true;
            }
        }
    }
    if started {
        args.push(current);
    }
    args
}

pub fn games_dir() -> PathBuf {
    berry_core::settings::data_dir().join("games")
}

pub fn game_path(folder_name: &str) -> PathBuf {
    games_dir().join(folder_name)
}

pub fn settings_path(game_id: &str) -> PathBuf {
    berry_core::settings::data_dir()
        .join("game-settings")
        .join(format!("{game_id}.ini"))
}

pub fn ensure_games_dir() -> anyhow::Result<()> {
    fs::create_dir_all(games_dir())?;
    Ok(())
}

pub fn is_in_library(folder_name: &str) -> bool {
    game_path(folder_name).is_dir()
}

pub fn add_to_library(folder_name: &str) -> anyhow::Result<()> {
    ensure_games_dir()?;
    fs::create_dir_all(game_path(folder_name))?;
    Ok(())
}

pub fn remove_from_library(folder_name: &str) -> anyhow::Result<()> {
    let path = game_path(folder_name);
    if path.exists() {
        fs::remove_dir_all(path)?;
    }
    Ok(())
}

pub fn last_game_id() -> Option<String> {
    let path = games_dir().join(".last-game");
    fs::read_to_string(path)
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

pub fn set_last_game_id(game_id: &str) -> anyhow::Result<()> {
    ensure_games_dir()?;
    fs::write(games_dir().join(".last-game"), game_id)?;
    Ok(())
}

pub fn load_settings(game_id: &str, default_path: PathBuf) -> GameSettings {
    let mut settings = GameSettings {
        path: default_path,
        ..Default::default()
    };
    let path = settings_path(game_id);
    let key_file = glib::KeyFile::new();
    if key_file
        .load_from_file(&path, glib::KeyFileFlags::NONE)
        .is_err()
    {
        return settings;
    }

    settings.gamemode = key_file.boolean("Game", "gamemode").unwrap_or(false);
    settings.dxvk_hud = DxvkHud::from_str(
        key_file
            .string("Game", "dxvk_hud")
            .as_deref()
            .unwrap_or("none"),
    );
    settings.fps_unlocker = key_file.boolean("Game", "fps_unlocker").unwrap_or(false);
    settings.fps = key_file
        .integer("Game", "fps_unlocker_fps")
        .unwrap_or(120)
        .clamp(30, 1000) as u32;
    settings.fps_interval_ms = key_file
        .integer("Game", "fps_unlocker_interval")
        .unwrap_or(DEFAULT_FPS_REFRESH_DELAY_MS as i32)
        .clamp(100, 60000) as u32;
    settings.fps_init_delay_ms = key_file
        .integer("Game", "fps_unlocker_init_delay")
        .unwrap_or(DEFAULT_FPS_INIT_DELAY_MS as i32)
        .clamp(0, 600000) as u32;
    settings.graphics_api = GraphicsApi::from_str(
        key_file
            .string("Game", "graphics_api")
            .as_deref()
            .unwrap_or("dx12"),
    );
    settings.fsr = key_file.boolean("Game", "fsr").unwrap_or(true);
    settings.fsr_quality = FsrQuality::from_str(
        key_file
            .string("Game", "fsr_quality")
            .as_deref()
            .unwrap_or("balanced"),
    );
    // No stored choice yet: keep the system default chosen by `GameSettings::default()`.
    if let Ok(value) = key_file.string("Game", "sync") {
        settings.sync = SyncMode::from_str(value.as_str());
    }
    load_gamescope(&key_file, &mut settings.gamescope);
    settings.launch_on_host = key_file.boolean("Game", "launch_on_host").unwrap_or(false);
    settings.launch_args = key_file
        .string("Game", "launch_args")
        .map(|v| v.to_string())
        .unwrap_or_default();
    settings.wrapper = key_file
        .string("Game", "wrapper")
        .map(|v| v.to_string())
        .unwrap_or_default();
    settings.dedicated_gpu = key_file.boolean("Game", "dedicated_gpu").unwrap_or(false);
    settings.proton_log = key_file.boolean("Game", "proton_log").unwrap_or(false);
    settings.runner = key_file
        .string("Game", "runner")
        .ok()
        .map(|s| s.to_string())
        .filter(|s| !s.is_empty());
    if let Ok(path) = key_file.string("Game", "path") {
        if !path.is_empty() {
            settings.path = PathBuf::from(path);
        }
    }
    if let Ok(keys) = key_file.keys("Environment") {
        for key in keys {
            if let Ok(value) = key_file.string("Environment", key.as_str()) {
                settings
                    .environment
                    .insert(key.to_string(), value.to_string());
            }
        }
    }
    settings
}

fn load_gamescope(key_file: &glib::KeyFile, gs: &mut GamescopeSettings) {
    const GROUP: &str = "Gamescope";
    let number = |key: &str, max: i32| key_file.integer(GROUP, key).map(|v| v.clamp(0, max) as u32);
    gs.enabled = key_file.boolean(GROUP, "enabled").unwrap_or(false);
    gs.width = number("width", 16384).unwrap_or(0);
    gs.height = number("height", 16384).unwrap_or(0);
    gs.game_width = number("game_width", 16384).unwrap_or(0);
    gs.game_height = number("game_height", 16384).unwrap_or(0);
    gs.fps = number("fps", 1000).unwrap_or(0);
    gs.fps_unfocused = number("fps_unfocused", 1000).unwrap_or(0);
    gs.sharpness = number("sharpness", 20).unwrap_or(5);
    gs.mode = GamescopeMode::from_str(
        key_file
            .string(GROUP, "mode")
            .as_deref()
            .unwrap_or("fullscreen"),
    );
    gs.filter = GamescopeFilter::from_str(
        key_file
            .string(GROUP, "filter")
            .as_deref()
            .unwrap_or("default"),
    );
    gs.integer_scaling = key_file.boolean(GROUP, "integer_scaling").unwrap_or(false);
    gs.hdr = key_file.boolean(GROUP, "hdr").unwrap_or(false);
    gs.force_grab_cursor = key_file
        .boolean(GROUP, "force_grab_cursor")
        .unwrap_or(false);
    gs.extra_args = key_file
        .string(GROUP, "extra_args")
        .map(|v| v.to_string())
        .unwrap_or_default();
}

fn save_gamescope(key_file: &glib::KeyFile, gs: &GamescopeSettings) {
    const GROUP: &str = "Gamescope";
    key_file.set_boolean(GROUP, "enabled", gs.enabled);
    key_file.set_integer(GROUP, "width", gs.width as i32);
    key_file.set_integer(GROUP, "height", gs.height as i32);
    key_file.set_integer(GROUP, "game_width", gs.game_width as i32);
    key_file.set_integer(GROUP, "game_height", gs.game_height as i32);
    key_file.set_integer(GROUP, "fps", gs.fps as i32);
    key_file.set_integer(GROUP, "fps_unfocused", gs.fps_unfocused as i32);
    key_file.set_integer(GROUP, "sharpness", gs.sharpness as i32);
    key_file.set_string(GROUP, "mode", gs.mode.as_str());
    key_file.set_string(GROUP, "filter", gs.filter.as_str());
    key_file.set_boolean(GROUP, "integer_scaling", gs.integer_scaling);
    key_file.set_boolean(GROUP, "hdr", gs.hdr);
    key_file.set_boolean(GROUP, "force_grab_cursor", gs.force_grab_cursor);
    key_file.set_string(GROUP, "extra_args", gs.extra_args.as_str());
}

pub fn save_settings(game_id: &str, settings: &GameSettings) -> anyhow::Result<()> {
    let path = settings_path(game_id);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let key_file = glib::KeyFile::new();
    key_file.set_boolean("Game", "gamemode", settings.gamemode);
    key_file.set_string("Game", "dxvk_hud", settings.dxvk_hud.as_str());
    key_file.set_boolean("Game", "fps_unlocker", settings.fps_unlocker);
    key_file.set_integer("Game", "fps_unlocker_fps", settings.fps as i32);
    key_file.set_integer(
        "Game",
        "fps_unlocker_interval",
        settings.fps_interval_ms as i32,
    );
    key_file.set_integer(
        "Game",
        "fps_unlocker_init_delay",
        settings.fps_init_delay_ms as i32,
    );
    key_file.set_string("Game", "graphics_api", settings.graphics_api.as_str());
    key_file.set_boolean("Game", "fsr", settings.fsr);
    key_file.set_string("Game", "fsr_quality", settings.fsr_quality.as_str());
    // The system default (NTSync when supported, otherwise FSync) is not stored, so it keeps
    // following the machine; only an explicit different choice is written.
    if settings.sync != SyncMode::system_default() {
        key_file.set_string("Game", "sync", settings.sync.as_str());
    }
    save_gamescope(&key_file, &settings.gamescope);
    key_file.set_boolean("Game", "launch_on_host", settings.launch_on_host);
    key_file.set_string("Game", "launch_args", settings.launch_args.trim());
    key_file.set_string("Game", "wrapper", settings.wrapper.trim());
    key_file.set_boolean("Game", "dedicated_gpu", settings.dedicated_gpu);
    key_file.set_boolean("Game", "proton_log", settings.proton_log);
    key_file.set_string("Game", "runner", settings.runner.as_deref().unwrap_or(""));
    key_file.set_string("Game", "path", settings.path.to_string_lossy().as_ref());
    for (name, value) in &settings.environment {
        if !name.is_empty() {
            key_file.set_string("Environment", name, value);
        }
    }
    key_file.save_to_file(path)?;
    Ok(())
}

pub fn ensure_runner_setting(
    game_id: &str,
    folder_name: &str,
    default_runner: Option<String>,
) -> anyhow::Result<()> {
    let settings = load_settings(game_id, game_path(folder_name));
    let mut settings = settings;
    if settings.runner.as_deref().unwrap_or("").is_empty() {
        settings.runner = default_runner;
    }
    save_settings(game_id, &settings)
}
