use anyhow::{bail, Context, Result};
use std::collections::HashMap;
use std::ffi::OsString;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex, OnceLock,
};
use std::time::{Duration, Instant};
use tracing::{debug, error, info, warn};

use berry_games::catalog::GameCatalogEntry;
use berry_games::library::GameSettings;
use berry_runners::RunnerService;

mod env;
mod fps_unlock;
mod gamescope;
mod report;
mod sync;

/// Turns `value` into a single safe path component: anything outside `[A-Za-z0-9._-]` becomes
/// `_`, and a result made only of dots (`.`, `..`) is replaced so it can never climb out of the
/// directory it is joined to.
fn sanitize_component(value: &str) -> String {
    let cleaned: String = value
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.') {
                c
            } else {
                '_'
            }
        })
        .collect();
    if cleaned.chars().all(|c| c == '.') {
        "_".to_owned()
    } else {
        cleaned
    }
}

pub struct GameLauncher;

/// Games started by this launcher that are still running, mapped to whether they live inside
/// the launcher's own Flatpak sandbox (and so would be killed together with it).
static RUNNING_GAMES: OnceLock<Mutex<HashMap<String, bool>>> = OnceLock::new();

fn running_games() -> &'static Mutex<HashMap<String, bool>> {
    RUNNING_GAMES.get_or_init(|| Mutex::new(HashMap::new()))
}

impl GameLauncher {
    pub fn proton_prefix(game_id: &str, runner: &str) -> PathBuf {
        berry_core::settings::prefixes_dir()
            .join(game_id)
            .join(sanitize_component(runner))
            .join("pfx")
    }

    pub fn compat_dir(game_id: &str, runner: &str) -> PathBuf {
        berry_core::settings::prefixes_dir()
            .join(game_id)
            .join(sanitize_component(runner))
    }

    pub fn is_installed(game: &GameCatalogEntry, root: &Path) -> bool {
        game.is_installed(root)
    }

    pub fn log_path(game: &GameCatalogEntry) -> PathBuf {
        berry_core::settings::logs_dir().join(format!("{}.log", game.id))
    }

    // One file per game, recreated on every launch: a diagnostic header (see `report`), then the
    // game's stdout and stderr in launch order. Falls back to the console if it cannot be created.
    fn game_log_stdio(path: &Path, header: &str) -> (Stdio, Stdio) {
        let opened = std::fs::create_dir_all(berry_core::settings::logs_dir())
            .and_then(|_| std::fs::File::create(path))
            .and_then(|mut file| {
                file.write_all(header.as_bytes())?;
                let stderr = file.try_clone()?;
                Ok((file, stderr))
            });
        match opened {
            Ok((stdout, stderr)) => (stdout.into(), stderr.into()),
            Err(error) => {
                warn!(
                    "[launch] could not create game log {}: {error}",
                    path.display()
                );
                (Stdio::inherit(), Stdio::inherit())
            }
        }
    }

    pub fn is_running(game_id: &str) -> bool {
        running_games()
            .lock()
            .map(|games| games.contains_key(game_id))
            .unwrap_or(false)
    }

    /// Whether `game_id` is running inside the launcher's own Flatpak sandbox. Flatpak tears the
    /// whole sandbox down when its main process exits, so the launcher has to stay alive (without
    /// a window) until that game ends; games started on the host or natively do not need it.
    pub fn needs_launcher_alive(game_id: &str) -> bool {
        running_games()
            .lock()
            .map(|games| games.get(game_id) == Some(&true))
            .unwrap_or(false)
    }

    /// `needs_launcher_alive` for any game.
    pub fn any_game_needs_launcher_alive() -> bool {
        running_games()
            .lock()
            .map(|games| games.values().any(|bound| *bound))
            .unwrap_or(false)
    }

    pub fn launch<F>(
        service: &RunnerService,
        game: &GameCatalogEntry,
        settings: &GameSettings,
        on_exit: F,
    ) -> Result<u32>
    where
        F: FnOnce() + Send + 'static,
    {
        let runner = settings
            .runner
            .as_deref()
            .filter(|s| !s.is_empty())
            .ok_or_else(|| anyhow::anyhow!("No runner is selected for {}", game.title))?;
        Self::validate(settings)?;
        let proton = service
            .proton_path(runner)
            .ok_or_else(|| anyhow::anyhow!("Installed runner '{runner}' was not found"))?;
        let runner_root = proton
            .parent()
            .map(Path::to_path_buf)
            .ok_or_else(|| anyhow::anyhow!("invalid runner path {}", proton.display()))?;
        let executable = game.find_executable(&settings.path).ok_or_else(|| {
            anyhow::anyhow!(
                "{} is not installed (executable not found in {})",
                game.title,
                settings.path.display(),
            )
        })?;
        let cwd = executable.parent().unwrap_or(&settings.path).to_path_buf();
        let prefix = Self::proton_prefix(game.id, runner);
        let prefix_existed = prefix.is_dir();
        std::fs::create_dir_all(&prefix)
            .with_context(|| format!("creating prefix {}", prefix.display()))?;
        let on_host = berry_core::settings::launch_on_host_for(
            settings.launch_on_host,
            &settings.environment,
        );

        let runtime = berry_runners::steamrt::runtime_for_runner(&runner_root);
        let entry_point: Option<PathBuf> =
            if berry_core::settings::use_steam_runtime_container(on_host, &settings.environment) {
                Some(berry_runners::steamrt::ensure(runtime).with_context(|| {
                    format!("preparing the Steam Linux Runtime ({})", runtime.edition())
                })?)
            } else {
                None
            };

        let base_env = Self::proton_env(game, runner, &runner_root, settings);
        let mut env = base_env.clone();
        let dxvk_hud_supported = game.id != "wuthering-waves"
            || settings.graphics_api == berry_games::library::GraphicsApi::Dx11;
        if dxvk_hud_supported && settings.dxvk_hud != berry_games::library::DxvkHud::None {
            env.push(("DXVK_HUD".into(), settings.dxvk_hud.as_str().into()));
        }

        let use_gamemode = settings.gamemode && Self::gamemode_available(on_host);
        if settings.gamemode && !use_gamemode {
            warn!("[launch] GameMode is disabled for this launch (gamemoderun was not found)");
        }

        // With the FPS unlocker the game uses `run` so both Proton calls share one wineserver.
        let verb = if settings.fps_unlocker {
            "run"
        } else {
            "waitforexitandrun"
        };
        let windows_exe = Self::windows_path(&executable);
        let game_args = Self::game_args(game, settings);
        // Where gamescope comes from depends on where the game runs (see `gamescope`): the
        // system (native, or the host from Flatpak) or the Flatpak extension in the sandbox.
        let gamescope_source = if settings.gamescope.enabled {
            let source = Self::gamescope_source(on_host);
            match &source {
                Some(source) => info!("[launch] gamescope: {}", source.describe(&settings.environment)),
                None if berry_core::settings::in_flatpak() && !on_host => warn!(
                    "[launch] gamescope is enabled but the {} extension is not installed; starting without it",
                    gamescope::EXTENSION_ID,
                ),
                None => warn!(
                    "[launch] gamescope is enabled but was not found on the system; starting without it",
                ),
            }
            source
        } else {
            None
        };
        let gamescope_prefix: Option<Vec<OsString>> = gamescope_source.as_ref().map(|source| {
            // The source's own options come first so the user's can override them.
            let mut options = source.leading_options(&settings.environment);
            options.extend(settings.gamescope.args());
            Self::gamescope_prefix(source, &options)
        });
        if let Some(source) = &gamescope_source {
            for (key, value) in source.extra_env(&settings.environment) {
                env.push((key, value));
            }
        }
        // The user's command prefix (e.g. `mangohud`) goes in front of everything else.
        let wrapper: Vec<OsString> = berry_games::library::split_command(&settings.wrapper)
            .into_iter()
            .map(OsString::from)
            .collect();
        let with_wrapper = |rest: Vec<OsString>| -> Vec<OsString> {
            wrapper.iter().cloned().chain(rest).collect()
        };
        let argv = with_wrapper(Self::game_argv(
            entry_point.as_deref(),
            &proton,
            verb,
            &windows_exe,
            use_gamemode,
            gamescope_prefix.as_deref(),
            &game_args,
        ));

        // With gamescope + FPS unlocker the game is started by a small script that gamescope runs,
        // so the unlocker (a second Wine process) shares gamescope's display (see
        // `render_launch_script` for the failure this avoids).
        let unlocker_in_gamescope = settings.fps_unlocker
            && gamescope_prefix.is_some()
            && fps_unlock::unlocker_game_id(game).is_some();
        let launch_script = unlocker_in_gamescope.then(|| Self::launch_script_path(game));
        let run_argv = match &launch_script {
            Some(script) => {
                Self::clear_unlocker_scripts(game);
                let inner = Self::proton_chain(
                    entry_point.as_deref(),
                    &proton,
                    verb,
                    &windows_exe,
                    use_gamemode,
                    &game_args,
                );
                let text = Self::render_launch_script(
                    &inner,
                    &Self::unlocker_script_path(game),
                    &Self::fps_unlock_dir().join("last-run.log"),
                );
                std::fs::create_dir_all(Self::fps_unlock_dir())
                    .and_then(|_| std::fs::write(script, text))
                    .with_context(|| format!("writing the launch script {}", script.display()))?;
                let mut wrapped = Self::launch_prefix(use_gamemode, gamescope_prefix.as_deref());
                wrapped.push("sh".into());
                wrapped.push(script.into());
                with_wrapper(wrapped)
            }
            None => argv.clone(),
        };
        let mut cmd = Self::make_command(&run_argv, &env, &cwd, on_host);
        debug!("[launch] command: {:?}", cmd);

        let log_path = Self::log_path(game);
        let header = report::render_header(&report::LaunchReport {
            game,
            settings,
            runner,
            proton: &proton,
            runner_root: &runner_root,
            executable: &executable,
            cwd: &cwd,
            prefix: &prefix,
            prefix_existed,
            runtime,
            entry_point: entry_point.as_deref(),
            env: &env,
            argv: &argv,
            verb,
            gamemode_requested: settings.gamemode,
            gamemode_active: use_gamemode,
            gamescope_active: gamescope_prefix.is_some(),
            gamescope_source: gamescope_source
                .as_ref()
                .map(|source| source.describe(&settings.environment)),
            launch_script: launch_script.as_deref(),
            on_host,
            log_path: &log_path,
        });
        let (stdout, stderr) = Self::game_log_stdio(&log_path, &header);
        cmd.stdout(stdout).stderr(stderr);
        info!(
            "[launch] {} ({}) with {runner}; log: {}",
            game.title,
            game.id,
            log_path.display(),
        );
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            cmd.process_group(0);
        }

        let mut child = cmd.spawn().context("starting game process")?;
        let pid = child.id();
        report::append_event(
            &log_path,
            "launch",
            &format!("game process started (pid {pid}, {})", report::timestamp()),
        );

        // Record in the log which synchronization primitive the runner really announces.
        let game_alive_for_sync = Arc::new(AtomicBool::new(true));
        Self::watch_sync_usage(
            log_path.clone(),
            header.len() as u64,
            settings.sync,
            Arc::clone(&game_alive_for_sync),
        );

        let unlocker_slot: Arc<Mutex<Option<std::process::Child>>> = Arc::new(Mutex::new(None));
        let game_alive = Arc::new(AtomicBool::new(true));
        if settings.fps_unlocker && fps_unlock::unlocker_game_id(game).is_some() {
            let slot = Arc::clone(&unlocker_slot);
            let alive = Arc::clone(&game_alive);
            let game = game.clone();
            let proton = proton.clone();
            let executable = executable.clone();
            let settings = settings.clone();
            // Wine helpers that run next to a gamescope game must not talk to the desktop's X
            // server: the wineserver they share with the game lives on gamescope's.
            let mut base_env = base_env.clone();
            if gamescope_prefix.is_some() {
                base_env.push(("DISPLAY".into(), String::new()));
                base_env.push(("WAYLAND_DISPLAY".into(), String::new()));
            }
            let log_path = log_path.clone();
            std::thread::spawn(move || {
                let event = |message: &str| report::append_event(&log_path, "fps-unlock", message);
                // HSR and HI3 create their graphics registry key only after the game first initializes.
                if fps_unlock::registry_fps(&game).is_some() {
                    let deadline = Instant::now() + Duration::from_secs(45);
                    // Every attempt starts a Proton process, so back off while the key is missing.
                    let mut delay = Duration::from_millis(750);
                    loop {
                        if !alive.load(Ordering::Acquire) {
                            return;
                        }
                        if Instant::now() >= deadline {
                            warn!(
                                "[fps-unlock] graphics registry was not created within 45s; BerryUnlock was not started",
                            );
                            event("graphics registry was not created within 45 s; the unlocker was NOT started");
                            return;
                        }
                        match Self::prepare_registry_fps(
                            &game, &proton, &settings, &base_env, on_host,
                        ) {
                            Ok(true) => {
                                event(&format!(
                                    "registry prepared with {} fps",
                                    fps_unlock::effective_fps(&game, &settings)
                                ));
                                break;
                            }
                            Ok(false) => {}
                            Err(error) => warn!(
                                "[fps-unlock] waiting for {} registry: {error:#}",
                                game.title,
                            ),
                        }
                        std::thread::sleep(delay);
                        delay = (delay * 3 / 2).min(Duration::from_secs(3));
                    }
                }
                if !alive.load(Ordering::Acquire) {
                    return;
                }
                if unlocker_in_gamescope {
                    match Self::release_fps_unlocker(&game, &proton, &executable, &settings) {
                        Ok(_) => event(&format!(
                            "released to the gamescope launch script ({} fps, interval {} ms); it starts a few seconds later inside gamescope",
                            fps_unlock::effective_fps(&game, &settings),
                            settings.fps_interval_ms,
                        )),
                        Err(error) => {
                            warn!("[fps-unlock] could not prepare the FPS unlocker: {error:#}");
                            event(&format!("could not prepare the FPS unlocker: {error:#}"));
                        }
                    }
                    return;
                }
                match Self::spawn_fps_unlocker(
                    &game,
                    &proton,
                    &executable,
                    &settings,
                    &base_env,
                    on_host,
                ) {
                    Ok(child) => {
                        event(&format!(
                            "started (pid {}, {} fps, interval {} ms)",
                            child.id(),
                            fps_unlock::effective_fps(&game, &settings),
                            settings.fps_interval_ms,
                        ));
                        if !alive.load(Ordering::Acquire) {
                            Self::stop_unlocker(child, on_host);
                            return;
                        }
                        if let Ok(mut guard) = slot.lock() {
                            *guard = Some(child);
                        }
                    }
                    Err(error) => {
                        warn!("[fps-unlock] could not start the FPS unlocker: {error:#}");
                        event(&format!("could not start the FPS unlocker: {error:#}"));
                    }
                }
            });
        }

        if let Ok(mut games) = running_games().lock() {
            // Only a game that shares the launcher's sandbox dies with it (see `make_command`).
            games.insert(
                game.id.to_string(),
                berry_core::settings::in_flatpak() && !on_host,
            );
        }
        info!("[launch] game process started (pid {pid})");
        let started = Instant::now();
        let game_id = game.id.to_string();
        let game_for_exit = game.clone();

        std::thread::spawn(move || {
            let status = child.wait();
            let elapsed = started.elapsed();
            match &status {
                Ok(status) => {
                    info!("[launch] game process {pid} exited ({status}) after {elapsed:.0?}",)
                }
                Err(error) => error!("[launch] waiting for game process {pid} failed: {error}"),
            }
            game_alive.store(false, Ordering::Release);
            game_alive_for_sync.store(false, Ordering::Release);
            if let Ok(mut guard) = unlocker_slot.lock() {
                if let Some(unlocker) = guard.take() {
                    Self::stop_unlocker(unlocker, on_host);
                    report::append_event(&log_path, "fps-unlock", "stopped with the game");
                }
            }
            if unlocker_in_gamescope {
                Self::kill_unlocker_by_name(on_host);
                Self::clear_unlocker_scripts(&game_for_exit);
                report::append_event(&log_path, "fps-unlock", "stopped with the game");
            }
            report::append(&log_path, &report::render_footer(&status, elapsed));
            if let Ok(mut games) = running_games().lock() {
                games.remove(&game_id);
            }
            on_exit();
        });
        Ok(pid)
    }

    pub fn validate(settings: &GameSettings) -> Result<()> {
        if settings.path.as_os_str().is_empty() {
            bail!("game installation path is empty");
        }
        if !settings.path.is_dir() {
            bail!(
                "game installation path does not exist: {}",
                settings.path.display()
            );
        }
        Ok(())
    }
}
