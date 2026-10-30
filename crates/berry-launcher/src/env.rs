use std::ffi::OsString;
use std::path::Path;
use std::process::{Command, Stdio};
use tracing::warn;

use super::GameLauncher;
use berry_games::catalog::GameCatalogEntry;
use berry_games::library::GameSettings;

impl GameLauncher {
    pub(super) fn compat_config(game: &GameCatalogEntry) -> &'static str {
        match game.id {
            "wuthering-waves" => "noopwr,noxalia",
            _ => "noxalia",
        }
    }

    pub(super) fn windows_path(path: &Path) -> String {
        format!("z:\\{}", path.to_string_lossy())
    }

    pub(super) fn is_reserved_launch_environment(key: &str) -> bool {
        matches!(
            key,
            "WINEPREFIX"
                | "WINEARCH"
                | "GAMEID"
                | "PROTONPATH"
                | "PROTON_VERB"
                | "STEAM_COMPAT_DATA_PATH"
                | "STEAM_COMPAT_INSTALL_PATH"
                | "STEAM_COMPAT_CLIENT_INSTALL_PATH"
                | "STEAM_COMPAT_APP_ID"
                | "STEAM_COMPAT_TOOL_PATHS"
                | "STEAM_COMPAT_LIBRARY_PATHS"
                | "STEAM_COMPAT_SHADER_PATH"
                | "SteamAppId"
                | "SteamGameId"
                | "SteamOverlayGameId"
                | "SteamOS"
                | "DXVK_HUD"
        )
    }

    pub(super) fn steam_game_id(game: &GameCatalogEntry) -> &'static str {
        match game.id {
            "wuthering-waves" => "3513350",
            _ => "0",
        }
    }

    pub(super) fn proton_env(
        game: &GameCatalogEntry,
        runner: &str,
        runner_root: &Path,
        settings: &GameSettings,
    ) -> Vec<(String, String)> {
        let compat = Self::compat_dir(game.id, runner);
        let install_path = settings.path.to_string_lossy().into_owned();
        let compat_s = compat.to_string_lossy().into_owned();
        let mut env = vec![
            (
                "WINEPREFIX".to_string(),
                Self::proton_prefix(game.id, runner)
                    .to_string_lossy()
                    .into_owned(),
            ),
            ("WINEARCH".into(), "win64".into()),
            ("SteamGameId".into(), Self::steam_game_id(game).into()),
            ("SteamOS".into(), "1".into()),
            ("STEAM_COMPAT_APP_ID".into(), "0".into()),
            ("STEAM_COMPAT_DATA_PATH".into(), compat_s.clone()),
            ("STEAM_COMPAT_INSTALL_PATH".into(), install_path.clone()),
            ("STEAM_COMPAT_CLIENT_INSTALL_PATH".into(), String::new()),
            ("SteamOverlayGameId".into(), "0".into()),
            ("PROTON_USE_XALIA".into(), "0".into()),
            ("PROTONFIXES_DISABLE".into(), "1".into()),
            (
                "STEAM_COMPAT_CONFIG".into(),
                Self::compat_config(game).into(),
            ),
            (
                "STEAM_COMPAT_TOOL_PATHS".into(),
                runner_root.to_string_lossy().into_owned(),
            ),
            (
                "STEAM_COMPAT_LIBRARY_PATHS".into(),
                format!("{install_path}:{compat_s}/pfx"),
            ),
            (
                "STEAM_COMPAT_SHADER_PATH".into(),
                format!("{compat_s}/shadercache"),
            ),
            (
                "WINEDLLOVERRIDES".into(),
                "lsteamclient=d;KRSDKExternal.exe=d".into(),
            ),
            ("__GL_SHADER_DISK_CACHE_SKIP_CLEANUP".into(), "1".into()),
        ];
        if let Some(locale) = berry_locale::wine_locale() {
            env.push(("LANG".into(), locale));
        }
        for (key, value) in Self::fsr_env(settings) {
            env.push((key.into(), value.into()));
        }
        for (key, value) in Self::sync_env(settings.sync) {
            env.push((key.into(), value.into()));
        }
        if settings.dedicated_gpu {
            for (key, value) in Self::dedicated_gpu_env() {
                env.push((key.into(), value.into()));
            }
        }
        if settings.proton_log {
            // The compat data directory is always mounted inside the Steam Linux Runtime.
            env.push(("PROTON_LOG".into(), "1".into()));
            env.push(("PROTON_LOG_DIR".into(), compat_s.clone()));
        }
        for (key, value) in &settings.environment {
            if Self::is_reserved_launch_environment(key) {
                warn!("[launch] ignoring reserved environment variable '{key}'");
                continue;
            }
            if key
                .chars()
                .next()
                .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
                && key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
            {
                env.retain(|(k, _)| k != key);
                env.push((key.clone(), value.clone()));
            } else {
                warn!("[launch] ignoring invalid environment variable name '{key}'");
            }
        }
        env
    }

    // Inside Flatpak the command runs in the launcher's own sandbox by default (see
    // `settings::launch_on_host_for`); only `on_host` goes through `flatpak-spawn --host`.
    pub(super) fn make_command(
        argv: &[OsString],
        env: &[(String, String)],
        cwd: &Path,
        on_host: bool,
    ) -> Command {
        if on_host {
            let mut command = Command::new("flatpak-spawn");
            command
                .arg("--host")
                .arg(format!("--directory={}", cwd.display()));
            for (key, value) in env {
                command.arg(format!("--env={key}={value}"));
            }
            command.args(argv);
            command
        } else {
            let mut command = Command::new(&argv[0]);
            command
                .args(&argv[1..])
                .current_dir(cwd)
                .envs(env.iter().map(|(k, v)| (k, v)));
            // The game inherits the launcher's sandbox environment. Library paths that point
            // into Flatpak extensions (gamescope, ...) are meant for the tools of those
            // extensions, not for Proton / Wine, which they can break (see below).
            if berry_core::settings::in_flatpak()
                && !env.iter().any(|(k, _)| k == "LD_LIBRARY_PATH")
            {
                let inherited = std::env::var("LD_LIBRARY_PATH").ok();
                if let Some(inherited) = inherited {
                    match Self::sandbox_library_path(&inherited) {
                        Some(path) => command.env("LD_LIBRARY_PATH", path),
                        None => command.env_remove("LD_LIBRARY_PATH"),
                    };
                }
            }
            command
        }
    }

    /// `LD_LIBRARY_PATH` for a game started inside the Flatpak sandbox: the inherited value
    /// without the entries that point into Flatpak extensions (`/app/lib/extensions/...`,
    /// `/usr/lib/extensions/...`). `None` when nothing is left, so the variable is unset.
    ///
    /// Why: a manifest that exports the gamescope extension's library directory
    /// globally makes *every* child process (Proton's Python, Wine, DXVK) load the extension's
    /// private copies of libraries ahead of the runtime's own. That is enough to crash
    /// runners that are sensitive to their libraries, DWProton for one. The extensions that
    /// need `add-ld-path` are already found through the sandbox's library cache.
    pub(super) fn sandbox_library_path(value: &str) -> Option<String> {
        let kept: Vec<&str> = value
            .split(':')
            .filter(|entry| !entry.is_empty())
            .filter(|entry| {
                !(entry.starts_with("/app/lib/extensions/")
                    || entry.starts_with("/usr/lib/extensions/"))
            })
            .collect();
        (!kept.is_empty()).then(|| kept.join(":"))
    }

    /// FSR through the Wine fullscreen hack, with the same presets AAGL exposes.
    pub(super) fn fsr_env(settings: &GameSettings) -> Vec<(&'static str, &'static str)> {
        if settings.fsr {
            vec![
                ("WINE_FULLSCREEN_FSR", "1"),
                ("WINE_FULLSCREEN_FSR_MODE", settings.fsr_quality.as_str()),
            ]
        } else {
            vec![("WINE_FULLSCREEN_FSR", "0")]
        }
    }

    /// Render offload for hybrid-graphics machines: NVIDIA PRIME render offload plus Mesa's
    /// `DRI_PRIME` for AMD / Intel. On a single-GPU desktop they change nothing.
    pub(super) fn dedicated_gpu_env() -> [(&'static str, &'static str); 4] {
        [
            ("DRI_PRIME", "1"),
            ("__NV_PRIME_RENDER_OFFLOAD", "1"),
            ("__VK_LAYER_NV_optimus", "NVIDIA_only"),
            ("__GLX_VENDOR_LIBRARY_NAME", "nvidia"),
        ]
    }

    /// Variables that make Proton / Wine pick the chosen synchronization primitive.
    /// Proton prefers ntsync, then fsync, then esync, so each mode switches the better ones off.
    pub(super) fn sync_env(
        mode: berry_games::library::SyncMode,
    ) -> Vec<(&'static str, &'static str)> {
        use berry_games::library::SyncMode;
        match mode {
            SyncMode::NtSync => vec![("PROTON_USE_NTSYNC", "1"), ("WINENTSYNC", "1")],
            SyncMode::FSync => vec![
                ("PROTON_USE_NTSYNC", "0"),
                ("PROTON_NO_NTSYNC", "1"),
                ("WINENTSYNC", "0"),
            ],
            SyncMode::ESync => vec![
                ("PROTON_USE_NTSYNC", "0"),
                ("PROTON_NO_NTSYNC", "1"),
                ("WINENTSYNC", "0"),
                ("PROTON_NO_FSYNC", "1"),
                ("WINEFSYNC", "0"),
            ],
            SyncMode::None => vec![
                ("PROTON_USE_NTSYNC", "0"),
                ("PROTON_NO_NTSYNC", "1"),
                ("WINENTSYNC", "0"),
                ("PROTON_NO_FSYNC", "1"),
                ("WINEFSYNC", "0"),
                ("PROTON_NO_ESYNC", "1"),
                ("WINEESYNC", "0"),
            ],
        }
    }

    /// Resource tier handed to Wuthering Waves through `-krqlv`. Since the "tiered quality"
    /// rollout the game refuses to start (`kuro: Use launcher to start game!`)
    /// unless the official launcher passes one, so we always do it on the user's behalf.
    /// Everything is downloaded, so the highest tier is the sensible default; advanced
    /// users can override it with a `BERRY_KRQLV=sd|hd|uhd` entry in the game's environment.
    const WUWA_DEFAULT_RESOURCE_TIER: &'static str = "uhd";

    pub(super) fn wuwa_resource_tier(settings: &GameSettings) -> &'static str {
        match settings
            .environment
            .get("BERRY_KRQLV")
            .map(|value| value.trim().to_ascii_lowercase())
            .as_deref()
        {
            Some("sd") => "sd",
            Some("hd") => "hd",
            Some("uhd") => "uhd",
            _ => Self::WUWA_DEFAULT_RESOURCE_TIER,
        }
    }

    /// Arguments appended after the game executable.
    pub(super) fn game_args(game: &GameCatalogEntry, settings: &GameSettings) -> Vec<OsString> {
        let mut args: Vec<OsString> = match game.id {
            "wuthering-waves" => vec![
                settings.graphics_api.launch_arg().into(),
                format!("-krqlv={}", Self::wuwa_resource_tier(settings)).into(),
            ],
            _ => Vec::new(),
        };
        // The user's own arguments come last so they can override the defaults above.
        args.extend(
            berry_games::library::split_command(&settings.launch_args)
                .into_iter()
                .map(OsString::from),
        );
        args
    }

    pub(super) fn game_argv(
        entry_point: Option<&Path>,
        proton: &Path,
        verb: &str,
        exe: &str,
        gamemode: bool,
        gamescope: Option<&[OsString]>,
        extra_args: &[OsString],
    ) -> Vec<OsString> {
        let mut argv = Self::launch_prefix(gamemode, gamescope);
        argv.extend(Self::proton_chain(
            entry_point,
            proton,
            verb,
            exe,
            gamemode,
            extra_args,
        ));
        argv
    }

    /// `gamemoderun gamescope <options> --`: whatever wraps the Proton / runtime chain.
    /// `gamescope` is the complete gamescope prefix (see `gamescope_prefix`), separator included.
    pub(super) fn launch_prefix(gamemode: bool, gamescope: Option<&[OsString]>) -> Vec<OsString> {
        let mut argv: Vec<OsString> = Vec::new();
        if gamemode {
            argv.push("gamemoderun".into());
        }
        // gamescope wraps the whole Proton / runtime chain: `gamescope <options> -- <command>`.
        if let Some(prefix) = gamescope {
            argv.extend(prefix.iter().cloned());
        }
        argv
    }

    /// The runtime entry point, Proton and the game itself.
    pub(super) fn proton_chain(
        entry_point: Option<&Path>,
        proton: &Path,
        verb: &str,
        exe: &str,
        gamemode: bool,
        extra_args: &[OsString],
    ) -> Vec<OsString> {
        let mut argv: Vec<OsString> = Vec::new();
        if let Some(entry_point) = entry_point {
            argv.push(entry_point.into());
            argv.push(format!("--verb={verb}").into());
            argv.push("--".into());
            // gamemoderun's LD_PRELOAD registers the host-side launch chain with gamemoded. Inside
            // the container libgamemode cannot be found, so Proton and Wine start without it
            // (it caused "gamemodeauto: dlopen failed" and Fontconfig errors).
            if gamemode {
                argv.extend(["env".into(), "-u".into(), "LD_PRELOAD".into()]);
            }
        }
        argv.push(proton.into());
        argv.push(verb.into());
        argv.push(exe.into());
        argv.extend(extra_args.iter().cloned());
        argv
    }

    /// Script that gamescope runs in place of the game when the FPS unlocker is on.
    ///
    /// Why: the unlocker is a second Proton process that joins the game's wineserver. Started from
    /// the launcher it inherits the *desktop's* DISPLAY while the game lives on gamescope's own X
    /// server, and two Wine processes of one wineserver talking to two X servers end in
    /// `X Error BadWindow (X_UnmapWindow)` and the game quits. Started from this script it gets
    /// exactly gamescope's environment (DISPLAY, WAYLAND_DISPLAY, ...), like the game does.
    ///
    /// The unlocker's own command is written later, by the launcher thread, to `unlocker_script`
    /// once the game's registry is ready; this script waits for that file and then runs it.
    pub(super) fn render_launch_script(
        game_command: &[OsString],
        unlocker_script: &Path,
        unlocker_log: &Path,
    ) -> String {
        use super::report::shell_quote;
        let quote = |path: &Path| shell_quote(&path.to_string_lossy());
        let command = game_command
            .iter()
            .map(|argument| shell_quote(&argument.to_string_lossy()))
            .collect::<Vec<_>>()
            .join(" ");
        format!(
            "#!/bin/sh\n\
             # Generated by Berry Launcher for every launch with gamescope + FPS unlocker.\n\
             UNLOCKER_SCRIPT={script}\n\
             UNLOCKER_LOG={log}\n\
             (\n\
             \x20 unset LD_PRELOAD DXVK_HUD\n\
             \x20 waited=0\n\
             \x20 while [ ! -e \"$UNLOCKER_SCRIPT\" ]; do\n\
             \x20   [ \"$waited\" -ge {timeout} ] && exit 0\n\
             \x20   sleep 1\n\
             \x20   waited=$((waited + 1))\n\
             \x20 done\n\
             \x20 sleep 2\n\
             \x20 exec sh \"$UNLOCKER_SCRIPT\" >\"$UNLOCKER_LOG\" 2>&1\n\
             ) &\n\
             unlocker=$!\n\
             {command}\n\
             status=$?\n\
             kill \"$unlocker\" 2>/dev/null\n\
             pkill -f 'berry_unlock[.]exe' 2>/dev/null\n\
             exit \"$status\"\n",
            script = quote(unlocker_script),
            log = quote(unlocker_log),
            timeout = Self::UNLOCKER_SCRIPT_TIMEOUT_SECS,
        )
    }

    /// How long the launch script waits for the launcher to release the unlocker.
    pub(super) const UNLOCKER_SCRIPT_TIMEOUT_SECS: u32 = 240;

    // Runs a shell snippet silently (on the host inside Flatpak) and returns its exit status.
    pub(super) fn shell_status(
        script: &str,
        on_host: bool,
    ) -> std::io::Result<std::process::ExitStatus> {
        let mut command = if on_host {
            let mut c = Command::new("flatpak-spawn");
            c.args(["--host", "sh", "-c", script]);
            c
        } else {
            let mut c = Command::new("sh");
            c.args(["-c", script]);
            c
        };
        command.stdout(Stdio::null()).stderr(Stdio::null()).status()
    }

    // `command -v` alone misses wrappers shipped without libgamemode, so gamemoderun is smoke-tested.
    pub(super) fn gamemode_available(on_host: bool) -> bool {
        Self::shell_status(
            "command -v gamemoderun >/dev/null 2>&1 && gamemoderun true >/dev/null 2>&1",
            on_host,
        )
        .map(|status| status.success())
        .unwrap_or(false)
    }
}
