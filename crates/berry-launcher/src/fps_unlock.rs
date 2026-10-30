use anyhow::{bail, Context, Result};
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;
use tracing::{debug, info, warn};

use super::GameLauncher;
use berry_games::catalog::GameCatalogEntry;
use berry_games::library::{GameSettings, DEFAULT_FPS_INIT_DELAY_MS, DEFAULT_FPS_REFRESH_DELAY_MS};

/// Id BerryUnlock expects for a game, `None` when the unlocker does not support it.
pub(super) fn unlocker_game_id(game: &GameCatalogEntry) -> Option<&'static str> {
    match game.id {
        "genshin-impact" => Some("hk4e_global"),
        "honkai-star-rail" => Some("hkrpg_global"),
        "honkai-impact-3rd" => Some("bh3_global"),
        "wuthering-waves" => Some("wuwa_global"),
        _ => None,
    }
}

/// Registry record that stores a game's FPS setting.
pub(super) struct RegistryFps {
    pub key: &'static str,
    pub prefix: &'static str,
    pub fields: &'static [&'static str],
}

/// berry-unlock's GitHub repository and the asset the launcher runs.
const UNLOCKER_REPO: &str = "A-Berry-Team/berry-unlock";
const UNLOCKER_ASSET: &str = "berry_unlock.exe";
/// Process pattern for `pkill -f`. `[.]` matches a literal dot, so only `berry_unlock.exe` matches
/// and the shell running pkill (whose own command line holds the bracketed text) never matches itself.
const UNLOCKER_PKILL: &str = "pkill -f 'berry_unlock[.]exe'";
const UNLOCKER_ID: &str = "berryunlock";
/// Install marker berry-installer writes for [`UNLOCKER_ID`] (`.berry-extra-<normalized id>`).
const UNLOCKER_MARKER: &str = ".berry-extra-berryunlock";

/// The FPS the unlocker is asked for: the stored value limited to what the game accepts.
pub(super) fn effective_fps(game: &GameCatalogEntry, settings: &GameSettings) -> u32 {
    game.fps_unlock()
        .map_or(settings.fps, |support| support.clamp(settings.fps))
}

/// Games whose graphics registry key must carry the target FPS before the unlocker starts.
pub(super) fn registry_fps(game: &GameCatalogEntry) -> Option<RegistryFps> {
    match game.id {
        "honkai-star-rail" => Some(RegistryFps {
            key: r"HKCU\Software\Cognosphere\Star Rail",
            prefix: "GraphicsSettings_Model",
            fields: &["FPS"],
        }),
        "honkai-impact-3rd" => Some(RegistryFps {
            key: r"HKCU\Software\miHoYo\Honkai Impact 3rd",
            prefix: "GENERAL_DATA_V2_PersonalGraphicsSettingV2_h3480068519",
            fields: &["TargetFrameRateForInLevel", "TargetFrameRateForOthers"],
        }),
        _ => None,
    }
}

impl GameLauncher {
    pub(super) fn fps_unlock_dir() -> PathBuf {
        berry_core::settings::extras_dir().join("fps_unlock")
    }

    pub fn prefetch_fps_unlocker() {
        std::thread::spawn(|| match Self::download_unlocker(&Self::fps_unlock_dir()) {
            Ok(()) => info!("[fps-unlock] cache ready"),
            Err(error) => {
                warn!("[fps-unlock] startup prefetch failed ({error:#}); Play will retry on demand")
            }
        });
    }

    /// The cached unlocker is never deleted: it stays in `dir` and is only replaced when a newer
    /// release is published. The one thing repaired here is an install marker that outlived its
    /// executable (the file was removed by an older build or by hand): without the executable the
    /// marker would claim "up to date" and nothing would ever download it again, so the marker is
    /// dropped and the next `ensure_github` fetches the release.
    fn repair_unlocker_marker(dir: &Path) {
        use berry_installer::extras::ExtrasManager;

        if Self::find_unlocker(dir).is_some()
            || ExtrasManager::installed_version(dir, UNLOCKER_ID).is_none()
        {
            return;
        }
        warn!("[fps-unlock] install marker without executable; it will be downloaded again");
        let _ = std::fs::remove_file(dir.join(UNLOCKER_MARKER));
    }

    // Fetches (or updates) BerryUnlock into `dir` from the latest release of its GitHub repository
    // through berry-installer's extras manager, shared by the startup prefetch and Play. Already
    // installed and up to date costs one small request, and an installed copy keeps working when
    // GitHub is unreachable. The error carries the real reason (no release, no matching asset,
    // HTTP status...).
    fn download_unlocker(dir: &Path) -> Result<()> {
        use berry_installer::extras::{ExtraOptions, ExtrasManager, GithubRef};

        std::fs::create_dir_all(dir).context("creating the FPS unlocker cache directory")?;
        let http = berry_installer::Http::new(berry_core::http::user_agent())
            .context("creating the HTTP client")?;
        // One small download: a single-threaded runtime is enough.
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .context("creating the download runtime")?;
        Self::repair_unlocker_marker(dir);
        // The asset may be published as a bare executable or as an archive; archives are
        // unpacked, executables are left as they are.
        runtime
            .block_on(ExtrasManager::new(http).ensure_github(
                GithubRef {
                    id: UNLOCKER_ID,
                    repo: UNLOCKER_REPO,
                    asset: UNLOCKER_ASSET,
                },
                dir,
                ExtraOptions {
                    extract: true,
                    strip_root: true,
                },
                &berry_installer::JobContext::new(),
            ))
            .context("download of the FPS unlocker failed")?;
        Ok(())
    }

    /// The cached unlocker: exactly `berry_unlock.exe` inside `dir`, no recursive or loose search.
    pub(super) fn find_unlocker(dir: &Path) -> Option<PathBuf> {
        let exe = dir.join(UNLOCKER_ASSET);
        exe.is_file().then_some(exe)
    }

    // Wuthering Waves keeps its exe under Client/Binaries/Win64, so it needs the install root.
    pub(super) fn unlocker_game_dir(
        game: &GameCatalogEntry,
        executable: &Path,
        install_root: &Path,
    ) -> PathBuf {
        if game.id == "wuthering-waves" {
            return install_root.to_path_buf();
        }
        executable.parent().unwrap_or(install_root).to_path_buf()
    }

    pub(super) fn run_proton_capture(
        proton: &Path,
        args: &[OsString],
        env: &[(String, String)],
        cwd: &Path,
        on_host: bool,
    ) -> Result<std::process::Output> {
        let mut argv = Vec::with_capacity(args.len() + 1);
        argv.push(proton.as_os_str().to_os_string());
        argv.extend(args.iter().cloned());
        let mut command = Self::make_command(&argv, env, cwd, on_host);
        command.stdout(Stdio::piped()).stderr(Stdio::piped());
        command.output().context("running Proton registry helper")
    }

    pub(super) fn parse_reg_binaries(output: &[u8]) -> Vec<(String, Vec<u8>)> {
        let text = String::from_utf8_lossy(output);
        let mut values = Vec::new();
        for line in text.lines() {
            let trimmed = line.trim();
            let mut parts = trimmed.split_whitespace();
            let Some(name) = parts.next() else {
                continue;
            };
            let Some(kind) = parts.next() else {
                continue;
            };
            if !kind.eq_ignore_ascii_case("REG_BINARY") {
                continue;
            }
            let mut bytes = Vec::new();
            for part in parts {
                if !part.bytes().all(|b| b.is_ascii_hexdigit()) || part.is_empty() {
                    continue;
                }
                if part.len() % 2 != 0 {
                    continue;
                }
                for pair in part.as_bytes().chunks_exact(2) {
                    let Ok(hex) = std::str::from_utf8(pair) else {
                        continue;
                    };
                    let Ok(byte) = u8::from_str_radix(hex, 16) else {
                        continue;
                    };
                    bytes.push(byte);
                }
            }
            if !bytes.is_empty() {
                values.push((name.to_string(), bytes));
            }
        }
        values
    }

    pub(super) fn query_reg_binary(
        proton: &Path,
        key: &str,
        prefix: &str,
        env: &[(String, String)],
        cwd: &Path,
        on_host: bool,
    ) -> Result<Option<(String, Vec<u8>)>> {
        let output = Self::run_proton_capture(
            proton,
            &["run".into(), "reg.exe".into(), "QUERY".into(), key.into()],
            env,
            cwd,
            on_host,
        )?;
        if !output.status.success() {
            return Ok(None);
        }
        Ok(Self::parse_reg_binaries(&output.stdout)
            .into_iter()
            .find(|(name, _)| {
                name.to_ascii_lowercase()
                    .starts_with(&prefix.to_ascii_lowercase())
            }))
    }

    pub(super) fn set_reg_binary(
        proton: &Path,
        key: &str,
        value_name: &str,
        data: &[u8],
        env: &[(String, String)],
        cwd: &Path,
        on_host: bool,
    ) -> Result<()> {
        let hex = data
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<Vec<_>>()
            .join(",");
        let output = Self::run_proton_capture(
            proton,
            &[
                "run".into(),
                "reg.exe".into(),
                "ADD".into(),
                key.into(),
                "/v".into(),
                value_name.into(),
                "/t".into(),
                "REG_BINARY".into(),
                "/d".into(),
                hex.into(),
                "/f".into(),
            ],
            env,
            cwd,
            on_host,
        )?;
        if !output.status.success() {
            bail!(
                "reg.exe failed for {key}\\{value_name}: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            );
        }
        Ok(())
    }

    pub(super) fn prepare_registry_fps(
        game: &GameCatalogEntry,
        proton: &Path,
        settings: &GameSettings,
        env: &[(String, String)],
        on_host: bool,
    ) -> Result<bool> {
        let Some(RegistryFps {
            key,
            prefix,
            fields,
        }) = registry_fps(game)
        else {
            return Ok(false);
        };

        let cwd = settings.path.as_path();
        let fps = effective_fps(game, settings);
        let Some((value_name, mut data)) =
            Self::query_reg_binary(proton, key, prefix, env, cwd, on_host)?
        else {
            warn!(
                "[fps-unlock] {} has no {} registry record yet; BerryUnlock may \
                 require launching the game once and changing a graphics setting",
                game.title, prefix
            );
            return Ok(false);
        };

        let text = std::str::from_utf8(&data)
            .context("Hoyo graphics registry value is not UTF-8")?
            .trim_start_matches('\u{feff}')
            .trim_end_matches('\0');
        let mut json: serde_json::Value =
            serde_json::from_str(text).context("Hoyo graphics registry value is not valid JSON")?;

        let mut changed = false;
        for field in fields {
            let Some(slot) = json.get_mut(*field) else {
                continue;
            };
            *slot = serde_json::Value::from(fps);
            changed = true;
        }
        if !changed {
            warn!(
                "[fps-unlock] {} registry record {} has none of the expected FPS fields",
                game.title, value_name
            );
            return Ok(false);
        }

        data = serde_json::to_vec(&json).context("serializing Hoyo graphics registry value")?;
        Self::set_reg_binary(proton, key, &value_name, &data, env, cwd, on_host)?;
        info!(
            "[fps-unlock] prepared {} registry FPS={} in {}",
            game.title, fps, value_name
        );
        Ok(true)
    }

    /// Where the launcher drops the unlocker's command for the gamescope launch script.
    pub(super) fn unlocker_script_path(game: &GameCatalogEntry) -> PathBuf {
        Self::fps_unlock_dir().join(format!("unlock-{}.sh", game.id))
    }

    /// The wrapper script gamescope runs instead of the game (see `render_launch_script`).
    pub(super) fn launch_script_path(game: &GameCatalogEntry) -> PathBuf {
        Self::fps_unlock_dir().join(format!("launch-{}.sh", game.id))
    }

    /// Finds (downloading it when needed) BerryUnlock and builds its command line.
    /// Returns the argv, the working directory and the unlocker's game id.
    fn unlocker_command(
        game: &GameCatalogEntry,
        proton: &Path,
        executable: &Path,
        settings: &GameSettings,
    ) -> Result<(Vec<OsString>, PathBuf, &'static str)> {
        let Some(unlocker_game_id) = unlocker_game_id(game) else {
            bail!("FPS unlocker does not support {}", game.title);
        };

        let dir = Self::fps_unlock_dir();
        let exe = match Self::find_unlocker(&dir) {
            Some(exe) => exe,
            None => {
                info!("[fps-unlock] downloading unlocker through berry-installer");
                Self::download_unlocker(&dir)?;
                Self::find_unlocker(&dir)
                    .context("the FPS unlocker was not found after downloading")?
            }
        };

        let game_dir = Self::unlocker_game_dir(game, executable, &settings.path);
        // Only Genshin Impact reads the delays; the other games get the defaults, which they ignore.
        let uses_delays = game.fps_unlock().is_some_and(|support| support.uses_delays);
        let (refresh_delay, init_delay) = if uses_delays {
            (settings.fps_interval_ms, settings.fps_init_delay_ms)
        } else {
            (DEFAULT_FPS_REFRESH_DELAY_MS, DEFAULT_FPS_INIT_DELAY_MS)
        };
        let argv: Vec<OsString> = vec![
            proton.into(),
            "run".into(),
            Self::windows_path(&exe).into(),
            // Global option: never open message boxes, nobody could close them under Proton.
            "--no-dialog".into(),
            "run".into(),
            unlocker_game_id.into(),
            effective_fps(game, settings).to_string().into(),
            refresh_delay.to_string().into(),
            init_delay.to_string().into(),
            Self::windows_path(&game_dir).into(),
        ];
        Ok((argv, dir, unlocker_game_id))
    }

    /// Normal launch (no gamescope): the unlocker is a separate process started by the launcher.
    pub(super) fn spawn_fps_unlocker(
        game: &GameCatalogEntry,
        proton: &Path,
        executable: &Path,
        settings: &GameSettings,
        env: &[(String, String)],
        on_host: bool,
    ) -> Result<std::process::Child> {
        let (argv, dir, unlocker_game_id) =
            Self::unlocker_command(game, proton, executable, settings)?;
        let log_path = dir.join("last-run.log");
        let mut command = Self::make_command(&argv, env, &dir, on_host);

        match std::fs::File::create(&log_path).and_then(|f| f.try_clone().map(|g| (f, g))) {
            Ok((out, err)) => {
                command.stdout(Stdio::from(out)).stderr(Stdio::from(err));
            }
            Err(_) => {
                command.stdout(Stdio::null()).stderr(Stdio::null());
            }
        }
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            command.process_group(0);
        }
        debug!("[fps-unlock] command: {:?}", command);
        let child = command.spawn().context("starting the FPS unlocker")?;
        info!(
            "[fps-unlock] started (pid {}, {} -> {} fps)",
            child.id(),
            unlocker_game_id,
            effective_fps(game, settings),
        );
        Ok(child)
    }

    /// Gamescope launch: hands the unlocker to the launch script that runs inside gamescope.
    /// The script is already waiting for this file, so it is written atomically (temp + rename).
    pub(super) fn release_fps_unlocker(
        game: &GameCatalogEntry,
        proton: &Path,
        executable: &Path,
        settings: &GameSettings,
    ) -> Result<PathBuf> {
        let (argv, dir, unlocker_game_id) =
            Self::unlocker_command(game, proton, executable, settings)?;
        let target = Self::unlocker_script_path(game);
        let command = argv
            .iter()
            .map(|argument| super::report::shell_quote(&argument.to_string_lossy()))
            .collect::<Vec<_>>()
            .join(" ");
        let body = format!(
            "#!/bin/sh\ncd {} || exit 1\nexec {command}\n",
            super::report::shell_quote(&dir.to_string_lossy()),
        );
        let temp = target.with_extension("sh.tmp");
        std::fs::write(&temp, body).context("writing the FPS unlocker script")?;
        std::fs::rename(&temp, &target).context("publishing the FPS unlocker script")?;
        info!(
            "[fps-unlock] released inside gamescope ({} -> {} fps)",
            unlocker_game_id,
            effective_fps(game, settings),
        );
        Ok(target)
    }

    /// Removes the leftovers of a gamescope launch (the launch script waits for these files).
    pub(super) fn clear_unlocker_scripts(game: &GameCatalogEntry) {
        let unlock = Self::unlocker_script_path(game);
        let _ = std::fs::remove_file(unlock.with_extension("sh.tmp"));
        let _ = std::fs::remove_file(unlock);
    }

    /// Makes sure no unlocker survives the game (it is not tracked as a child in gamescope mode).
    pub(super) fn kill_unlocker_by_name(on_host: bool) {
        let _ = Self::shell_status(UNLOCKER_PKILL, on_host);
    }

    pub(super) fn stop_unlocker(mut child: std::process::Child, on_host: bool) {
        let group = format!("-{}", child.id());
        let signal = |name: &str| {
            let _ = Command::new("kill")
                .args([name, "--", group.as_str()])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
        };
        let kill_by_name = || {
            let _ = Self::shell_status(UNLOCKER_PKILL, on_host);
        };
        if matches!(child.try_wait(), Ok(Some(_))) {
            kill_by_name();
            return;
        }
        signal("-TERM");
        for _ in 0..30 {
            if matches!(child.try_wait(), Ok(Some(_))) {
                kill_by_name();
                return;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        signal("-KILL");
        kill_by_name();
        let _ = child.wait();
    }
}
