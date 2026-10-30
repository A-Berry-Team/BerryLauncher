//! Text written to `logs/<game>.log`: a diagnostic header describing the machine, the runner and
//! every option of the launch, event lines while the game runs, and an exit footer. The game's own
//! stdout/stderr land between the header and the footer.
//!
//! The aim is that a log attached to a bug report answers the usual first questions (which GPU
//! driver, which runner, which sync primitive, which arguments) without a round trip.

use std::ffi::OsString;
use std::fmt::{Display, Write as _};
use std::fs::OpenOptions;
use std::io::{Read, Write as _};
use std::path::Path;
use std::process::{Command, ExitStatus, Stdio};
use std::sync::mpsc;
use std::sync::OnceLock;
use std::time::Duration;

use berry_core::system::{probe_ntsync_device, NtsyncDevice};
use berry_games::catalog::GameCatalogEntry;
use berry_games::library::GameSettings;
use berry_runners::steamrt::SteamRuntime;
use berry_runners::RunnerProvider;

use super::fps_unlock::{registry_fps, unlocker_game_id};
use super::GameLauncher;

const KEY_WIDTH: usize = 24;
const TOOL_TIMEOUT: Duration = Duration::from_secs(8);
/// Wine needs this many open files for esync to work (see Wine's esync documentation).
const ESYNC_MIN_OPEN_FILES: u64 = 524_288;
/// `futex_waitv`, which fsync is built on, arrived in Linux 5.16.
const FSYNC_MIN_KERNEL: (u32, u32) = (5, 16);

/// Switches that choose between ntsync, fsync and esync, in the order they are reported.
const SYNC_VARIABLES: &[&str] = &[
    "PROTON_USE_NTSYNC",
    "PROTON_NO_NTSYNC",
    "WINENTSYNC",
    "PROTON_NO_FSYNC",
    "WINEFSYNC",
    "PROTON_NO_ESYNC",
    "WINEESYNC",
];

/// Inherited variables worth showing: they change how Vulkan, OpenGL, Wine or Proton behave.
const RELEVANT_PREFIXES: &[&str] = &[
    "DISPLAY",
    "WAYLAND_DISPLAY",
    "XDG_SESSION_TYPE",
    "XDG_CURRENT_DESKTOP",
    "LANG",
    "LC_ALL",
    "LANGUAGE",
    "LD_PRELOAD",
    "LD_LIBRARY_PATH",
    "VK_",
    "MESA_",
    "RADV_",
    "AMD_",
    "DXVK_",
    "VKD3D_",
    "PROTON_",
    "WINE",
    "STEAM",
    "SDL_",
    "__GL",
    "__NV",
    "__VK",
    "LIBGL_",
    "DRI_PRIME",
    "MANGOHUD",
    "GAMEMODE",
    "ENABLE_",
    "vblank_mode",
];

/// Everything that went into one launch.
pub(super) struct LaunchReport<'a> {
    pub game: &'a GameCatalogEntry,
    pub settings: &'a GameSettings,
    pub runner: &'a str,
    pub proton: &'a Path,
    pub runner_root: &'a Path,
    pub executable: &'a Path,
    pub cwd: &'a Path,
    pub prefix: &'a Path,
    pub prefix_existed: bool,
    pub runtime: SteamRuntime,
    pub entry_point: Option<&'a Path>,
    /// Variables the launcher passes on (its own plus the user's).
    pub env: &'a [(String, String)],
    /// The command line before any `flatpak-spawn` wrapping.
    pub argv: &'a [OsString],
    pub verb: &'a str,
    pub gamemode_requested: bool,
    pub gamemode_active: bool,
    pub gamescope_active: bool,
    /// Where the gamescope in use comes from (system, host or Flatpak extension).
    pub gamescope_source: Option<String>,
    /// Wrapper script that runs the game and the FPS unlocker inside one gamescope session.
    pub launch_script: Option<&'a Path>,
    pub on_host: bool,
    pub log_path: &'a Path,
}

// ---------------------------------------------------------------------------------------------
// Public entry points
// ---------------------------------------------------------------------------------------------

pub(super) fn timestamp() -> String {
    format_now("%Y-%m-%d %H:%M:%S %z")
}

fn format_now(format: &str) -> String {
    glib::DateTime::now_local()
        .ok()
        .and_then(|now| now.format(format).ok())
        .map(|text| text.to_string())
        .unwrap_or_default()
}

pub(super) fn append(path: &Path, text: &str) {
    let _ = OpenOptions::new()
        .append(true)
        .open(path)
        .and_then(|mut file| file.write_all(text.as_bytes()));
}

/// One timestamped line, e.g. `[03:12:45] [fps-unlock] started (pid 4242, …)`.
pub(super) fn append_event(path: &Path, tag: &str, message: &str) {
    append(
        path,
        &format!("[{}] [{tag}] {message}\n", format_now("%H:%M:%S")),
    );
}

impl GameLauncher {
    /// Is `/dev/ntsync` present and accessible to the launcher?
    pub fn ntsync_available() -> bool {
        berry_core::system::ntsync_available()
    }
}

/// Warms the machine description (it may run `vulkaninfo`) so the first Play does not wait for it.
impl GameLauncher {
    pub fn prefetch_system_info() {
        std::thread::spawn(|| {
            system_section(berry_core::settings::launch_on_host());
        });
    }
}

pub(super) fn render_header(report: &LaunchReport) -> String {
    let mut doc = Doc::default();
    let _ = writeln!(
        doc.0,
        "Berry Launcher {} - {} ({})\nLaunch started: {}",
        berry_core::VERSION,
        report.game.title,
        report.game.id,
        timestamp(),
    );
    launcher_section(&mut doc, report);
    doc.raw(system_section(report.on_host));
    kernel_section(&mut doc);
    sync_section(&mut doc, report);
    runner_section(&mut doc, report);
    game_section(&mut doc, report);
    options_section(&mut doc, report);
    environment_section(&mut doc, report);
    command_section(&mut doc, report);
    doc.0
        .push_str("\n---------------- game output (stdout + stderr) ----------------\n");
    doc.0
}

pub(super) fn render_footer(status: &std::io::Result<ExitStatus>, elapsed: Duration) -> String {
    let mut doc = Doc::default();
    doc.0
        .push_str("\n---------------- game process ended ----------------\n");
    doc.section("Exit");
    doc.kv("Finished", timestamp());
    doc.kv("Run time", format_duration(elapsed));
    match status {
        Ok(status) => doc.kv("Result", describe_status(status)),
        Err(error) => doc.kv("Result", format!("waiting for the process failed: {error}")),
    }
    doc.0
}

// ---------------------------------------------------------------------------------------------
// Formatting helpers
// ---------------------------------------------------------------------------------------------

#[derive(Default)]
struct Doc(String);

impl Doc {
    fn section(&mut self, title: &str) {
        if !self.0.is_empty() {
            self.0.push('\n');
        }
        let _ = writeln!(self.0, "[{title}]");
    }

    fn kv(&mut self, key: &str, value: impl Display) {
        let _ = writeln!(
            self.0,
            "  {:<width$}{value}",
            format!("{key}:"),
            width = KEY_WIDTH
        );
    }

    fn item(&mut self, text: impl Display) {
        let _ = writeln!(self.0, "    {text}");
    }

    /// Appends an already rendered section, separated by a blank line.
    fn raw(&mut self, text: &str) {
        if !self.0.is_empty() {
            self.0.push('\n');
        }
        self.0.push_str(text);
    }
}

fn format_duration(duration: Duration) -> String {
    let seconds = duration.as_secs();
    let (hours, minutes, seconds) = (seconds / 3600, seconds % 3600 / 60, seconds % 60);
    if hours > 0 {
        format!("{hours}h {minutes:02}m {seconds:02}s")
    } else if minutes > 0 {
        format!("{minutes}m {seconds:02}s")
    } else {
        format!("{:.1}s", duration.as_secs_f64())
    }
}

fn describe_status(status: &ExitStatus) -> String {
    use std::os::unix::process::ExitStatusExt;
    match (status.code(), status.signal()) {
        (Some(0), _) => "exit code 0 (clean exit)".to_owned(),
        (Some(code), _) => format!("exit code {code}"),
        (None, Some(signal)) => {
            let core = if status.core_dumped() {
                ", core dumped"
            } else {
                ""
            };
            format!(
                "terminated by signal {signal} ({}){core}",
                signal_name(signal)
            )
        }
        (None, None) => status.to_string(),
    }
}

fn signal_name(signal: i32) -> &'static str {
    match signal {
        1 => "SIGHUP",
        2 => "SIGINT",
        3 => "SIGQUIT",
        4 => "SIGILL",
        6 => "SIGABRT",
        7 => "SIGBUS",
        8 => "SIGFPE",
        9 => "SIGKILL",
        11 => "SIGSEGV",
        13 => "SIGPIPE",
        15 => "SIGTERM",
        _ => "unknown signal",
    }
}

pub(super) fn shell_quote(argument: &str) -> String {
    let plain = !argument.is_empty()
        && argument
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_./:=,+@%".contains(&b));
    if plain {
        argument.to_owned()
    } else {
        format!("'{}'", argument.replace('\'', r"'\''"))
    }
}

fn read_trimmed(path: impl AsRef<Path>) -> Option<String> {
    std::fs::read_to_string(path)
        .ok()
        .map(|text| text.trim().to_owned())
        .filter(|text| !text.is_empty())
}

fn first_line(text: &str) -> &str {
    text.lines().next().unwrap_or_default().trim()
}

// ---------------------------------------------------------------------------------------------
// Sections
// ---------------------------------------------------------------------------------------------

fn launcher_section(doc: &mut Doc, report: &LaunchReport) {
    doc.section("Launcher");
    doc.kv("Version", berry_core::VERSION);
    doc.kv(
        "Packaging",
        if report.on_host {
            "Flatpak (the game is started on the host through flatpak-spawn; \"Launch on the host\" is on)"
        } else if berry_core::settings::in_flatpak() {
            "Flatpak (the game runs inside the launcher's sandbox)"
        } else {
            "native"
        },
    );
    if berry_core::settings::in_flatpak() && !report.on_host {
        doc.kv(
            "X11 for Wine",
            x11_state(std::env::var("DISPLAY").ok().as_deref()),
        );
    }
    doc.kv("Data directory", berry_core::settings::data_dir().display());
    doc.kv("Game log", report.log_path.display());
    doc.kv(
        "RUST_LOG",
        std::env::var("RUST_LOG").unwrap_or_else(|_| "not set".into()),
    );
}

/// Socket of an X display such as `:0` or `:1.0`; `None` for remote displays.
fn x11_socket_path(display: &str) -> Option<std::path::PathBuf> {
    let number = display.strip_prefix(':')?.split('.').next()?;
    (!number.is_empty() && number.chars().all(|c| c.is_ascii_digit()))
        .then(|| std::path::PathBuf::from(format!("/tmp/.X11-unix/X{number}")))
}

/// Wine and Proton use the X11 driver by default (Valve's Proton, and DWProton, ship no Wayland
/// driver), so inside the sandbox they need the X socket, i.e. `--socket=x11` in the manifest.
fn x11_state(display: Option<&str>) -> String {
    match display {
        None | Some("") => {
            "NOT available: DISPLAY is not set (the manifest needs --socket=x11; runners without a Wayland driver, such as DWProton, cannot open a window)".to_owned()
        }
        Some(display) => match x11_socket_path(display) {
            Some(socket) if socket.exists() => format!("DISPLAY={display}, socket present"),
            Some(socket) => format!("DISPLAY={display} but {} is missing", socket.display()),
            None => format!("DISPLAY={display}"),
        },
    }
}

/// Machine description that does not change between launches, rendered once.
fn system_section(on_host: bool) -> &'static str {
    static SECTION: OnceLock<String> = OnceLock::new();
    SECTION.get_or_init(|| {
        let mut doc = Doc::default();
        doc.section("System");
        doc.kv("OS", os_name(on_host));
        doc.kv(
            "Kernel",
            read_trimmed("/proc/sys/kernel/osrelease").unwrap_or_else(unknown),
        );
        doc.kv("Architecture", std::env::consts::ARCH);
        doc.kv("CPU", cpu_summary());
        let var = |name: &str| std::env::var(name).unwrap_or_else(|_| "unset".into());
        doc.kv(
            "Session",
            format!(
                "{} / {}",
                var("XDG_CURRENT_DESKTOP"),
                var("XDG_SESSION_TYPE")
            ),
        );
        doc.kv(
            "Display",
            format!(
                "WAYLAND_DISPLAY={} DISPLAY={}",
                var("WAYLAND_DISPLAY"),
                var("DISPLAY")
            ),
        );
        let gpus = gpus_from_sysfs();
        if gpus.is_empty() {
            doc.kv("GPU", "none found under /sys/class/drm");
        }
        for gpu in gpus {
            doc.kv("GPU", gpu);
        }
        if let Some(nvidia) = nvidia_driver() {
            doc.kv("NVIDIA driver", nvidia);
        }
        vulkan_lines(&mut doc, on_host);
        doc.0
    })
}

fn unknown() -> String {
    "unknown".to_owned()
}

fn os_name(on_host: bool) -> String {
    // Inside Flatpak /etc/os-release describes the runtime, the host's copy is mounted here.
    let host = Path::new("/run/host/os-release");
    let (path, is_runtime) = if on_host && host.is_file() {
        (host, false)
    } else {
        (Path::new("/etc/os-release"), on_host)
    };
    let name = std::fs::read_to_string(path)
        .ok()
        .and_then(|text| parse_os_release(&text));
    match (name, is_runtime) {
        (Some(name), true) => format!("{name} (Flatpak runtime; host OS not visible)"),
        (Some(name), false) => name,
        (None, _) => unknown(),
    }
}

fn parse_os_release(text: &str) -> Option<String> {
    let value = |key: &str| {
        text.lines()
            .find_map(|line| line.strip_prefix(key)?.strip_prefix('='))
            .map(|value| value.trim().trim_matches('"').to_owned())
    };
    value("PRETTY_NAME").or_else(|| value("NAME"))
}

fn cpu_summary() -> String {
    let text = std::fs::read_to_string("/proc/cpuinfo").unwrap_or_default();
    let (model, threads) = parse_cpuinfo(&text);
    let threads = if threads > 0 {
        threads
    } else {
        std::thread::available_parallelism().map_or(0, |n| n.get())
    };
    format!("{} ({threads} threads)", model.unwrap_or_else(unknown))
}

fn parse_cpuinfo(text: &str) -> (Option<String>, usize) {
    let mut model = None;
    let mut threads = 0;
    for line in text.lines() {
        if line.starts_with("processor") {
            threads += 1;
        } else if model.is_none()
            && ["model name", "Hardware", "cpu model"]
                .iter()
                .any(|key| line.starts_with(key))
        {
            model = line
                .split_once(':')
                .map(|(_, value)| value.trim().to_owned());
        }
    }
    (model, threads)
}

fn gpus_from_sysfs() -> Vec<String> {
    let mut cards: Vec<_> = std::fs::read_dir("/sys/class/drm")
        .into_iter()
        .flatten()
        .flatten()
        .filter(|entry| {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            name.strip_prefix("card")
                .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
        })
        .collect();
    cards.sort_by_key(|entry| entry.file_name());
    cards
        .iter()
        .map(|card| {
            let device = card.path().join("device");
            let id = |file: &str| {
                read_trimmed(device.join(file))
                    .map(|v| v.trim_start_matches("0x").to_owned())
                    .unwrap_or_else(|| "????".into())
            };
            let (vendor, product) = (id("vendor"), id("device"));
            let driver = std::fs::read_link(device.join("driver"))
                .ok()
                .and_then(|link| link.file_name().map(|n| n.to_string_lossy().into_owned()))
                .unwrap_or_else(|| "no driver bound".into());
            format!(
                "{} {vendor}:{product}, kernel driver {driver} ({})",
                vendor_name(&vendor),
                card.file_name().to_string_lossy(),
            )
        })
        .collect()
}

fn vendor_name(id: &str) -> &'static str {
    match id {
        "10de" => "NVIDIA",
        "1002" => "AMD",
        "8086" => "Intel",
        _ => "unknown vendor",
    }
}

fn nvidia_driver() -> Option<String> {
    let text = std::fs::read_to_string("/proc/driver/nvidia/version").ok()?;
    Some(first_line(&text).to_owned()).filter(|line| !line.is_empty())
}

fn vulkan_lines(doc: &mut Doc, on_host: bool) {
    let Some(output) = run_tool("vulkaninfo", &["--summary"], on_host) else {
        doc.kv(
            "Vulkan",
            "vulkaninfo is not available (or timed out), no driver details",
        );
        return;
    };
    let (instance, devices) = parse_vulkaninfo(&output);
    doc.kv("Vulkan instance", instance.unwrap_or_else(unknown));
    if devices.is_empty() {
        doc.kv("Vulkan devices", "none reported");
    }
    for device in devices {
        doc.kv("Vulkan device", device);
    }
}

fn parse_vulkaninfo(text: &str) -> (Option<String>, Vec<String>) {
    let mut instance = None;
    let mut devices: Vec<Vec<(String, String)>> = Vec::new();
    for line in text.lines().map(str::trim) {
        if let Some(version) = line.strip_prefix("Vulkan Instance Version:") {
            instance = Some(version.trim().to_owned());
        } else if line.starts_with("GPU") && line.ends_with(':') {
            devices.push(Vec::new());
        } else if let (Some(device), Some((key, value))) =
            (devices.last_mut(), line.split_once('='))
        {
            device.push((key.trim().to_owned(), value.trim().to_owned()));
        }
    }
    let lines = devices
        .iter()
        .enumerate()
        .map(|(index, fields)| {
            let get = |key: &str| {
                fields
                    .iter()
                    .find(|(k, _)| k == key)
                    .map_or("?", |(_, value)| value.as_str())
            };
            format!(
                "GPU{index}: {} | driver {} {} | Vulkan {} | {}",
                get("deviceName"),
                get("driverName"),
                get("driverInfo"),
                get("apiVersion"),
                get("deviceType")
                    .trim_start_matches("PHYSICAL_DEVICE_TYPE_")
                    .to_lowercase(),
            )
        })
        .collect();
    (instance, lines)
}

/// Runs a small host tool and returns its stdout; `None` if it is missing, fails or hangs.
fn run_tool(program: &str, args: &[&str], on_host: bool) -> Option<String> {
    let mut command = if on_host {
        let mut command = Command::new("flatpak-spawn");
        command.args(["--host", program]);
        command
    } else {
        Command::new(program)
    };
    command
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let mut child = command.spawn().ok()?;
    let mut stdout = child.stdout.take()?;
    // Read on a helper thread so a tool that hangs can still be abandoned after the timeout.
    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        let mut text = String::new();
        let _ = stdout.read_to_string(&mut text);
        let _ = sender.send(text);
    });
    match receiver.recv_timeout(TOOL_TIMEOUT) {
        Ok(text) => child.wait().ok()?.success().then_some(text),
        Err(_) => {
            let _ = child.kill();
            let _ = child.wait();
            None
        }
    }
}

/// Memory, limits and namespace policy: what the kernel allows right now.
fn kernel_section(doc: &mut Doc) {
    doc.section("Memory and limits");
    let meminfo = std::fs::read_to_string("/proc/meminfo").unwrap_or_default();
    let gib = |key: &str| {
        meminfo_kib(&meminfo, key).map_or_else(unknown, |kib| {
            format!("{:.1} GiB", kib as f64 / 1_048_576.0)
        })
    };
    doc.kv(
        "RAM",
        format!(
            "{} total, {} available",
            gib("MemTotal"),
            gib("MemAvailable")
        ),
    );
    doc.kv(
        "Swap",
        format!("{} total, {} free", gib("SwapTotal"), gib("SwapFree")),
    );
    doc.kv(
        "vm.max_map_count",
        read_trimmed("/proc/sys/vm/max_map_count").unwrap_or_else(unknown),
    );
    let limits = std::fs::read_to_string("/proc/self/limits").unwrap_or_default();
    match parse_open_files(&limits) {
        Some((soft, hard)) => doc.kv(
            "Open files limit",
            format!("soft {}, hard {}", limit_text(soft), limit_text(hard)),
        ),
        None => doc.kv("Open files limit", "unknown"),
    }

    let namespaces: Vec<String> = [
        (
            "unprivileged_userns_clone",
            "/proc/sys/kernel/unprivileged_userns_clone",
        ),
        ("max_user_namespaces", "/proc/sys/user/max_user_namespaces"),
        (
            "apparmor_restrict_unprivileged_userns",
            "/proc/sys/kernel/apparmor_restrict_unprivileged_userns",
        ),
    ]
    .iter()
    .filter_map(|(name, path)| read_trimmed(path).map(|value| format!("{name}={value}")))
    .collect();
    doc.kv(
        "User namespaces",
        if namespaces.is_empty() {
            "no restriction knobs found".to_owned()
        } else {
            namespaces.join(", ")
        },
    );
}

fn meminfo_kib(text: &str, key: &str) -> Option<u64> {
    text.lines().find_map(|line| {
        line.strip_prefix(key)?
            .strip_prefix(':')?
            .trim()
            .strip_suffix("kB")?
            .trim()
            .parse()
            .ok()
    })
}

fn parse_open_files(limits: &str) -> Option<(u64, u64)> {
    const LABEL: &str = "Max open files";
    let line = limits.lines().find(|line| line.starts_with(LABEL))?;
    let mut numbers = line[LABEL.len()..].split_whitespace().map(|value| {
        if value == "unlimited" {
            Some(u64::MAX)
        } else {
            value.parse().ok()
        }
    });
    Some((numbers.next()??, numbers.next()??))
}

fn limit_text(limit: u64) -> String {
    if limit == u64::MAX {
        "unlimited".to_owned()
    } else {
        limit.to_string()
    }
}

// --- synchronization ---------------------------------------------------------------------------

fn parse_kernel_version(release: &str) -> Option<(u32, u32)> {
    let mut numbers = release
        .split(|c: char| !c.is_ascii_digit())
        .filter(|part| !part.is_empty());
    Some((numbers.next()?.parse().ok()?, numbers.next()?.parse().ok()?))
}

/// Does `file` contain `needle`? `None` when it cannot be read. Streams the file in chunks.
fn file_contains(path: &Path, needle: &[u8]) -> Option<bool> {
    let mut file = std::fs::File::open(path).ok()?;
    let mut chunk = vec![0u8; 256 * 1024];
    let mut window: Vec<u8> = Vec::new();
    loop {
        let read = file.read(&mut chunk).ok()?;
        if read == 0 {
            return Some(false);
        }
        window.extend_from_slice(&chunk[..read]);
        if window
            .windows(needle.len())
            .any(|candidate| candidate == needle)
        {
            return Some(true);
        }
        // Keep a tail so a match split across two chunks is still found.
        let keep = (needle.len() - 1).min(window.len());
        window.drain(..window.len() - keep);
    }
}

/// Does the runner's Wine build talk to `/dev/ntsync`?
fn runner_has_ntsync(runner_root: &Path) -> Option<bool> {
    ["files", "dist"]
        .iter()
        .flat_map(|dir| ["lib", "lib64"].map(|lib| runner_root.join(dir).join(lib)))
        .map(|lib| lib.join("wine/x86_64-unix/ntdll.so"))
        .find(|path| path.is_file())
        .and_then(|ntdll| file_contains(&ntdll, b"/dev/ntsync"))
}

/// Which sync switches the runner's `proton` script knows about.
fn proton_script_switches(proton: &Path) -> Vec<&'static str> {
    let script = std::fs::read(proton).unwrap_or_default();
    SYNC_VARIABLES
        .iter()
        .copied()
        .filter(|name| {
            script
                .windows(name.len())
                .any(|window| window == name.as_bytes())
        })
        .collect()
}

#[derive(Debug)]
struct SyncInputs {
    ntsync_device_usable: bool,
    /// `None` when the runner could not be inspected.
    runner_ntsync: Option<bool>,
    /// The runner only enables ntsync when asked (`PROTON_USE_NTSYNC=1`) and the user did not.
    ntsync_opt_in_missing: bool,
    fsync_kernel: bool,
    esync_limit_ok: bool,
    ntsync_disabled: bool,
    fsync_disabled: bool,
    esync_disabled: bool,
}

/// Best-effort guess of the primitive Proton will pick: ntsync, then fsync, then esync.
fn expected_sync(inputs: &SyncInputs) -> &'static str {
    if !inputs.ntsync_disabled
        && inputs.ntsync_device_usable
        && inputs.runner_ntsync != Some(false)
        && !inputs.ntsync_opt_in_missing
    {
        "ntsync"
    } else if !inputs.fsync_disabled && inputs.fsync_kernel {
        "fsync"
    } else if !inputs.esync_disabled && inputs.esync_limit_ok {
        "esync"
    } else {
        "none (plain wineserver synchronization)"
    }
}

fn lookup_env(report: &LaunchReport, name: &str) -> Option<String> {
    let explicit = report
        .env
        .iter()
        .rev()
        .find(|(key, _)| key == name)
        .map(|(_, value)| value.clone());
    // Outside Flatpak the game inherits the launcher's environment.
    explicit.or_else(|| {
        if report.on_host {
            None
        } else {
            std::env::var(name).ok()
        }
    })
}

fn sync_section(doc: &mut Doc, report: &LaunchReport) {
    let is = |name: &str, wanted: &[&str]| {
        lookup_env(report, name).is_some_and(|value| wanted.contains(&value.as_str()))
    };
    let switches = proton_script_switches(report.proton);
    let device = probe_ntsync_device();
    let kernel = read_trimmed("/proc/sys/kernel/osrelease").and_then(|r| parse_kernel_version(&r));
    let open_files = std::fs::read_to_string("/proc/self/limits")
        .ok()
        .and_then(|text| parse_open_files(&text));
    let runner_ntsync = runner_has_ntsync(report.runner_root);

    let inputs = SyncInputs {
        ntsync_device_usable: matches!(device, NtsyncDevice::Usable),
        runner_ntsync,
        ntsync_opt_in_missing: switches.contains(&"PROTON_USE_NTSYNC")
            && !switches.contains(&"PROTON_NO_NTSYNC")
            && !is("PROTON_USE_NTSYNC", &["1"]),
        fsync_kernel: kernel.is_some_and(|version| version >= FSYNC_MIN_KERNEL),
        esync_limit_ok: open_files.is_some_and(|(_, hard)| hard >= ESYNC_MIN_OPEN_FILES),
        ntsync_disabled: is("PROTON_NO_NTSYNC", &["1"])
            || is("WINENTSYNC", &["0"])
            || is("PROTON_USE_NTSYNC", &["0"]),
        fsync_disabled: is("PROTON_NO_FSYNC", &["1"]) || is("WINEFSYNC", &["0"]),
        esync_disabled: is("PROTON_NO_ESYNC", &["1"]) || is("WINEESYNC", &["0"]),
    };

    doc.section("Synchronization (ntsync / fsync / esync)");
    doc.kv(
        "Selected in Berry",
        format!(
            "{} (the primitive the game really uses is logged as a [sync] event after startup)",
            report.settings.sync.display()
        ),
    );
    doc.kv(
        "ntsync device",
        match &device {
            NtsyncDevice::Usable => "/dev/ntsync present and accessible".to_owned(),
            NtsyncDevice::Missing => {
                "/dev/ntsync missing (needs Linux 6.14+ with the ntsync module)".to_owned()
            }
            NtsyncDevice::Denied(error) => {
                format!("/dev/ntsync present but not accessible ({error})")
            }
        },
    );
    doc.kv(
        "ntsync in runner",
        match runner_ntsync {
            Some(true) => "yes (Wine's ntdll.so references /dev/ntsync)",
            Some(false) => "no (Wine's ntdll.so does not reference /dev/ntsync)",
            None => "unknown (Wine's ntdll.so was not found)",
        },
    );
    doc.kv(
        "fsync (futex_waitv)",
        match kernel {
            Some((major, minor)) => format!(
                "{} by kernel {major}.{minor} (needs {}.{}+)",
                if inputs.fsync_kernel {
                    "supported"
                } else {
                    "NOT supported"
                },
                FSYNC_MIN_KERNEL.0,
                FSYNC_MIN_KERNEL.1,
            ),
            None => "unknown kernel version".to_owned(),
        },
    );
    doc.kv(
        "esync (open files)",
        match open_files {
            Some((_, hard)) => format!(
                "hard limit {} ({}, needs {ESYNC_MIN_OPEN_FILES}+)",
                limit_text(hard),
                if inputs.esync_limit_ok {
                    "ok"
                } else {
                    "too low"
                },
            ),
            None => "unknown".to_owned(),
        },
    );
    doc.kv(
        "Switches in runner",
        if switches.is_empty() {
            "none found in the proton script".to_owned()
        } else {
            switches.join(", ")
        },
    );
    let overrides: Vec<String> = SYNC_VARIABLES
        .iter()
        .filter_map(|name| lookup_env(report, name).map(|value| format!("{name}={value}")))
        .collect();
    doc.kv(
        "Overrides in effect",
        if overrides.is_empty() {
            "none".to_owned()
        } else {
            overrides.join(" ")
        },
    );
    doc.kv(
        "Expected to be used",
        format!(
            "{} (estimate: the runner decides at startup, see its fsync/ntsync lines below)",
            expected_sync(&inputs),
        ),
    );
}

// --- runner, game, options ---------------------------------------------------------------------

fn runner_section(doc: &mut Doc, report: &LaunchReport) {
    let provider = report
        .runner_root
        .parent()
        .and_then(|dir| dir.file_name())
        .and_then(|name| {
            RunnerProvider::ALL
                .iter()
                .find(|p| p.directory_name() == name)
        })
        .map_or("unknown", |provider| provider.display_name());
    doc.section("Runner");
    doc.kv("Name", report.runner);
    doc.kv("Provider", provider);
    doc.kv(
        "Version file",
        read_trimmed(report.runner_root.join("version"))
            .map(|text| first_line(&text).to_owned())
            .unwrap_or_else(|| "none".into()),
    );
    doc.kv("Path", report.runner_root.display());
    doc.kv("Proton script", report.proton.display());
    doc.kv(
        "Steam Linux Runtime",
        format!(
            "{} ({})",
            report.runtime.edition(),
            if report.runtime.is_ready() {
                "installed"
            } else {
                "NOT marked ready"
            },
        ),
    );
    match report.entry_point {
        Some(entry_point) => doc.kv("Runtime entry point", entry_point.display()),
        None => doc.kv(
            "Runtime entry point",
            "none (Proton runs directly in the Flatpak sandbox, without pressure-vessel)",
        ),
    }
}

fn game_section(doc: &mut Doc, report: &LaunchReport) {
    let compat = report.prefix.parent().unwrap_or(report.prefix);
    doc.section("Game");
    doc.kv(
        "Title",
        format!("{} ({})", report.game.title, report.game.id),
    );
    doc.kv(
        "Installed version",
        berry_downloads::installed_version(&report.settings.path).unwrap_or_else(unknown),
    );
    doc.kv("Install directory", report.settings.path.display());
    doc.kv("Executable", report.executable.display());
    doc.kv("Working directory", report.cwd.display());
    doc.kv("Compat data", compat.display());
    doc.kv(
        "Wine prefix",
        format!(
            "{} ({})",
            report.prefix.display(),
            if report.prefix_existed {
                "existing"
            } else {
                "new, created for this launch"
            },
        ),
    );
    if let Some(version) = read_trimmed(compat.join("version")) {
        doc.kv("Prefix created by", version);
    }
}

fn options_section(doc: &mut Doc, report: &LaunchReport) {
    let settings = report.settings;
    doc.section("Launch options");
    doc.kv("Proton verb", report.verb);
    doc.kv(
        "GameMode",
        match (report.gamemode_requested, report.gamemode_active) {
            (false, _) => "off",
            (true, true) => "on (gamemoderun wraps the launch chain)",
            (true, false) => {
                "requested but NOT active (gamemoderun missing or failed its self-test)"
            }
        },
    );
    doc.kv("DXVK HUD", settings.dxvk_hud.as_str());
    doc.kv(
        "FSR (fullscreen hack)",
        if settings.fsr {
            format!("on, preset {}", settings.fsr_quality.as_str())
        } else {
            "off".to_owned()
        },
    );
    doc.kv(
        "Gamescope",
        match (settings.gamescope.enabled, report.gamescope_active) {
            (false, _) => "off".to_owned(),
            (true, true) => format!(
                "on: gamescope {} --  [{}]",
                settings.gamescope.args().join(" "),
                report
                    .gamescope_source
                    .as_deref()
                    .unwrap_or("source unknown"),
            ),
            (true, false) if report.on_host => {
                "requested but NOT active (gamescope was not found on the host)".to_owned()
            }
            (true, false) if berry_core::settings::in_flatpak() => format!(
                "requested but NOT active (the {} extension is not installed in the sandbox)",
                super::gamescope::EXTENSION_ID,
            ),
            (true, false) => {
                "requested but NOT active (gamescope was not found on the system)".to_owned()
            }
        },
    );
    if !settings.wrapper.trim().is_empty() {
        doc.kv("Command prefix", settings.wrapper.trim());
    }
    if !settings.launch_args.trim().is_empty() {
        doc.kv("Launch arguments", settings.launch_args.trim());
    }
    if settings.dedicated_gpu {
        doc.kv(
            "Dedicated GPU",
            "requested (PRIME render offload variables are set)",
        );
    }
    if settings.proton_log {
        doc.kv(
            "Proton log",
            "on (PROTON_LOG=1, written next to the prefix)",
        );
    }
    if report.game.id == "wuthering-waves" {
        doc.kv(
            "Graphics API",
            format!(
                "{} (resource tier -krqlv={})",
                settings.graphics_api.as_str(),
                GameLauncher::wuwa_resource_tier(settings),
            ),
        );
    }
    if !settings.fps_unlocker {
        doc.kv("FPS unlocker", "off");
    } else {
        let mut summary = format!(
            "on, target {} fps, check interval {} ms",
            settings.fps, settings.fps_interval_ms
        );
        if let Some(support) = report.game.fps_unlock() {
            if support.uses_delays {
                summary.push_str(&format!(", init delay {} ms", settings.fps_init_delay_ms));
            }
            if support.clamp(settings.fps) != settings.fps {
                summary.push_str(&format!(
                    " (limited to {} fps for this game)",
                    support.clamp(settings.fps)
                ));
            }
        }
        doc.kv("FPS unlocker", summary);
        match unlocker_game_id(report.game) {
            Some(id) => {
                doc.kv("Unlocker game id", id);
                let dir = GameLauncher::fps_unlock_dir();
                doc.kv(
                    "Unlocker executable",
                    GameLauncher::find_unlocker(&dir).map_or_else(
                        || {
                            format!(
                                "not downloaded yet (fetched on launch into {})",
                                dir.display()
                            )
                        },
                        |exe| exe.display().to_string(),
                    ),
                );
                doc.kv(
                    "Unlocker game dir",
                    GameLauncher::unlocker_game_dir(report.game, report.executable, &settings.path)
                        .display(),
                );
                doc.kv("Unlocker output", dir.join("last-run.log").display());
                doc.kv(
                    "Unlocker started",
                    if report.launch_script.is_some() {
                        "from the gamescope launch script (it shares gamescope's display with the game)"
                    } else {
                        "by the launcher, next to the game"
                    },
                );
                doc.kv(
                    "Registry preparation",
                    if registry_fps(report.game).is_some() {
                        "required (FPS is written to the game's graphics registry key first)"
                    } else {
                        "not needed"
                    },
                );
            }
            None => doc.kv(
                "Unlocker support",
                "this game is not supported by the unlocker",
            ),
        }
    }
    if settings.environment.is_empty() {
        doc.kv("Custom environment", "none");
    } else {
        doc.kv(
            "Custom environment",
            format!("{} variable(s), listed below", settings.environment.len()),
        );
    }
}

fn environment_section(doc: &mut Doc, report: &LaunchReport) {
    doc.section("Environment passed to the runner");
    let mut env: Vec<_> = report.env.iter().collect();
    env.sort();
    for (key, value) in env {
        let origin = if report.settings.environment.contains_key(key) {
            "  # custom"
        } else {
            ""
        };
        doc.item(format!("{key}={value}{origin}"));
    }

    doc.section("Inherited environment (relevant variables)");
    if report.on_host {
        doc.item(
            "not applicable: the game runs on the host, only the variables above are forwarded",
        );
        return;
    }
    let mut inherited: Vec<(String, String)> = std::env::vars()
        .filter(|(key, _)| {
            RELEVANT_PREFIXES
                .iter()
                .any(|prefix| key.starts_with(prefix))
        })
        .collect();
    inherited.sort();
    if inherited.is_empty() {
        doc.item("none");
    }
    for (key, value) in inherited {
        let overridden = report.env.iter().any(|(k, _)| *k == key);
        doc.item(format!(
            "{key}={value}{}",
            if overridden {
                "  # overridden above"
            } else {
                ""
            }
        ));
    }
    // Inside the sandbox the library path is cleaned before the game starts (see
    // `sandbox_library_path`); say what the game actually receives.
    let overridden = report.env.iter().any(|(k, _)| k == "LD_LIBRARY_PATH");
    if berry_core::settings::in_flatpak() && !overridden {
        if let Ok(raw) = std::env::var("LD_LIBRARY_PATH") {
            doc.item(match GameLauncher::sandbox_library_path(&raw) {
                Some(path) if path == raw => "LD_LIBRARY_PATH reaches the game unchanged".to_owned(),
                Some(path) => format!("LD_LIBRARY_PATH reaches the game as {path} (Flatpak extension entries removed)"),
                None => "LD_LIBRARY_PATH is unset for the game (it only listed Flatpak extension directories)".to_owned(),
            });
        }
    }
}

fn command_section(doc: &mut Doc, report: &LaunchReport) {
    doc.section("Command");
    doc.kv("Working directory", report.cwd.display());
    doc.kv(
        "Started through",
        if report.on_host {
            "flatpak-spawn --host"
        } else {
            "direct spawn"
        },
    );
    let line = report
        .argv
        .iter()
        .map(|argument| shell_quote(&argument.to_string_lossy()))
        .collect::<Vec<_>>()
        .join(" ");
    doc.kv("Command line", line);
    if let Some(script) = report.launch_script {
        doc.kv(
            "Launch script",
            format!(
                "{} (gamescope runs it instead of the game; the FPS unlocker starts from it so it shares gamescope's display)",
                script.display(),
            ),
        );
    }
}
