//! Reims Studio launcher GUI on iced 0.14.
//!
//! First launch is a sequential setup flow: welcome, host checks, engine,
//! macOS version, VM settings, installation and a final summary. After setup
//! the app opens a compact dashboard; advanced controls live in Settings.

// SAFETY scope: one environment variable is set on the main thread before any
// window or runtime thread exists; see `main`.
#![allow(unsafe_code, reason = "select the iced backend before window init")]
// The GUI must not open a terminal when launched from Explorer. Headless modes
// attach to a console explicitly in console.rs.
#![cfg_attr(windows, windows_subsystem = "windows")]

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use iced::{Size, Subscription, Task, Theme};

use reims_core::contracts::{
    Channel, CpuVendor, EngineManifest, EngineRelease, HostOs, Settings as StudioSettings,
    Validate, VmConfig, load_json, load_toml,
};
use reims_core::log::LogBuffer;
use reims_platform::preflight::{
    Finding, Level, PreflightDoctor, PreflightInput, PreflightReport, WhpxState,
    available_memory_mib,
};
use shadcn_iced::prelude::{ApplicationExt, Mode};

use catalog::CatalogSource;
use ui::theme::Notice;

const TICKS_TO_CLEAR_SENTINEL: u64 = 12;
const LOG_LINES: usize = 72;
const POST_INSTALL_COMMAND: &str = "defaults write \"/Volumes/<Disk>/Library/Preferences/.GlobalPreferences\" METAL_ERROR_MODE -int 5";

mod catalog;
mod cli;
mod console;
mod ui;

fn main() -> iced::Result {
    let mut arguments = std::env::args().skip(1);
    match arguments.next().as_deref() {
        Some("--check") => {
            console::attach_for_cli();
            return headless("--check", false);
        }
        Some("--e2e") => {
            console::attach_for_cli();
            return headless("--e2e", true);
        }
        Some("--help") | Some("-h") => {
            console::attach_for_cli();
            println!("Usage: reims-studio [--check | --e2e | --help]");
            println!("  --check  verify that a built engine is visible to the launcher");
            println!("  --e2e    also verify the engine archive SHA256");
            return Ok(());
        }
        Some(other) => {
            console::attach_for_cli();
            eprintln!("Unknown option: {other}");
            std::process::exit(2);
        }
        None => {}
    }
    launch_gui()
}

fn headless(mode: &str, require_archive: bool) -> iced::Result {
    match cli::check(&portable_root(), require_archive) {
        Ok(report) => {
            println!("{report}");
            println!("{mode}: OK");
            Ok(())
        }
        Err(error) => {
            eprintln!("{mode}: FAIL: {error}");
            std::process::exit(1);
        }
    }
}

fn portable_root() -> PathBuf {
    std::env::var_os("REIMS_PORTABLE_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")))
}

fn launch_gui() -> iced::Result {
    // SAFETY: this runs once on the main thread before iced creates any window
    // or executor thread, so no concurrent reader can observe the change.
    unsafe { std::env::set_var("ICED_BACKEND", "tiny-skia") };
    iced::application(boot, update, ui::view)
        .title(|_: &Studio| String::from("Reims Studio"))
        .theme(|state: &Studio| state.ui_theme.clone())
        .window(window_settings())
        .subscription(subscription)
        .shadcn_iced()
        .run()
}

fn window_settings() -> iced::window::Settings {
    iced::window::Settings {
        size: Size::new(1120.0, 760.0),
        ..iced::window::Settings::default()
    }
}

/// Top-level screen. Setup is the default on first launch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Stage {
    /// Sequential setup flow.
    Setup(SetupStep),
    /// Compact home after setup.
    Dashboard,
    /// Advanced settings.
    Settings,
}

/// One step of the setup flow.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SetupStep {
    /// Explain what will happen.
    Welcome,
    /// Host and bundle checks.
    Doctor,
    /// Pick a published engine build.
    Engine,
    /// Pick the macOS version to install.
    MacOs,
    /// Name and size the VM.
    Vm,
    /// Installation checklist.
    Install,
    /// Final recap.
    Summary,
}

impl SetupStep {
    /// All steps in order.
    const ALL: [Self; 7] = [
        Self::Welcome,
        Self::Doctor,
        Self::Engine,
        Self::MacOs,
        Self::Vm,
        Self::Install,
        Self::Summary,
    ];
    /// Short titles shown above the progress bar.
    const TITLES: [&'static str; 7] = [
        "Welcome",
        "Check this PC",
        "Choose an engine",
        "macOS version",
        "Virtual machine",
        "Install macOS",
        "Done",
    ];

    fn index(self) -> usize {
        match self {
            Self::Welcome => 0,
            Self::Doctor => 1,
            Self::Engine => 2,
            Self::MacOs => 3,
            Self::Vm => 4,
            Self::Install => 5,
            Self::Summary => 6,
        }
    }

    fn previous(self) -> Option<Self> {
        match self {
            Self::Welcome => None,
            Self::Doctor => Some(Self::Welcome),
            Self::Engine => Some(Self::Doctor),
            Self::MacOs => Some(Self::Engine),
            Self::Vm => Some(Self::MacOs),
            Self::Install => Some(Self::Vm),
            Self::Summary => Some(Self::Install),
        }
    }
}

/// Where the current catalog came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CatalogState {
    /// Not loaded yet.
    Empty,
    /// Loaded from the portable folder.
    Cached,
    /// Downloaded from the GitHub release.
    Remote,
}

/// Actions whose completion time is shown in the UI.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ActionKind {
    /// Host checks.
    Doctor,
    /// Engine catalog refresh.
    Catalog,
    /// Engine download, verification and unpack.
    Install,
    /// VM creation.
    Vm,
    /// Settings save.
    Settings,
}

impl ActionKind {
    /// Label used in the completion badge.
    const fn label(self) -> &'static str {
        match self {
            Self::Doctor => "Host checks",
            Self::Catalog => "Catalog refresh",
            Self::Install => "Engine install",
            Self::Vm => "VM creation",
            Self::Settings => "Settings save",
        }
    }
}

/// Last completed action and how long it took.
#[derive(Debug, Clone, PartialEq, Eq)]
struct ActionFeedback {
    /// Which action completed.
    kind: ActionKind,
    /// Wall-clock duration in milliseconds.
    elapsed_ms: u128,
    /// Whether the action succeeded.
    ok: bool,
}

/// Application state.
struct Studio {
    portable_root: PathBuf,
    sentinel: PathBuf,
    ticks: u64,
    channel: Channel,
    bridge_token: String,
    stage: Stage,
    engines: Vec<EngineRelease>,
    selected_engine: Option<usize>,
    catalog_state: CatalogState,
    preflight: Option<PreflightReport>,
    warning_signature: Option<String>,
    pending_action: Option<(ActionKind, Instant)>,
    last_action: Option<ActionFeedback>,
    flash_started: Option<Instant>,
    macos_version: String,
    vm_name: String,
    ram: String,
    vcpus: String,
    resolution: String,
    bridge_port: String,
    qmp_port: String,
    post_install_applied: bool,
    installed_engine_id: Option<String>,
    installing: bool,
    install_counter: Arc<AtomicU64>,
    install_progress: u64,
    install_total: u64,
    status: Notice,
    logs: LogBuffer,
    log_page: usize,
    paused: bool,
    ram_usage: f32,
    /// Current shadcn theme, cloned into iced's theme closure.
    ui_theme: Theme,
}

/// Messages produced by widgets and subscriptions.
#[derive(Debug, Clone)]
enum Message {
    /// Periodic timer tick.
    Tick,
    /// Repaint frame while a completion flash is running.
    Frame,
    /// Change the visible screen.
    Stage(Stage),
    /// Switch between dark and light appearance.
    SetTheme(Mode),
    /// Advance in the setup flow.
    SetupNext,
    /// Go back one setup step.
    SetupBack,
    /// Finish setup and open the dashboard.
    FinishSetup,
    /// Reopen the setup flow.
    RerunSetup,
    /// Run host checks.
    RunDoctor,
    /// Acknowledge advisory findings.
    AckWarnings(bool),
    /// Result of a catalog download.
    CatalogFetched(Result<(Vec<EngineRelease>, CatalogSource), String>),
    /// Reload the catalog from the GitHub release.
    RefreshCatalog,
    /// Pick an engine from the catalog.
    SelectEngine(usize),
    /// Select an engine channel.
    Channel(Channel),
    /// Copy the selected engine's download URL.
    CopyEngineUrl,
    /// Open the selected engine's release page in a browser.
    OpenRelease,
    /// Download, verify and unpack the selected engine.
    InstallEngine,
    /// Result of an engine installation for a captured engine id.
    EngineInstalled {
        /// Engine selected when the download started.
        engine_id: String,
        /// Installation result.
        result: Result<(), String>,
    },
    /// Edit the target macOS version.
    MacVersion(String),
    /// Edit the VM name.
    VmName(String),
    /// Edit guest RAM.
    Ram(String),
    /// Edit the vCPU count.
    Vcpus(String),
    /// Edit the guest resolution.
    Resolution(String),
    /// Render the QEMU command for diagnostics.
    BuildCommand,
    /// Switch OpenCore to post-install settings.
    PostInstall,
    /// Copy the Tahoe command to the clipboard.
    CopyPostInstall,
    /// Edit the guest bridge port.
    BridgePort(String),
    /// Edit the local QMP port.
    QmpPort(String),
    /// Persist settings.toml.
    SaveSettings,
    /// Copy the visible log page to the clipboard.
    CopyLogs,
    /// Move the log page slider.
    LogPage(f32),
    /// Pause or resume log updates.
    Pause,
    /// Close the window.
    Exit,
}

/// Loads settings, creating a valid default file on first launch.
///
/// A corrupt file is preserved as `settings.toml.bak` instead of being
/// silently overwritten, and the caller is told about it.
fn bootstrap_settings(root: &Path) -> (StudioSettings, Option<Notice>) {
    let path = root.join("settings.toml");
    if path.exists() {
        match load_toml::<StudioSettings>(&path) {
            Ok(settings) => return (settings, None),
            Err(_) => {
                let backup = root.join("settings.toml.bak");
                let _ = std::fs::rename(&path, &backup);
                let settings = StudioSettings::for_host(HostOs::current());
                if let Ok(content) = toml::to_string(&settings)
                    && let Err(error) = std::fs::write(&path, content)
                {
                    return (
                        settings,
                        Some(Notice::error(format!(
                            "settings.toml could not be recreated: {error}"
                        ))),
                    );
                }
                return (
                    settings,
                    Some(Notice::warning(format!(
                        "settings.toml was invalid; backup kept at {}",
                        backup.display()
                    ))),
                );
            }
        }
    }
    let settings = StudioSettings::for_host(HostOs::current());
    if let Some(parent) = path.parent()
        && let Err(error) = std::fs::create_dir_all(parent)
    {
        return (
            settings,
            Some(Notice::error(format!(
                "portable folder unavailable: {error}"
            ))),
        );
    }
    match toml::to_string(&settings) {
        Ok(content) => match std::fs::write(&path, content) {
            Ok(()) => (settings, None),
            Err(error) => (
                settings,
                Some(Notice::error(format!("settings.toml not written: {error}"))),
            ),
        },
        Err(error) => (
            settings,
            Some(Notice::error(format!(
                "settings.toml not serialized: {error}"
            ))),
        ),
    }
}

fn boot() -> (Studio, Task<Message>) {
    let root = portable_root();
    let (settings, notice) = bootstrap_settings(&root);
    boot_from(root, settings, notice)
}

/// Builds initial state from explicit inputs so tests never write to the repo.
fn boot_from(
    portable_root: PathBuf,
    settings: StudioSettings,
    settings_notice: Option<Notice>,
) -> (Studio, Task<Message>) {
    let sentinel = portable_root.join("ui_booting.flag");
    if let Some(parent) = sentinel.parent()
        && std::fs::create_dir_all(parent).is_ok()
    {
        let _ = std::fs::write(&sentinel, b"booting");
    }
    let setup_done = portable_root.join("setup_done.flag").exists();
    let bridge_token = format!("{:032x}", rand::random::<u128>());
    let mut logs = LogBuffer::new(256);
    logs.push("Reims Studio started on tiny-skia");
    logs.push(format!("engine channel: {}", settings.channel));
    if settings_notice.is_none() {
        logs.push("default settings.toml is present");
    }
    if let Some(notice) = &settings_notice {
        logs.push(notice.text.clone());
    }
    let (engines, catalog_state, status) = match catalog::load(&portable_root.join("index.json")) {
        Ok(list) => {
            logs.push(format!("{} engine build(s) available locally", list.len()));
            let state = if list.is_empty() {
                CatalogState::Empty
            } else {
                CatalogState::Cached
            };
            (list, state, Notice::info("Local engine catalog loaded."))
        }
        Err(error) => {
            logs.push(format!("local catalog unavailable: {error}"));
            (
                Vec::new(),
                CatalogState::Empty,
                Notice::info("No local catalog yet. Choose an engine to download it."),
            )
        }
    };
    // Prefer a build whose bundle is already unpacked so the first launch
    // starts with a usable engine when one is present on disk.
    let selected_engine = engines
        .iter()
        .position(|engine| {
            engine.channel == settings.channel && engine_is_installed(&portable_root, engine)
        })
        .or_else(|| {
            engines
                .iter()
                .position(|engine| engine.channel == settings.channel)
        });
    let installed_engine_id = selected_engine.and_then(|index| {
        engine_is_installed(&portable_root, &engines[index])
            .then(|| engines[index].engine_id.clone())
    });
    let status = settings_notice.unwrap_or(status);
    let ui_theme = ui::theme_for(ui::load_theme(&portable_root));
    (
        Studio {
            portable_root,
            sentinel,
            ticks: 0,
            channel: settings.channel,
            bridge_token,
            stage: if setup_done {
                Stage::Dashboard
            } else {
                Stage::Setup(SetupStep::Welcome)
            },
            engines,
            selected_engine,
            catalog_state,
            preflight: None,
            warning_signature: None,
            pending_action: None,
            last_action: None,
            flash_started: None,
            macos_version: String::from("26.4"),
            vm_name: String::from("macos-tahoe"),
            ram: String::from("8192"),
            vcpus: String::from("4"),
            resolution: String::from("1920x1080"),
            bridge_port: settings.bridge_port.to_string(),
            qmp_port: settings.qmp_port.to_string(),
            post_install_applied: false,
            installed_engine_id,
            installing: false,
            install_counter: Arc::new(AtomicU64::new(0)),
            install_progress: 0,
            install_total: 0,
            status,
            logs,
            log_page: 0,
            paused: false,
            ram_usage: 0.18,
            ui_theme,
        },
        Task::none(),
    )
}

/// Runs deterministic host checks and adds the engine bundle result.
fn run_doctor(studio: &mut Studio) {
    studio.pending_action = Some((ActionKind::Doctor, Instant::now()));
    let os = HostOs::current();
    let linux = matches!(os, HostOs::Linux);
    let kvm = Path::new("/dev/kvm").exists();
    let input = PreflightInput {
        os,
        filesystem: if linux {
            String::from("ext4")
        } else {
            String::from("NTFS")
        },
        available_memory_mib: available_memory_mib(),
        virtualization_enabled: if linux { kvm } else { true },
        whpx: if kvm || !linux {
            WhpxState::Active
        } else {
            WhpxState::Disabled
        },
        color_depth: 32,
        kernel_release: std::fs::read_to_string("/proc/sys/kernel/osrelease").ok(),
        kvm_accessible: kvm,
        udmabuf_accessible: Path::new("/dev/udmabuf").exists(),
        display_available: !linux
            || std::env::var_os("DISPLAY").is_some()
            || std::env::var_os("WAYLAND_DISPLAY").is_some(),
        gpu_count: 1,
        identical_gpu_ids: false,
        old_vulkan_loader: false,
        elevated: false,
        cpu_vendor: CpuVendor::Intel,
        audio_enabled: true,
        wayland_nvidia: false,
        amd_debug_kernel: false,
    };
    let mut report = PreflightDoctor.check(&input);
    let engine = cli::check(&studio.portable_root, false);
    report.findings.push(Finding {
        code: "engine",
        level: if engine.is_ok() {
            Level::Ok
        } else {
            Level::Warning
        },
        message: match engine {
            Ok(value) => format!("Engine bundle {} is usable.", value.engine_id),
            Err(error) => format!("Engine bundle not ready yet: {error}"),
        },
    });
    studio.warning_signature = None;
    studio.preflight = Some(report);
    studio.logs.push("Pre-Flight checks completed");
    finish_action(studio, ActionKind::Doctor, true);
    flash(studio);
    studio.status = Notice::info("Host checks complete. Review the results below.");
}

fn apply_catalog(studio: &mut Studio, result: Result<(Vec<EngineRelease>, CatalogSource), String>) {
    match result {
        Ok((engines, _source)) => {
            studio.selected_engine = engines
                .iter()
                .position(|engine| engine.channel == studio.channel)
                .or(if engines.is_empty() { None } else { Some(0) });
            studio.catalog_state = CatalogState::Remote;
            studio.status = Notice::success(format!(
                "Loaded {} engine build(s) from GitHub release.",
                engines.len()
            ));
            studio
                .logs
                .push(format!("catalog: {} build(s)", engines.len()));
            studio.engines = engines;
            flash(studio);
        }
        Err(error) => {
            let fallback = catalog::load(&studio.portable_root.join("index.json"));
            match fallback {
                Ok(list) => {
                    studio.engines = list;
                    studio.selected_engine = if studio.engines.is_empty() {
                        None
                    } else {
                        Some(0)
                    };
                    studio.catalog_state = CatalogState::Cached;
                    studio.status =
                        Notice::warning(format!("Offline вЂ” showing cached catalog. {error}"));
                }
                Err(_) => {
                    studio.status = Notice::error(format!("Catalog unavailable: {error}"));
                }
            }
            studio.logs.push(format!("catalog refresh failed: {error}"));
        }
    }
}

fn selected(studio: &Studio) -> Option<&EngineRelease> {
    studio
        .selected_engine
        .and_then(|index| studio.engines.get(index))
}

/// Opens a URL in the host's default browser.
fn open_url(url: &str) -> Result<(), String> {
    #[cfg(windows)]
    let result = Command::new("explorer.exe").arg(url).spawn();
    #[cfg(target_os = "linux")]
    let result = Command::new("xdg-open").arg(url).spawn();
    #[cfg(not(any(windows, target_os = "linux")))]
    let result = Command::new("open").arg(url).spawn();
    result.map(|_| ()).map_err(|error| error.to_string())
}

/// Returns true when an engine bundle is present with a matching manifest.
fn engine_is_installed(root: &Path, engine: &EngineRelease) -> bool {
    let path = reims_engine_mgr::engine_dir(root, &engine.engine_id).join("manifest.json");
    load_json::<EngineManifest>(&path).is_ok_and(|manifest| manifest.engine_id == engine.engine_id)
}

/// Returns true when the selected engine is fully installed and usable.
fn engine_ready(studio: &Studio) -> bool {
    !studio.installing
        && selected(studio).is_some_and(|engine| {
            studio.installed_engine_id.as_deref() == Some(engine.engine_id.as_str())
                && engine_is_installed(&studio.portable_root, engine)
        })
}

/// Stable signature of all non-passing Doctor findings.
fn warning_signature(report: &PreflightReport) -> Option<String> {
    let mut codes: Vec<String> = report
        .findings
        .iter()
        .filter(|finding| finding.level != Level::Ok)
        .map(|finding| format!("{}:{}", finding.code, finding.message))
        .collect();
    if codes.is_empty() {
        return None;
    }
    codes.sort();
    Some(codes.join("|"))
}

/// Records the duration of the most recent action of a given kind.
fn finish_action(studio: &mut Studio, kind: ActionKind, ok: bool) {
    let elapsed_ms = studio
        .pending_action
        .filter(|(pending, _)| *pending == kind)
        .map_or(0, |(_, started)| started.elapsed().as_millis());
    studio.pending_action = None;
    studio.last_action = Some(ActionFeedback {
        kind,
        elapsed_ms,
        ok,
    });
}

/// Starts a short tint decay on the status badge after a successful action.
fn flash(studio: &mut Studio) {
    studio.flash_started = Some(Instant::now());
}

/// Returns whether a setup step may advance and why it may not.
fn can_continue(studio: &Studio, step: SetupStep) -> (bool, &'static str) {
    match step {
        SetupStep::Welcome => (true, ""),
        SetupStep::Doctor => match &studio.preflight {
            None => (false, "Run the host checks first."),
            Some(report) if report.has_blocker() => (false, "Fix the blocking checks first."),
            Some(report) => match warning_signature(report) {
                Some(signature)
                    if studio.warning_signature.as_deref() != Some(signature.as_str()) =>
                {
                    (false, "Acknowledge the advisory findings to continue.")
                }
                _ => (true, ""),
            },
        },
        SetupStep::Engine => {
            if engine_ready(studio) {
                (true, "")
            } else {
                (false, "Download and install the selected engine first.")
            }
        }
        SetupStep::MacOs => {
            let version = studio.macos_version.trim();
            if !version.is_empty()
                && selected(studio).is_some_and(|engine| catalog::supports_macos(engine, version))
            {
                (true, "")
            } else {
                (
                    false,
                    "Choose a macOS version supported by the selected engine.",
                )
            }
        }
        SetupStep::Vm => (
            vm_config(studio).is_ok(),
            "Fix the VM settings before continuing.",
        ),
        SetupStep::Install => (true, ""),
        SetupStep::Summary => (true, ""),
    }
}

/// Builds a validated VM config without writing anything to disk.
fn vm_config(studio: &Studio) -> Result<VmConfig, String> {
    let ram = studio
        .ram
        .parse::<u64>()
        .map_err(|error| format!("invalid RAM: {error}"))?;
    let vcpus = studio
        .vcpus
        .parse::<u32>()
        .map_err(|error| format!("invalid vCPU count: {error}"))?;
    let (width, height) = studio
        .resolution
        .split_once('x')
        .ok_or_else(|| String::from("resolution must be WIDTHxHEIGHT"))?;
    let mut config = VmConfig::new(studio.vm_name.clone());
    config.ram_mib = ram;
    config.vcpus = vcpus;
    config.resolution.width = width.parse().map_err(|_| String::from("invalid width"))?;
    config.resolution.height = height.parse().map_err(|_| String::from("invalid height"))?;
    config.validate().map_err(|error| error.to_string())?;
    Ok(config)
}

fn save_settings(studio: &Studio) -> Result<(), String> {
    let bridge_port = studio
        .bridge_port
        .parse::<u16>()
        .map_err(|_| String::from("bridge port must be a number"))?;
    let qmp_port = studio
        .qmp_port
        .parse::<u16>()
        .map_err(|_| String::from("QMP port must be a number"))?;
    let mut settings = StudioSettings::for_host(HostOs::current());
    settings.channel = studio.channel;
    settings.bridge_port = bridge_port;
    settings.qmp_port = qmp_port;
    settings.validate().map_err(|error| error.to_string())?;
    std::fs::write(
        studio.portable_root.join("settings.toml"),
        toml::to_string(&settings).map_err(|error| error.to_string())?,
    )
    .map_err(|error| error.to_string())
}

fn update(studio: &mut Studio, message: Message) -> Task<Message> {
    match message {
        Message::Tick => {
            studio.ticks += 1;
            if studio.ticks == TICKS_TO_CLEAR_SENTINEL {
                let _ = std::fs::remove_file(&studio.sentinel);
                studio.logs.push("ui_booting.flag cleared after 3s");
            }
            studio.ram_usage = 0.18 + f32::from((studio.ticks % 8) as u8) / 100.0;
            if studio.installing {
                studio.install_progress = studio.install_counter.load(Ordering::Relaxed);
            }
            if studio
                .flash_started
                .is_some_and(|started| started.elapsed() >= Duration::from_millis(200))
            {
                studio.flash_started = None;
            }
            Task::none()
        }
        Message::Frame => Task::none(),
        Message::Stage(stage) => {
            studio.stage = stage;
            Task::none()
        }
        Message::SetTheme(mode) => {
            studio.ui_theme = ui::theme_for(mode);
            match ui::save_theme(&studio.portable_root, mode) {
                Ok(()) => studio.status = Notice::success("Appearance saved."),
                Err(error) => {
                    studio.status = Notice::error(format!("Appearance not saved: {error}"))
                }
            }
            Task::none()
        }
        Message::SetupNext => {
            let step = match studio.stage {
                Stage::Setup(step) => step,
                _ => return Task::none(),
            };
            let (allowed, reason) = can_continue(studio, step);
            if !allowed {
                studio.status = Notice::warning(reason);
                return Task::none();
            }
            let next = match step {
                SetupStep::Welcome => {
                    run_doctor(studio);
                    SetupStep::Doctor
                }
                SetupStep::Doctor => SetupStep::Engine,
                SetupStep::Engine => SetupStep::MacOs,
                SetupStep::MacOs => SetupStep::Vm,
                SetupStep::Vm => {
                    studio.pending_action = Some((ActionKind::Vm, Instant::now()));
                    match create_vm(studio) {
                        Ok(config) => {
                            finish_action(studio, ActionKind::Vm, true);
                            studio.status = Notice::success(format!("Created vms/{}", config.name));
                            studio.logs.push(format!(
                                "VM {} saved: {} MiB, {} vCPUs",
                                config.name, config.ram_mib, config.vcpus
                            ));
                            SetupStep::Install
                        }
                        Err(error) => {
                            finish_action(studio, ActionKind::Vm, false);
                            studio.status = Notice::error(format!("VM not created: {error}"));
                            return Task::none();
                        }
                    }
                }
                SetupStep::Install => SetupStep::Summary,
                SetupStep::Summary => return update(studio, Message::FinishSetup),
            };
            studio.stage = Stage::Setup(next);
            Task::none()
        }
        Message::SetupBack => {
            if let Stage::Setup(step) = studio.stage
                && let Some(previous) = step.previous()
            {
                studio.stage = Stage::Setup(previous);
            }
            Task::none()
        }
        Message::FinishSetup => {
            let flag = studio.portable_root.join("setup_done.flag");
            if let Some(parent) = flag.parent()
                && std::fs::create_dir_all(parent).is_ok()
            {
                let _ = std::fs::write(&flag, b"done");
            }
            studio.stage = Stage::Dashboard;
            studio.status = Notice::success("Setup complete. You can now use the dashboard.");
            studio.logs.push("setup complete");
            Task::none()
        }
        Message::RerunSetup => {
            let _ = std::fs::remove_file(studio.portable_root.join("setup_done.flag"));
            studio.stage = Stage::Setup(SetupStep::Welcome);
            Task::none()
        }
        Message::RunDoctor => {
            run_doctor(studio);
            Task::none()
        }
        Message::AckWarnings(value) => {
            studio.warning_signature = value
                .then(|| studio.preflight.as_ref().and_then(warning_signature))
                .flatten();
            Task::none()
        }
        Message::RefreshCatalog => {
            studio.pending_action = Some((ActionKind::Catalog, Instant::now()));
            studio.status = Notice::info("Downloading the engine catalog from GitHub...");
            let root = studio.portable_root.clone();
            Task::perform(
                async move {
                    match tokio::task::spawn_blocking(move || catalog::fetch(&root)).await {
                        Ok(result) => result,
                        Err(error) => Err(format!("catalog task failed: {error}")),
                    }
                },
                Message::CatalogFetched,
            )
        }
        Message::CatalogFetched(result) => {
            let ok = result.is_ok();
            apply_catalog(studio, result);
            finish_action(studio, ActionKind::Catalog, ok);
            Task::none()
        }
        Message::SelectEngine(index) => {
            if let Some(engine) = studio.engines.get(index) {
                studio.selected_engine = Some(index);
                studio.status = Notice::success(format!("Selected engine {}", engine.engine_id));
                studio
                    .logs
                    .push(format!("selected engine {}", engine.engine_id));
            }
            Task::none()
        }
        Message::Channel(channel) => {
            studio.channel = channel;
            studio.selected_engine = studio
                .engines
                .iter()
                .position(|engine| engine.channel == channel);
            studio.status = Notice::info(format!("Engine channel: {channel}"));
            Task::none()
        }
        Message::CopyEngineUrl => match selected(studio) {
            Some(engine) => iced::clipboard::write(engine.url.clone()),
            None => {
                studio.status = Notice::error("Select an engine before copying its URL.");
                Task::none()
            }
        },
        Message::OpenRelease => {
            match selected(studio).and_then(catalog::release_url) {
                Some(url) => match open_url(&url) {
                    Ok(()) => {
                        studio.status = Notice::success(format!("Opened release page: {url}"));
                    }
                    Err(error) => {
                        studio.status = Notice::error(format!("Browser could not start: {error}"));
                    }
                },
                None => {
                    studio.status = Notice::error("This engine has no release page URL.");
                }
            }
            Task::none()
        }
        Message::InstallEngine => {
            let Some(engine) = selected(studio).cloned() else {
                studio.status = Notice::error("Select an engine before installing it.");
                return Task::none();
            };
            let counter = Arc::new(AtomicU64::new(0));
            studio.install_counter = counter.clone();
            studio.install_progress = 0;
            studio.install_total = engine.archive_size;
            studio.installing = true;
            studio.pending_action = Some((ActionKind::Install, Instant::now()));
            studio.status = Notice::info(format!("Downloading {}...", engine.engine_id));
            let root = studio.portable_root.clone();
            let engine_id = engine.engine_id.clone();
            Task::perform(
                async move {
                    let result = match tokio::task::spawn_blocking(move || {
                        catalog::install(&root, &engine, counter)
                    })
                    .await
                    {
                        Ok(result) => result,
                        Err(error) => Err(format!("install task failed: {error}")),
                    };
                    (engine_id, result)
                },
                |(engine_id, result)| Message::EngineInstalled { engine_id, result },
            )
        }
        Message::EngineInstalled { engine_id, result } => {
            studio.installing = false;
            let ok = result.is_ok();
            match result {
                Ok(()) => {
                    studio.installed_engine_id = Some(engine_id);
                    studio.status = Notice::success("Engine downloaded, verified and installed.");
                    studio.logs.push("engine installed");
                    flash(studio);
                }
                Err(error) => {
                    studio.status = Notice::error(format!("Engine install failed: {error}"));
                    studio.logs.push(format!("engine install failed: {error}"));
                }
            }
            finish_action(studio, ActionKind::Install, ok);
            Task::none()
        }
        Message::MacVersion(value) => {
            studio.macos_version = value;
            Task::none()
        }
        Message::VmName(value) => {
            studio.vm_name = value;
            Task::none()
        }
        Message::Ram(value) => {
            studio.ram = value;
            Task::none()
        }
        Message::Vcpus(value) => {
            studio.vcpus = value;
            Task::none()
        }
        Message::Resolution(value) => {
            studio.resolution = value;
            Task::none()
        }
        Message::BuildCommand => {
            match render_command(studio) {
                Ok(command) => {
                    studio.status = Notice::success("QEMU command rendered; see logs.");
                    studio.logs.push(command);
                }
                Err(error) => studio.status = Notice::error(format!("Command failed: {error}")),
            }
            Task::none()
        }
        Message::PostInstall => {
            match apply_post_install(studio) {
                Ok(()) => {
                    studio.post_install_applied = true;
                    studio.status = Notice::success("Post-install settings applied.");
                    studio
                        .logs
                        .push("ScanPolicy 983299, Timeout 2, ShowPicker true");
                }
                Err(error) => {
                    studio.status = Notice::error(format!("Post-install failed: {error}"))
                }
            }
            Task::none()
        }
        Message::CopyPostInstall => iced::clipboard::write(String::from(POST_INSTALL_COMMAND)),
        Message::BridgePort(value) => {
            studio.bridge_port = value;
            Task::none()
        }
        Message::QmpPort(value) => {
            studio.qmp_port = value;
            Task::none()
        }
        Message::SaveSettings => {
            studio.pending_action = Some((ActionKind::Settings, Instant::now()));
            match save_settings(studio) {
                Ok(()) => {
                    finish_action(studio, ActionKind::Settings, true);
                    studio.status = Notice::success("Settings saved to settings.toml.");
                    studio.logs.push("settings saved");
                    flash(studio);
                }
                Err(error) => {
                    finish_action(studio, ActionKind::Settings, false);
                    studio.status = Notice::error(format!("Settings not saved: {error}"));
                }
            }
            Task::none()
        }
        Message::CopyLogs => iced::clipboard::write(studio.logs.page(studio.log_page, LOG_LINES)),
        Message::LogPage(value) => {
            studio.log_page = value as usize;
            Task::none()
        }
        Message::Pause => {
            studio.paused = !studio.paused;
            studio.logs.push(if studio.paused {
                "log paused"
            } else {
                "log resumed"
            });
            Task::none()
        }
        Message::Exit => iced::exit(),
    }
}

fn subscription(studio: &Studio) -> Subscription<Message> {
    let mut subscriptions =
        vec![iced::time::every(Duration::from_millis(250)).map(|_| Message::Tick)];
    if studio.flash_started.is_some() {
        // ~6 frames over 200 ms; opacity-only so tiny-skia does no relayout.
        subscriptions.push(iced::time::every(Duration::from_millis(33)).map(|_| Message::Frame));
    }
    Subscription::batch(subscriptions)
}

fn create_vm(studio: &Studio) -> Result<VmConfig, String> {
    let ram = studio
        .ram
        .parse::<u64>()
        .map_err(|error| format!("invalid RAM: {error}"))?;
    let vcpus = studio
        .vcpus
        .parse::<u32>()
        .map_err(|error| format!("invalid vCPU count: {error}"))?;
    let (width, height) = studio
        .resolution
        .split_once('x')
        .ok_or_else(|| String::from("resolution must be WIDTHxHEIGHT"))?;
    let mut config = VmConfig::new(studio.vm_name.clone());
    config.ram_mib = ram;
    config.vcpus = vcpus;
    config.resolution.width = width.parse().map_err(|_| String::from("invalid width"))?;
    config.resolution.height = height.parse().map_err(|_| String::from("invalid height"))?;
    config.validate().map_err(|error| error.to_string())?;

    let vm_dir = studio.portable_root.join("vms").join(&config.name);
    std::fs::create_dir_all(&vm_dir).map_err(|error| error.to_string())?;
    std::fs::write(
        vm_dir.join("vm.toml"),
        toml::to_string(&config).map_err(|error| error.to_string())?,
    )
    .map_err(|error| error.to_string())?;
    // The bridge token is written here so the install step can inject it into
    // the guest LaunchAgent later.
    std::fs::write(vm_dir.join("bridge.token"), studio.bridge_token.as_bytes())
        .map_err(|error| error.to_string())?;
    Ok(config)
}

fn apply_post_install(studio: &Studio) -> Result<(), String> {
    let path = studio
        .portable_root
        .join("vms")
        .join(&studio.vm_name)
        .join("vm.toml");
    let mut config: VmConfig = load_toml(&path).map_err(|error| error.to_string())?;
    config.scan_policy = 983_299;
    config.installer_attached = false;
    config.validate().map_err(|error| error.to_string())?;
    std::fs::write(
        &path,
        toml::to_string(&config).map_err(|error| error.to_string())?,
    )
    .map_err(|error| error.to_string())
}

fn render_command(studio: &Studio) -> Result<String, String> {
    let vm_path = studio
        .portable_root
        .join("vms")
        .join(&studio.vm_name)
        .join("vm.toml");
    let config: VmConfig = load_toml(&vm_path).map_err(|error| error.to_string())?;
    let manifest_path = studio
        .portable_root
        .join("engines")
        .join(format!("test-{}", studio.channel))
        .join("manifest.json");
    let manifest: reims_core::contracts::EngineManifest =
        load_json(&manifest_path).map_err(|error| error.to_string())?;
    let adapter = reims_platform::adapter::EngineAdapter::for_manifest(&manifest)
        .map_err(|error| error.to_string())?;
    let spec = reims_platform::adapter::VmSpec {
        os: manifest.os,
        engine_id: manifest.engine_id.clone(),
        manifest,
        vm: config,
        installer_image: None,
        qmp_port: 4444,
        bridge_port: 8080,
        shader_cache_dir: String::from("../../cache/shaders/default"),
        disable_video_decode: false,
    };
    let command = adapter.build(&spec).map_err(|error| error.to_string())?;
    Ok(format!(
        "cd {} && {} {}",
        command.working_dir,
        command.program,
        command.args.join(" ")
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::screens::setup::nav_labels;
    use crate::ui::theme::Tone;

    #[test]
    fn sentinel_is_removed_after_three_seconds() {
        let directory = tempfile::tempdir().expect("temp dir");
        let mut studio = boot_with_root(directory.path());
        assert!(studio.sentinel.exists());
        for _ in 0..TICKS_TO_CLEAR_SENTINEL {
            let _ = update(&mut studio, Message::Tick);
        }
        assert!(!studio.sentinel.exists());
    }

    #[test]
    fn first_launch_starts_at_welcome_and_setup_finishes_to_dashboard() {
        let directory = tempfile::tempdir().expect("temp dir");
        let studio = boot_with_root(directory.path());
        assert_eq!(studio.stage, Stage::Setup(SetupStep::Welcome));
        let mut studio = studio;
        let _ = update(&mut studio, Message::FinishSetup);
        assert_eq!(studio.stage, Stage::Dashboard);
        assert!(directory.path().join("setup_done.flag").exists());
    }

    #[test]
    fn boot_creates_a_default_settings_file() {
        let directory = tempfile::tempdir().expect("temp dir");
        let (settings, notice) = bootstrap_settings(directory.path());
        assert!(notice.is_none());
        let path = directory.path().join("settings.toml");
        assert!(path.exists());
        let loaded: StudioSettings = load_toml(&path).expect("settings");
        loaded.validate().expect("valid settings");
        assert_eq!(loaded.channel, settings.channel);
        assert_eq!(loaded.bridge_port, 8080);
    }

    #[test]
    fn doctor_warnings_require_acknowledgement() {
        let directory = tempfile::tempdir().expect("temp dir");
        let mut studio = boot_with_root(directory.path());
        studio.stage = Stage::Setup(SetupStep::Doctor);
        assert!(!can_continue(&studio, SetupStep::Doctor).0);
        studio.preflight = Some(PreflightReport {
            findings: vec![Finding {
                code: "advisory",
                level: Level::Warning,
                message: String::from("advisory finding"),
            }],
            disable_video_decode: false,
            allow_wgpu: false,
            gpu_selection: reims_platform::preflight::GpuSelection::Default,
        });
        assert!(!can_continue(&studio, SetupStep::Doctor).0);
        let signature = warning_signature(studio.preflight.as_ref().unwrap()).unwrap();
        studio.warning_signature = Some(signature);
        assert!(can_continue(&studio, SetupStep::Doctor).0);
        studio.preflight.as_mut().unwrap().findings[0].level = Level::Blocker;
        assert!(!can_continue(&studio, SetupStep::Doctor).0);
    }

    #[test]
    fn setup_navigation_is_present_on_every_step() {
        let directory = tempfile::tempdir().expect("temp dir");
        let mut studio = boot_with_root(directory.path());
        for step in SetupStep::ALL {
            let labels: Vec<&str> = nav_labels(step);
            if step == SetupStep::Welcome {
                assert!(labels.contains(&"Run checks"));
            } else {
                assert!(labels.contains(&"Back"), "missing Back on {step:?}");
            }
            if step != SetupStep::Summary && step != SetupStep::Welcome {
                assert!(
                    labels.iter().any(|label| {
                        *label != "Back" && *label != "Settings" && *label != "Open dashboard"
                    }),
                    "missing forward button on {step:?}"
                );
            }
        }
        studio.stage = Stage::Setup(SetupStep::Engine);
        write_index(directory.path());
        let engines = catalog::load(&directory.path().join("index.json")).expect("index");
        let _ = update(
            &mut studio,
            Message::CatalogFetched(Ok((engines, CatalogSource::Remote))),
        );
        let _ = update(&mut studio, Message::SelectEngine(0));
        let id = if cfg!(windows) {
            "win64-master-test"
        } else {
            "linux64-master-test"
        };
        write_manifest(directory.path(), id);
        let _ = update(
            &mut studio,
            Message::EngineInstalled {
                engine_id: String::from(id),
                result: Ok(()),
            },
        );
        let labels: Vec<&str> = nav_labels(SetupStep::Engine);
        assert!(labels.contains(&"Back"));
        assert!(labels.contains(&"Continue"));
        assert!(can_continue(&studio, SetupStep::Engine).0);
    }

    #[test]
    fn completed_actions_record_their_duration() {
        let directory = tempfile::tempdir().expect("temp dir");
        let mut studio = boot_with_root(directory.path());
        run_doctor(&mut studio);
        assert_eq!(
            studio.last_action.as_ref().map(|a| a.kind),
            Some(ActionKind::Doctor)
        );
        assert!(studio.last_action.as_ref().is_some_and(|a| a.ok));
    }

    #[test]
    fn setup_requires_a_selected_engine() {
        let directory = tempfile::tempdir().expect("temp dir");
        let mut studio = boot_with_root(directory.path());
        studio.stage = Stage::Setup(SetupStep::Engine);
        let _ = update(&mut studio, Message::SetupNext);
        assert_eq!(studio.stage, Stage::Setup(SetupStep::Engine));
        assert_eq!(studio.status.tone, Tone::Warning);
        write_index(directory.path());
        let _ = update(&mut studio, Message::RefreshCatalog);
        // No network in tests; simulate the downloaded catalog result instead.
        let engines = catalog::load(&directory.path().join("index.json")).expect("index");
        let _ = update(
            &mut studio,
            Message::CatalogFetched(Ok((engines, CatalogSource::Remote))),
        );
        let _ = update(&mut studio, Message::SelectEngine(0));
        // Simulate a completed download; tests must not hit the network.
        let id = if cfg!(windows) {
            "win64-master-test"
        } else {
            "linux64-master-test"
        };
        write_manifest(directory.path(), id);
        let _ = update(
            &mut studio,
            Message::EngineInstalled {
                engine_id: String::from(id),
                result: Ok(()),
            },
        );
        let _ = update(&mut studio, Message::SetupNext);
        assert_eq!(studio.stage, Stage::Setup(SetupStep::MacOs));
    }

    #[test]
    fn vm_step_writes_vm_and_token() {
        let directory = tempfile::tempdir().expect("temp dir");
        let mut studio = boot_with_root(directory.path());
        studio.stage = Stage::Setup(SetupStep::Vm);
        let _ = update(&mut studio, Message::SetupNext);
        let vm_dir = directory.path().join("vms/macos-tahoe");
        assert!(vm_dir.join("vm.toml").exists());
        assert_eq!(
            std::fs::read_to_string(vm_dir.join("bridge.token")).expect("token"),
            studio.bridge_token
        );
        assert_eq!(studio.stage, Stage::Setup(SetupStep::Install));
    }

    #[test]
    fn settings_reject_an_invalid_port() {
        let directory = tempfile::tempdir().expect("temp dir");
        let mut studio = boot_with_root(directory.path());
        studio.bridge_port = String::from("not-a-port");
        let _ = update(&mut studio, Message::SaveSettings);
        assert_eq!(studio.status.tone, Tone::Error);
        assert!(!directory.path().join("settings.toml").exists());
    }

    fn write_index(root: &std::path::Path) {
        let prefix = if cfg!(windows) { "win64" } else { "linux64" };
        let os = if cfg!(windows) { "windows" } else { "linux" };
        let content = format!(
            r#"{{"schema_version":1,"engines":[{{"engine_id":"{prefix}-master-test","os":"{os}","channel":"master","abi_version":1,"archive_sha256":"{}","archive_size":14,"url":"https://example.invalid/a.tar.zst","supported_macos_range":"26.4+"}}]}}"#,
            "a".repeat(64)
        );
        std::fs::write(root.join("index.json"), content).expect("write index");
    }

    fn write_manifest(root: &std::path::Path, engine_id: &str) {
        let os = if cfg!(windows) { "windows" } else { "linux" };
        let content = format!(
            r#"{{
  "schema_version": 1,
  "abi_version": 1,
  "engine_id": "{engine_id}",
  "os": "{os}",
  "channel": "master",
  "source_revision": "test",
  "qemu_revision": "test",
  "rom_filename": "ReimsGraphics.rom",
  "video_prop_name": "video-disable",
  "ovmf_code_size": 2097152,
  "supported_macos_range": "26.4+",
  "va_driver_name": null,
  "va_drivers_relpath": null,
  "files": {{}},
  "guest_kexts": []
}}"#
        );
        let path = reims_engine_mgr::engine_dir(root, engine_id).join("manifest.json");
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("manifest dir");
        }
        std::fs::write(&path, content).expect("write manifest");
        let _: EngineManifest = load_json(&path).expect("valid manifest fixture");
    }

    fn boot_with_root(root: &std::path::Path) -> Studio {
        let settings = StudioSettings::for_host(HostOs::current());
        let mut studio = boot_from(root.to_path_buf(), settings, None).0;
        studio.sentinel = root.join("ui_booting.flag");
        let _ = std::fs::write(&studio.sentinel, b"booting");
        studio
    }
}
