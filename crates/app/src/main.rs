//! Reims Studio launcher GUI on iced 0.14.
//!
//! The window starts on the software `tiny-skia` backend. A `ui_booting.flag`
//! sentinel is removed three seconds after boot; if the UI dies before that,
//! the flag remains as a crash marker.

// SAFETY scope: one environment variable is set on the main thread before any
// window or runtime thread exists; see `main`.
#![allow(unsafe_code, reason = "select the iced backend before window init")]

use std::path::PathBuf;
use std::time::Duration;

use iced::widget::{button, canvas, column, container, row, slider, text, text_input};
use iced::{Element, Length, Rectangle, Size, Subscription, Task, Theme};

use reims_core::contracts::{
    Channel, EngineRelease, HostOs, Settings as StudioSettings, Validate, VmConfig, load_toml,
};
use reims_core::log::LogBuffer;

const TICKS_TO_CLEAR_SENTINEL: u64 = 12;
const LOG_LINES: usize = 72;
const POST_INSTALL_COMMAND: &str = "defaults write \"/Volumes/<Disk>/Library/Preferences/.GlobalPreferences\" METAL_ERROR_MODE -int 5";

mod catalog;
mod cli;
mod theme;

fn main() -> iced::Result {
    let mut arguments = std::env::args().skip(1);
    match arguments.next().as_deref() {
        Some("--check") => return headless("--check", false),
        Some("--e2e") => return headless("--e2e", true),
        Some("--help") | Some("-h") => {
            println!("Usage: reims-studio [--check | --e2e | --help]");
            println!("  --check  verify that a built engine is visible to the launcher");
            println!("  --e2e    also verify the engine archive SHA256");
            return Ok(());
        }
        Some(other) => {
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
    iced::application(boot, update, view)
        .title(|_: &Studio| String::from("Reims Studio"))
        .window(window_settings())
        .subscription(subscription)
        .run()
}

fn window_settings() -> iced::window::Settings {
    iced::window::Settings {
        size: Size::new(1120.0, 760.0),
        ..iced::window::Settings::default()
    }
}

/// Screen shown in the main area.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Screen {
    /// Engine channels, VM status and diagnostics.
    Dashboard,
    /// Staged VM creation flow.
    Wizard,
    /// Published engines with changelogs.
    Releases,
    /// Post-install checklist.
    PostInstall,
}

/// Step of the staged VM creation flow.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WizardStep {
    /// Pick one of the available engine builds.
    Engine,
    /// Enter the VM configuration.
    Configure,
    /// Follow the macOS installation checklist.
    Install,
    /// Review what was created.
    Done,
}

impl WizardStep {
    /// Labels rendered in the progress indicator.
    const LABELS: [&'static str; 4] = ["Engine", "Configure", "Install", "Done"];

    fn index(self) -> usize {
        match self {
            Self::Engine => 0,
            Self::Configure => 1,
            Self::Install => 2,
            Self::Done => 3,
        }
    }

    fn previous(self) -> Option<Self> {
        match self {
            Self::Engine => None,
            Self::Configure => Some(Self::Engine),
            Self::Install => Some(Self::Configure),
            Self::Done => Some(Self::Install),
        }
    }
}

/// Application state.
struct Studio {
    portable_root: PathBuf,
    sentinel: PathBuf,
    ticks: u64,
    channel: Channel,
    bridge_token: String,
    screen: Screen,
    engines: Vec<EngineRelease>,
    selected_engine: Option<usize>,
    wizard_step: WizardStep,
    vm_name: String,
    ram: String,
    vcpus: String,
    resolution: String,
    status: String,
    logs: LogBuffer,
    log_page: usize,
    paused: bool,
    ram_usage: f32,
}

/// Messages produced by widgets and subscriptions.
#[derive(Debug, Clone)]
enum Message {
    /// Periodic timer tick.
    Tick,
    /// Change the visible screen.
    Screen(Screen),
    /// Select an engine channel.
    Channel(Channel),
    /// Pick an engine from the catalog.
    SelectEngine(usize),
    /// Reload the engine catalog from disk.
    RefreshCatalog,
    /// Copy the selected engine's download URL.
    CopyEngineUrl,
    /// Advance in the staged VM flow.
    WizardNext,
    /// Go back one step in the staged VM flow.
    WizardBack,
    /// Edit the VM name.
    VmName(String),
    /// Edit guest RAM.
    Ram(String),
    /// Edit the vCPU count.
    Vcpus(String),
    /// Edit the guest resolution.
    Resolution(String),
    /// Switch OpenCore to post-install settings.
    PostInstall,
    /// Copy the Tahoe command to the clipboard.
    CopyPostInstall,
    /// Copy the visible log page to the clipboard.
    CopyLogs,
    /// Move the log page slider.
    LogPage(f32),
    /// Pause or resume log updates.
    Pause,
    /// Render the QEMU command for diagnostics.
    BuildCommand,
    /// Close the window.
    Exit,
}

fn boot() -> (Studio, Task<Message>) {
    let portable_root = portable_root();
    let sentinel = portable_root.join("ui_booting.flag");
    if let Some(parent) = sentinel.parent()
        && std::fs::create_dir_all(parent).is_ok()
    {
        let _ = std::fs::write(&sentinel, b"booting");
    }
    let channel = load_toml::<StudioSettings>(&portable_root.join("settings.toml"))
        .map(|settings| settings.channel)
        .unwrap_or_else(|_| Channel::for_host(HostOs::current()));
    let bridge_token = format!("{:032x}", rand::random::<u128>());
    let mut logs = LogBuffer::new(256);
    logs.push("Reims Studio started on tiny-skia");
    logs.push(format!("engine channel: {channel}"));
    let engines = match catalog::load(&portable_root.join("index.json")) {
        list @ Ok(_) => {
            logs.push(format!(
                "{} engine build(s) available for {}",
                list.as_ref().map_or(0, Vec::len),
                HostOs::current()
            ));
            list.unwrap_or_default()
        }
        Err(error) => {
            logs.push(format!("engine catalog unavailable: {error}"));
            Vec::new()
        }
    };
    let selected_engine = engines.iter().position(|engine| engine.channel == channel);
    (
        Studio {
            portable_root,
            sentinel,
            ticks: 0,
            channel,
            bridge_token,
            screen: Screen::Dashboard,
            engines,
            selected_engine,
            wizard_step: WizardStep::Engine,
            vm_name: String::from("macos-tahoe"),
            ram: String::from("8192"),
            vcpus: String::from("4"),
            resolution: String::from("1920x1080"),
            status: String::from("Ready."),
            logs,
            log_page: 0,
            paused: false,
            ram_usage: 0.18,
        },
        Task::none(),
    )
}

fn reload_catalog(studio: &mut Studio) {
    match catalog::load(&studio.portable_root.join("index.json")) {
        Ok(engines) => {
            studio
                .logs
                .push(format!("catalog refreshed: {} engine(s)", engines.len()));
            studio.selected_engine = engines.iter().position(|e| e.channel == studio.channel);
            studio.engines = engines;
            studio.status = String::from("Engine catalog refreshed.");
        }
        Err(error) => {
            studio.status = format!("Catalog refresh failed: {error}");
            studio.logs.push(format!("catalog refresh failed: {error}"));
        }
    }
}

fn update(studio: &mut Studio, message: Message) -> Task<Message> {
    match message {
        Message::Tick => {
            studio.ticks += 1;
            if studio.ticks == TICKS_TO_CLEAR_SENTINEL {
                // The sentinel is only evidence while the UI has not finished
                // starting; a removal failure does not block the UI.
                let _ = std::fs::remove_file(&studio.sentinel);
                studio.logs.push("ui_booting.flag cleared after 3s");
            }
            studio.ram_usage = 0.18 + f32::from((studio.ticks % 8) as u8) / 100.0;
            Task::none()
        }
        Message::Screen(screen) => {
            studio.screen = screen;
            Task::none()
        }
        Message::Channel(channel) => {
            studio.channel = channel;
            studio.selected_engine = studio
                .engines
                .iter()
                .position(|engine| engine.channel == channel);
            studio.status = format!("Engine channel: {channel}");
            studio.logs.push(format!("selected channel {channel}"));
            Task::none()
        }
        Message::SelectEngine(index) => {
            if let Some(engine) = studio.engines.get(index) {
                studio.selected_engine = Some(index);
                studio.status = format!("Selected engine {}", engine.engine_id);
                studio
                    .logs
                    .push(format!("selected engine {}", engine.engine_id));
            }
            Task::none()
        }
        Message::RefreshCatalog => {
            reload_catalog(studio);
            Task::none()
        }
        Message::CopyEngineUrl => {
            if let Some(engine) = studio.selected_engine.and_then(|i| studio.engines.get(i)) {
                iced::clipboard::write(engine.url.clone())
            } else {
                studio.status = String::from("Select an engine before copying its URL.");
                Task::none()
            }
        }
        Message::WizardNext => {
            let next = match studio.wizard_step {
                WizardStep::Engine => {
                    if studio.selected_engine.is_none() {
                        studio.status = String::from("Choose an engine to continue.");
                        return Task::none();
                    }
                    WizardStep::Configure
                }
                WizardStep::Configure => match create_vm(studio) {
                    Ok(config) => {
                        studio.status = format!("Created vms/{}", config.name);
                        studio.logs.push(format!(
                            "VM {} saved: {} MiB, {} vCPUs, policy {}",
                            config.name, config.ram_mib, config.vcpus, config.scan_policy
                        ));
                        WizardStep::Install
                    }
                    Err(error) => {
                        studio.status = format!("VM not created: {error}");
                        studio.logs.push(format!("VM creation failed: {error}"));
                        return Task::none();
                    }
                },
                WizardStep::Install => WizardStep::Done,
                WizardStep::Done => return Task::none(),
            };
            studio.wizard_step = next;
            Task::none()
        }
        Message::WizardBack => {
            if let Some(previous) = studio.wizard_step.previous() {
                studio.wizard_step = previous;
            }
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
        Message::PostInstall => {
            match apply_post_install(studio) {
                Ok(()) => {
                    studio.status = String::from("Post-install settings applied.");
                    studio
                        .logs
                        .push("ScanPolicy 983299, Timeout 2, ShowPicker true");
                }
                Err(error) => studio.status = format!("Post-install failed: {error}"),
            }
            Task::none()
        }
        Message::CopyPostInstall => iced::clipboard::write(String::from(POST_INSTALL_COMMAND)),
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
        Message::BuildCommand => {
            match render_command(studio) {
                Ok(command) => {
                    studio.status = String::from("QEMU command rendered; see logs.");
                    studio.logs.push(command);
                }
                Err(error) => studio.status = format!("Command failed: {error}"),
            }
            Task::none()
        }
        Message::Exit => iced::exit(),
    }
}

fn subscription(_studio: &Studio) -> Subscription<Message> {
    iced::time::every(Duration::from_millis(250)).map(|_| Message::Tick)
}

fn view(studio: &Studio) -> Element<'_, Message> {
    let header = row![
        theme::section_title("Reims Studio"),
        theme::muted("Portable macOS virtualization · Ctrl+Alt+Win/Super to release the cursor"),
    ]
    .spacing(20);

    let sidebar = column![
        theme::nav(
            "Dashboard",
            studio.screen == Screen::Dashboard,
            Message::Screen(Screen::Dashboard)
        ),
        theme::nav(
            "Create VM",
            studio.screen == Screen::Wizard,
            Message::Screen(Screen::Wizard)
        ),
        theme::nav(
            "Engines & releases",
            studio.screen == Screen::Releases,
            Message::Screen(Screen::Releases)
        ),
        theme::nav(
            "Post-install",
            studio.screen == Screen::PostInstall,
            Message::Screen(Screen::PostInstall)
        ),
        button("Exit").on_press(Message::Exit),
    ]
    .spacing(6)
    .width(Length::Fixed(170.0));

    let content = match studio.screen {
        Screen::Dashboard => dashboard(studio),
        Screen::Wizard => wizard(studio),
        Screen::Releases => engine_list(studio, "Published engines and changelogs"),
        Screen::PostInstall => post_install(studio),
    };
    container(
        column![
            header,
            row![sidebar, content].spacing(16),
            theme::status(&studio.status),
        ]
        .spacing(12),
    )
    .padding(16)
    .into()
}

fn dashboard(studio: &Studio) -> Element<'_, Message> {
    let channels = row![
        button("master").on_press(Message::Channel(Channel::Master)),
        button("development").on_press(Message::Channel(Channel::Development)),
        button("patched").on_press(Message::Channel(Channel::Patched)),
    ]
    .spacing(8);
    let selected = studio
        .selected_engine
        .and_then(|index| studio.engines.get(index));
    let engine_line = selected.map_or_else(
        || String::from("No engine available for this host yet."),
        |engine| format!("Engine: {}", catalog::line(engine)),
    );
    let canvas = canvas(Telemetry {
        usage: studio.ram_usage,
    })
    .width(Length::Fill)
    .height(Length::Fixed(64.0));
    column![
        theme::card(
            "Engine",
            column![
                text(engine_line),
                theme::muted(format!("Active channel: {}", studio.channel)),
                channels,
                button("Refresh catalog").on_press(Message::RefreshCatalog),
            ]
            .spacing(8)
            .into(),
        ),
        theme::card("Host / VM RAM and present mode", canvas.into()),
        log_panel(studio),
    ]
    .spacing(theme::GAP)
    .into()
}

fn wizard(studio: &Studio) -> Element<'_, Message> {
    let indicator = theme::steps(&WizardStep::LABELS, studio.wizard_step.index());
    let body = match studio.wizard_step {
        WizardStep::Engine => engine_list(studio, "1 · Choose an engine"),
        WizardStep::Configure => configure(studio),
        WizardStep::Install => install_checklist(studio),
        WizardStep::Done => done(studio),
    };
    let mut controls = Vec::new();
    if studio.wizard_step.previous().is_some() {
        controls.push(button("Back").on_press(Message::WizardBack).into());
    }
    if let Some(label) = match studio.wizard_step {
        WizardStep::Engine => Some("Continue to VM settings"),
        WizardStep::Configure => Some("Create VM and continue"),
        WizardStep::Install => Some("Finish"),
        WizardStep::Done => None,
    } {
        controls.push(theme::primary(label, Message::WizardNext));
    }
    column![indicator, body, row(controls).spacing(8)]
        .spacing(theme::GAP)
        .into()
}

fn engine_list<'a>(studio: &'a Studio, title: &'static str) -> Element<'a, Message> {
    if studio.engines.is_empty() {
        return theme::card(
            title,
            column![
                text("No engine builds found for this host."),
                theme::muted(
                    "Publish an engine build, place index.json in the portable folder, then refresh."
                ),
                button("Refresh catalog").on_press(Message::RefreshCatalog),
            ]
            .spacing(8)
            .into(),
        );
    }
    let rows = studio
        .engines
        .iter()
        .enumerate()
        .map(|(index, engine)| {
            theme::choice(
                catalog::line(engine),
                studio.selected_engine == Some(index),
                Message::SelectEngine(index),
            )
        })
        .collect::<Vec<_>>();
    let selected = studio
        .selected_engine
        .and_then(|index| studio.engines.get(index));
    let detail: Element<'_, Message> = match selected {
        Some(engine) => column![
            text(&engine.engine_id).size(15),
            theme::muted(format!(
                "ABI {} · macOS {} · {}",
                engine.abi_version,
                engine.supported_macos_range,
                catalog::size(engine.archive_size),
            )),
            text(changelog(engine)).size(12),
            button("Copy download URL").on_press(Message::CopyEngineUrl),
        ]
        .spacing(8)
        .into(),
        None => theme::muted("Select a build to see its details and changelog."),
    };
    theme::card(
        title,
        column![
            column(rows).spacing(6),
            detail,
            button("Refresh catalog").on_press(Message::RefreshCatalog),
        ]
        .spacing(12)
        .into(),
    )
}

/// Returns a short, readable changelog for an engine build.
fn changelog(engine: &EngineRelease) -> String {
    match engine.changelog.as_deref() {
        Some(log) if !log.trim().is_empty() => {
            let clipped: String = log.chars().take(1200).collect();
            format!("Recent engine changes:\n{clipped}")
        }
        _ => String::from("No changelog was published with this engine build yet."),
    }
}

fn configure(studio: &Studio) -> Element<'_, Message> {
    theme::card(
        "2 · VM settings",
        column![
            row![
                text("Name").width(Length::Fixed(120.0)),
                text_input("vm name", &studio.vm_name).on_input(Message::VmName)
            ],
            row![
                text("RAM MiB").width(Length::Fixed(120.0)),
                text_input("8192", &studio.ram).on_input(Message::Ram)
            ],
            row![
                text("vCPUs").width(Length::Fixed(120.0)),
                text_input("4", &studio.vcpus).on_input(Message::Vcpus)
            ],
            row![
                text("Resolution").width(Length::Fixed(120.0)),
                text_input("1920x1080", &studio.resolution).on_input(Message::Resolution)
            ],
            theme::muted(
                "The system disk is thin 128 GiB on NVMe addr=0xa. Audio is on by default."
            ),
            button("Render QEMU command").on_press(Message::BuildCommand),
        ]
        .spacing(10)
        .into(),
    )
}

fn install_checklist(_studio: &Studio) -> Element<'_, Message> {
    theme::card(
        "3 · Install macOS",
        column![
            text("1. Attach the macOS installer and boot the VM."),
            text("2. In Recovery Terminal run:"),
            text(POST_INSTALL_COMMAND).size(12),
            button("Copy command").on_press(Message::CopyPostInstall),
            text("3. Eject the installer, then apply the OpenCore settings:"),
            button("Apply post-install settings").on_press(Message::PostInstall),
            theme::muted("ScanPolicy 983299 · Timeout 2 · ShowPicker true"),
        ]
        .spacing(10)
        .into(),
    )
}

fn done(studio: &Studio) -> Element<'_, Message> {
    let engine = studio
        .selected_engine
        .and_then(|index| studio.engines.get(index))
        .map_or_else(|| String::from("none"), |engine| engine.engine_id.clone());
    theme::card(
        "4 · Done",
        column![
            theme::badge("VM created", theme::SUCCESS),
            theme::muted(format!("VM: vms/{}", studio.vm_name)),
            theme::muted(format!("Engine: {engine}")),
            theme::muted(format!("Bridge token: {}", studio.bridge_token)),
            button("Back to dashboard").on_press(Message::Screen(Screen::Dashboard)),
        ]
        .spacing(8)
        .into(),
    )
}

fn post_install(_studio: &Studio) -> Element<'_, Message> {
    theme::card(
        "Post-install checklist",
        column![
            text("1. In Recovery Terminal run:"),
            text(POST_INSTALL_COMMAND).size(12),
            button("Copy command").on_press(Message::CopyPostInstall),
            text("2. Eject Installer, set ScanPolicy 983299 and Timeout 2."),
            button("Apply post-install settings").on_press(Message::PostInstall),
        ]
        .spacing(10)
        .into(),
    )
}

fn log_panel(studio: &Studio) -> Element<'_, Message> {
    let log_text = studio.logs.page(studio.log_page, LOG_LINES);
    let max_page = studio.logs.len().saturating_sub(1) as f32;
    theme::card(
        "Engine log",
        column![
            row![
                button(if studio.paused { "Resume" } else { "Pause" }).on_press(Message::Pause),
                button("Copy page").on_press(Message::CopyLogs),
            ]
            .spacing(8),
            // One unclipped text block: no scrollable and no clip on tiny-skia.
            text(log_text).size(12),
            slider(
                0.0..=max_page.max(0.0),
                studio.log_page as f32,
                Message::LogPage
            ),
        ]
        .spacing(6)
        .into(),
    )
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
        toml::to_string(&config).map_err(|e| e.to_string())?,
    )
    .map_err(|error| error.to_string())?;
    // The bridge token is generated here so the wizard can inject it into the
    // guest LaunchAgent on the next step.
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
    std::fs::write(&path, toml::to_string(&config).map_err(|e| e.to_string())?)
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
        load_toml(&manifest_path).map_err(|error| error.to_string())?;
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

#[derive(Debug)]
struct Telemetry {
    usage: f32,
}

impl canvas::Program<Message> for Telemetry {
    type State = ();

    fn draw(
        &self,
        _state: &(),
        renderer: &iced::Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: iced::mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let mut frame = canvas::Frame::new(renderer, bounds.size());
        let bar = canvas::Path::rectangle(
            iced::Point::new(0.0, 0.0),
            Size::new(bounds.width * self.usage.clamp(0.0, 1.0), bounds.height),
        );
        frame.fill(&bar, theme::ACCENT);
        vec![frame.into_geometry()]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn wizard_writes_vm_and_token() {
        let directory = tempfile::tempdir().expect("temp dir");
        let mut studio = boot_with_root(directory.path());
        studio.wizard_step = WizardStep::Configure;
        let _ = update(&mut studio, Message::WizardNext);
        let vm_dir = directory.path().join("vms/macos-tahoe");
        assert!(vm_dir.join("vm.toml").exists());
        assert_eq!(
            std::fs::read_to_string(vm_dir.join("bridge.token")).expect("token"),
            studio.bridge_token
        );
    }

    #[test]
    fn wizard_steps_advance_and_go_back() {
        let directory = tempfile::tempdir().expect("temp dir");
        write_index(directory.path());
        let mut studio = boot_with_root(directory.path());
        reload_catalog(&mut studio);
        assert_eq!(studio.engines.len(), 1);
        // The active channel may already match the fixture; clear it so the
        // "no selection" branch is exercised on every host.
        studio.selected_engine = None;
        assert_eq!(studio.wizard_step, WizardStep::Engine);
        let _ = update(&mut studio, Message::WizardBack);
        assert_eq!(studio.wizard_step, WizardStep::Engine);
        // Without a selected engine the wizard refuses to advance.
        let _ = update(&mut studio, Message::WizardNext);
        assert_eq!(studio.wizard_step, WizardStep::Engine);
        let _ = update(&mut studio, Message::SelectEngine(0));
        let _ = update(&mut studio, Message::WizardNext);
        assert_eq!(studio.wizard_step, WizardStep::Configure);
        let _ = update(&mut studio, Message::WizardBack);
        assert_eq!(studio.wizard_step, WizardStep::Engine);
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

    fn boot_with_root(root: &std::path::Path) -> Studio {
        let mut studio = boot().0;
        studio.portable_root = root.to_path_buf();
        studio.sentinel = root.join("ui_booting.flag");
        let _ = std::fs::write(&studio.sentinel, b"booting");
        studio
    }
}
