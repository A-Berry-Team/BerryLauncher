//! Where gamescope comes from for a given launch target.
//!
//! * **Native package** (rpm / deb): the system's `gamescope`, found on `PATH`.
//! * **Flatpak, game launched on the host** (`flatpak-spawn --host`): the system's `gamescope`
//!   on the host, e.g. `sudo dnf install gamescope`. Nothing is needed inside the sandbox.
//! * **Flatpak, game launched in the sandbox** (the default): a host binary cannot run here
//!   (different glibc, Mesa and Xwayland), so gamescope has to come from the
//!   `org.freedesktop.Platform.VulkanLayer.gamescope` extension. It is only offered when that
//!   extension is actually installed.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

use super::GameLauncher;

/// Flatpak extension that provides gamescope inside the sandbox.
pub const EXTENSION_ID: &str = "org.freedesktop.Platform.VulkanLayer.gamescope";

/// Where Flatpak mounts the extension: under the app (declared in the manifest) or under the
/// runtime (when the runtime itself declares the extension point).
const EXTENSION_ROOTS: [&str; 2] = [
    "/app/lib/extensions/vulkan/gamescope",
    "/usr/lib/extensions/vulkan/gamescope",
];

/// The gamescope that a launch will use.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GamescopeSource {
    /// `gamescope` resolved through `PATH` of the launch target (native system or host).
    System,
    /// The binary shipped by the Flatpak extension, addressed by its full path.
    Extension(PathBuf),
}

impl GamescopeSource {
    /// What goes in front of gamescope's own options.
    pub fn program(&self) -> OsString {
        match self {
            Self::System => "gamescope".into(),
            Self::Extension(binary) => binary.clone().into(),
        }
    }

    /// Short description for the launch log.
    pub fn describe(&self, game_env: &BTreeMap<String, String>) -> String {
        match self {
            Self::System => "system gamescope".to_owned(),
            Self::Extension(binary) if Self::uses_x11_backend(game_env) => {
                format!("Flatpak extension ({}), X11/SDL backend", binary.display())
            }
            Self::Extension(binary) => format!("Flatpak extension ({})", binary.display()),
        }
    }

    /// The extension's gamescope runs on X11 (`--backend sdl` + `SDL_VIDEODRIVER=x11`) instead
    /// of its Wayland backend, unless the game's environment asks for another backend with
    /// `BERRY_GAMESCOPE_BACKEND=wayland` (or `auto`).
    ///
    /// Why: in windowed mode the Wayland backend draws the window frame through libdecor's GTK
    /// plugin. That GTK instance loads its icons with glycin, which starts a loader through
    /// `flatpak-spawn --sandbox`; inside the launcher's own sandbox that loader exits at once,
    /// GTK treats the failed fallback icon as fatal (`Gtk:ERROR ... image-missing.svg`) and
    /// gamescope dies with SIGABRT a second after starting, taking the game with it. The X11
    /// backend needs neither libdecor nor GTK (the manifest grants the X socket).
    fn uses_x11_backend(game_env: &BTreeMap<String, String>) -> bool {
        !matches!(
            game_env
                .get("BERRY_GAMESCOPE_BACKEND")
                .map(|value| value.trim().to_ascii_lowercase())
                .as_deref(),
            Some("wayland" | "auto")
        )
    }

    /// Options that go in front of the user's own gamescope options, so those can still
    /// override them.
    pub(super) fn leading_options(&self, game_env: &BTreeMap<String, String>) -> Vec<String> {
        match self {
            Self::Extension(_) if Self::uses_x11_backend(game_env) => {
                vec!["--backend".to_owned(), "sdl".to_owned()]
            }
            _ => Vec::new(),
        }
    }

    /// Environment gamescope needs besides its options. A variable the user set for the game
    /// is never overridden.
    pub(super) fn extra_env(&self, game_env: &BTreeMap<String, String>) -> Vec<(String, String)> {
        match self {
            Self::Extension(_)
                if Self::uses_x11_backend(game_env)
                    && !game_env.contains_key("SDL_VIDEODRIVER") =>
            {
                vec![("SDL_VIDEODRIVER".to_owned(), "x11".to_owned())]
            }
            _ => Vec::new(),
        }
    }
}

/// `<root>/bin/gamescope` of the first root that has an executable one.
pub(super) fn extension_binary_in(roots: &[PathBuf]) -> Option<PathBuf> {
    roots
        .iter()
        .map(|root| root.join("bin").join("gamescope"))
        .find(|binary| is_executable_file(binary))
}

fn is_executable_file(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path)
        .map(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

impl GameLauncher {
    /// The gamescope extension's binary, if the extension is installed (Flatpak only).
    pub fn gamescope_extension_binary() -> Option<PathBuf> {
        if !berry_core::settings::in_flatpak() {
            return None;
        }
        let roots: Vec<PathBuf> = EXTENSION_ROOTS.iter().map(PathBuf::from).collect();
        extension_binary_in(&roots)
    }

    /// Which gamescope would be used when the game is launched on the host (`on_host`) or in
    /// the launcher's own environment (native system, or the Flatpak sandbox).
    pub(super) fn gamescope_source(on_host: bool) -> Option<GamescopeSource> {
        if berry_core::settings::in_flatpak() && !on_host {
            return Self::gamescope_extension_binary().map(GamescopeSource::Extension);
        }
        Self::shell_status("command -v gamescope >/dev/null 2>&1", on_host)
            .ok()
            .filter(|status| status.success())
            .map(|_| GamescopeSource::System)
    }

    /// Is gamescope available where the launcher itself would start the game, ignoring any
    /// per-game choice? (Kept for callers that have no game at hand.)
    pub fn gamescope_available() -> bool {
        Self::gamescope_available_for(berry_core::settings::launch_on_host())
    }

    /// Same, for an explicit launch target. `on_host` is only meaningful in Flatpak.
    ///
    /// * native: the system's gamescope;
    /// * Flatpak + host: the host's gamescope;
    /// * Flatpak + sandbox: only with `org.freedesktop.Platform.VulkanLayer.gamescope`.
    pub fn gamescope_available_for(on_host: bool) -> bool {
        Self::gamescope_source(on_host).is_some()
    }

    /// The full `gamescope <options> --` prefix that wraps the Proton / runtime chain.
    pub(super) fn gamescope_prefix(source: &GamescopeSource, options: &[String]) -> Vec<OsString> {
        let mut prefix: Vec<OsString> = vec![source.program()];
        prefix.extend(options.iter().map(OsString::from));
        prefix.push("--".into());
        prefix
    }
}
