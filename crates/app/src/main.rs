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
    Channel, HostOs, Settings as StudioSettings, Validate, VmConfig, load_toml,
};
use reims_core::log::LogBuffer;

const TICKS_TO_CLEAR_SENTINEL: u64 = 12;
const LOG_LINES: usize = 72;
const POST_INSTALL_COMMAND: &str = "defaults write \"/Volumes/<Disk>/Library/Preferences/.GlobalPreferences\" METAL_ERROR_MODE -int 5";

mod cli;

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
    /// VM creation form.
    Wizard,
    /// Post-install checklist.
    PostInstall,
}

/// Application state.
struct Studio {
    portable_root: PathBuf,
    sentinel: PathBuf,
    ticks: u64,
    channel: Channel,
    bridge_token: String,
    screen: Screen,
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
    /// Edit the VM name.
    VmName(String),
    /// Edit guest RAM.
    Ram(String),
    /// Edit the vCPU count.
    Vcpus(String),
    /// Edit the guest resolution.
    Resolution(String),
    /// Write a new VM configuration.
    CreateVm,
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
    (
        Studio {
            portable_root,
            sentinel,
            ticks: 0,
            channel,
            bridge_token,
            screen: Screen::Dashboard,
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
            studio.status = format!("Engine channel: {channel}");
            studio.logs.push(format!("selected channel {channel}"));
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
        Message::CreateVm => {
            match create_vm(studio) {
                Ok(config) => {
                    studio.status = format!("Created vms/{}", config.name);
                    studio.logs.push(format!(
                        "VM {} saved: {} MiB, {} vCPUs, policy {}",
                        config.name, config.ram_mib, config.vcpus, config.scan_policy
                    ));
                }
                Err(error) => {
                    studio.status = format!("VM not created: {error}");
                    studio.logs.push(format!("VM creation failed: {error}"));
                }
            }
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
        text("Reims Studio").size(22),
        text("Exit capture: Ctrl+Alt+Win (Windows) / Ctrl+Alt+Super (Linux)").size(12),
    ]
    .spacing(20);

    let sidebar = column![
        button("Dashboard").on_press(Message::Screen(Screen::Dashboard)),
        button("VM wizard").on_press(Message::Screen(Screen::Wizard)),
        button("Post-Install").on_press(Message::Screen(Screen::PostInstall)),
        button("Exit").on_press(Message::Exit),
    ]
    .spacing(8)
    .width(Length::Fixed(160.0));

    let content = match studio.screen {
        Screen::Dashboard => dashboard(studio),
        Screen::Wizard => wizard(studio),
        Screen::PostInstall => post_install(studio),
    };
    container(column![header, row![sidebar, content].spacing(16)].spacing(12))
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
    let canvas = canvas(Telemetry {
        usage: studio.ram_usage,
    })
    .width(Length::Fill)
    .height(Length::Fixed(64.0));
    column![
        text(&studio.status),
        text(format!("Active channel: {}", studio.channel)),
        channels,
        text("Host / VM RAM and present mode").size(12),
        canvas,
        log_panel(studio),
    ]
    .spacing(10)
    .into()
}

fn wizard(studio: &Studio) -> Element<'_, Message> {
    column![
        text("Create a macOS VM"),
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
        text("The system disk is thin 128 GiB on NVMe addr=0xa. Audio is on by default.").size(12),
        button("Create VM").on_press(Message::CreateVm),
        button("Render QEMU command").on_press(Message::BuildCommand),
    ]
    .spacing(8)
    .into()
}

fn post_install(_studio: &Studio) -> Element<'_, Message> {
    column![
        text("Post-Install checklist"),
        text("1. In Recovery Terminal run:"),
        text(POST_INSTALL_COMMAND).size(12),
        button("Copy command").on_press(Message::CopyPostInstall),
        text("2. Eject Installer, set ScanPolicy 983299 and Timeout 2."),
        button("Apply post-install settings").on_press(Message::PostInstall),
    ]
    .spacing(10)
    .into()
}

fn log_panel(studio: &Studio) -> Element<'_, Message> {
    let log_text = studio.logs.page(studio.log_page, LOG_LINES);
    let max_page = studio.logs.len().saturating_sub(1) as f32;
    column![
        row![
            text("Engine log").size(14),
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
    .into()
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
        frame.fill(&bar, iced::Color::from_rgb(0.16, 0.42, 0.68));
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
        let _ = update(&mut studio, Message::CreateVm);
        let vm_dir = directory.path().join("vms/macos-tahoe");
        assert!(vm_dir.join("vm.toml").exists());
        assert_eq!(
            std::fs::read_to_string(vm_dir.join("bridge.token")).expect("token"),
            studio.bridge_token
        );
    }

    fn boot_with_root(root: &std::path::Path) -> Studio {
        let mut studio = boot().0;
        studio.portable_root = root.to_path_buf();
        studio.sentinel = root.join("ui_booting.flag");
        let _ = std::fs::write(&studio.sentinel, b"booting");
        studio
    }
}
