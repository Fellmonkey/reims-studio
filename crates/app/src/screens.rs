//! All rendered screens of the launcher.

use iced::widget::{button, canvas, checkbox, column, container, row, slider, text, text_input};
use iced::{Background, Element, Length, Theme};

use reims_core::contracts::{Channel, EngineRelease};
use reims_platform::preflight::Level;

use crate::can_continue;
use crate::catalog;
use crate::selected;
use crate::theme::{self, Tone};
use crate::warning_signature;
use crate::widgets::{Telemetry, field};
use crate::{CatalogState, LOG_LINES, Message, POST_INSTALL_COMMAND, SetupStep, Stage, Studio};

pub(crate) fn view(studio: &Studio) -> Element<'_, Message> {
    match studio.stage {
        Stage::Setup(step) => setup_view(studio, step),
        Stage::Dashboard => shell(studio, dashboard(studio), true),
        Stage::Settings => shell(studio, settings(studio), true),
    }
}

/// Shared frame for the post-setup screens.
fn shell<'a>(
    studio: &'a Studio,
    content: Element<'a, Message>,
    show_settings: bool,
) -> Element<'a, Message> {
    let mut nav_items = vec![theme::nav(
        "Dashboard",
        studio.stage == Stage::Dashboard,
        Message::Stage(Stage::Dashboard),
    )];
    if show_settings {
        nav_items.push(theme::nav(
            "Settings",
            studio.stage == Stage::Settings,
            Message::Stage(Stage::Settings),
        ));
    }
    nav_items.push(theme::nav("Rerun setup", false, Message::RerunSetup));
    nav_items.push(button("Exit").on_press(Message::Exit).into());
    let sidebar = column(nav_items).spacing(6).width(Length::Fixed(170.0));
    container(
        column![
            column![
                text("Reims Studio").size(22.0),
                theme::muted("Portable macOS virtualization"),
            ]
            .spacing(2),
            theme::divider(),
            row![sidebar, content].spacing(theme::GAP),
            feedback_line(studio),
        ]
        .spacing(theme::GAP),
    )
    .padding(theme::PAD)
    .style(page_style)
    .into()
}

fn page_style(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(theme::PAGE)),
        text_color: Some(theme::TEXT),
        ..container::Style::default()
    }
}

/// Status notice plus a persistent badge for the last completed action.
fn feedback_line(studio: &Studio) -> Element<'_, Message> {
    let tint = match studio.flash_started {
        Some(started) => {
            let progress = (started.elapsed().as_millis() as f32 / 200.0).clamp(0.0, 1.0);
            0.16 + 0.20 * (1.0 - (1.0 - progress).powi(5))
        }
        None => 1.0,
    };
    let mut items: Vec<Element<'_, Message>> = vec![theme::notice_alpha(&studio.status, tint)];
    if let Some(action) = &studio.last_action {
        let tone = if action.ok {
            Tone::Success
        } else {
            Tone::Error
        };
        let verb = if action.ok { "done in" } else { "failed in" };
        items.push(theme::badge(
            format!(
                "{} В· {} {} ms",
                action.kind.label(),
                verb,
                action.elapsed_ms
            ),
            tone,
        ));
    }
    row(items).spacing(10).into()
}

/// Labels and enabled state of the setup navigation bar.
pub(crate) fn nav_state(studio: &Studio, step: SetupStep) -> Vec<(&'static str, bool)> {
    let mut items = Vec::new();
    if step.previous().is_some() {
        items.push(("Back", true));
    }
    if step == SetupStep::Summary {
        items.push(("Open dashboard", true));
        items.push(("Settings", true));
    } else {
        let label = match step {
            SetupStep::Welcome => "Run checks",
            SetupStep::Doctor => "Continue",
            SetupStep::Engine => "Continue",
            SetupStep::MacOs => "Continue",
            SetupStep::Vm => "Create VM and continue",
            SetupStep::Install => "Review summary",
            SetupStep::Summary => unreachable!(),
        };
        items.push((label, can_continue(studio, step).0));
    }
    items
}

/// Navigation bar pinned above the content so a long changelog cannot hide it.
fn setup_nav(studio: &Studio, step: SetupStep) -> Element<'_, Message> {
    let mut buttons: Vec<Element<'_, Message>> = Vec::new();
    for (label, enabled) in nav_state(studio, step) {
        match label {
            "Back" => buttons.push(theme::secondary(label, Message::SetupBack)),
            "Open dashboard" => buttons.push(theme::primary(label, Message::FinishSetup)),
            "Settings" => buttons.push(theme::secondary(label, Message::Stage(Stage::Settings))),
            _ if enabled => buttons.push(theme::primary(label, Message::SetupNext)),
            _ => buttons.push(theme::primary_enabled(label, None)),
        }
    }
    let (_, reason) = can_continue(studio, step);
    let mut items: Vec<Element<'_, Message>> = vec![row(buttons).spacing(10).into()];
    if step != SetupStep::Summary && !reason.is_empty() {
        items.push(theme::muted(reason));
    }
    column(items).spacing(6).into()
}

/// Distraction-free setup flow. No dashboard navigation until setup is done.
fn setup_view(studio: &Studio, step: SetupStep) -> Element<'_, Message> {
    let step_number = step.index() + 1;
    let header = column![
        text("Set up Reims Studio").size(22.0),
        theme::muted(format!(
            "Step {step_number}/7 В· {}",
            SetupStep::TITLES[step.index()]
        )),
        theme::progress(step_number, SetupStep::ALL.len()),
    ]
    .spacing(8);
    let body = match step {
        SetupStep::Welcome => welcome(),
        SetupStep::Doctor => doctor(studio),
        SetupStep::Engine => engine_list(studio),
        SetupStep::MacOs => macos(studio),
        SetupStep::Vm => vm_settings(studio),
        SetupStep::Install => install(studio),
        SetupStep::Summary => summary(studio),
    };
    container(
        column![
            header,
            theme::divider(),
            setup_nav(studio, step),
            body,
            theme::divider(),
            feedback_line(studio),
        ]
        .spacing(theme::GAP),
    )
    .padding(theme::PAD)
    .style(page_style)
    .into()
}

fn welcome() -> Element<'static, Message> {
    theme::card(
        "Welcome",
        column![
            theme::body("This wizard sets up a portable macOS VM in four short stages."),
            theme::body("1 В· Check this PC for blockers."),
            theme::body("2 В· Pick an engine build from the published release."),
            theme::body("3 В· Choose macOS and size the VM."),
            theme::body("4 В· Follow the install checklist and review the result."),
            theme::muted("Nothing is downloaded until you choose an engine."),
        ]
        .spacing(10)
        .into(),
    )
}

fn doctor(studio: &Studio) -> Element<'_, Message> {
    let body: Element<'_, Message> = match &studio.preflight {
        None => column![
            theme::body("Checks have not run yet."),
            theme::muted("Press Run checks on the previous step to inspect this host."),
        ]
        .spacing(8)
        .into(),
        Some(report) => {
            let blockers = report
                .findings
                .iter()
                .filter(|finding| finding.level == Level::Blocker)
                .count();
            let warnings = report
                .findings
                .iter()
                .filter(|finding| finding.level == Level::Warning)
                .count();
            let mut rows: Vec<Element<'_, Message>> = vec![
                row![
                    theme::badge(format!("{blockers} blockers"), Tone::Error),
                    theme::badge(format!("{warnings} warnings"), Tone::Warning),
                    theme::badge(
                        format!(
                            "{} passed",
                            report
                                .findings
                                .iter()
                                .filter(|finding| finding.level == Level::Ok)
                                .count()
                        ),
                        Tone::Success,
                    ),
                ]
                .spacing(8)
                .into(),
            ];
            for finding in &report.findings {
                let tone = match finding.level {
                    Level::Ok => Tone::Success,
                    Level::Warning => Tone::Warning,
                    Level::Blocker => Tone::Error,
                };
                rows.push(
                    row![
                        theme::badge(finding.code, tone),
                        theme::body(finding.message.clone()),
                    ]
                    .spacing(10)
                    .into(),
                );
            }
            if let Some(signature) = warning_signature(report) {
                let acknowledged = studio.warning_signature.as_deref() == Some(signature.as_str());
                rows.push(
                    checkbox(acknowledged)
                        .label("I understand the advisory findings")
                        .on_toggle(Message::AckWarnings)
                        .into(),
                );
                if !acknowledged {
                    rows.push(theme::muted(
                        "Advisory findings must be acknowledged before continuing.",
                    ));
                }
            }
            rows.push(theme::muted(
                "Some host facts are best-effort; blockers are always enforced.",
            ));
            theme::scroll_box(column(rows).spacing(8), 300.0)
        }
    };
    theme::card_headed(
        "Diagnostics",
        "Host checks",
        "Best effort",
        column![body, theme::secondary("Re-run checks", Message::RunDoctor)]
            .spacing(12)
            .into(),
    )
}

fn engine_list(studio: &Studio) -> Element<'_, Message> {
    if studio.engines.is_empty() {
        return theme::card(
            "Choose an engine",
            column![
                theme::body("No engine builds are available yet."),
                theme::muted(
                    "Download the published index from GitHub, then pick a build for this host."
                ),
                theme::primary("Download from GitHub", Message::RefreshCatalog),
            ]
            .spacing(10)
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
    let detail: Element<'_, Message> = match selected(studio) {
        Some(engine) => {
            let installed =
                studio.installed_engine_id.as_deref() == Some(engine.engine_id.as_str());
            let install_control: Element<'_, Message> = if studio.installing {
                column![
                    theme::muted(format!(
                        "Downloading {}/{} MiB вЂ” verifying and unpacking when complete",
                        studio.install_progress / (1024 * 1024),
                        studio.install_total / (1024 * 1024),
                    )),
                    theme::bytes(studio.install_progress, studio.install_total),
                ]
                .spacing(6)
                .into()
            } else if installed {
                theme::badge("Installed", Tone::Success)
            } else {
                theme::primary("Download and install", Message::InstallEngine)
            };
            column![
                text(&engine.engine_id).size(15.0),
                theme::muted(format!(
                    "ABI {} В· macOS {} В· {}",
                    engine.abi_version,
                    engine.supported_macos_range,
                    catalog::size(engine.archive_size)
                )),
                theme::scroll_box(
                    text(changelog(engine)).size(12.0).width(Length::Fill),
                    170.0
                ),
                row![
                    install_control,
                    theme::secondary("Copy download URL", Message::CopyEngineUrl),
                    theme::secondary("Open release page", Message::OpenRelease),
                ]
                .spacing(10),
            ]
            .spacing(8)
            .into()
        }
        None => theme::muted("Select a build to see its details and changelog."),
    };
    let source = match studio.catalog_state {
        CatalogState::Remote => "Source: GitHub release",
        CatalogState::Cached => "Source: local cache",
        CatalogState::Empty => "Source: none",
    };
    theme::card_headed(
        "Engine selection",
        "Choose an engine",
        "GitHub release",
        column![
            theme::body("Pick one build. It determines which macOS versions you can install."),
            theme::scroll_box(column(rows).spacing(6), 110.0),
            detail,
            row![
                theme::secondary("Refresh from GitHub", Message::RefreshCatalog),
                theme::muted(source),
            ]
            .spacing(10),
        ]
        .spacing(12)
        .into(),
    )
}

/// Returns a short changelog for an engine build.
fn changelog(engine: &EngineRelease) -> String {
    match engine.changelog.as_deref() {
        Some(log) if !log.trim().is_empty() => {
            let clipped: String = log.chars().take(1200).collect();
            format!("Recent engine changes:\n{clipped}")
        }
        _ => String::from("No changelog was published with this engine build yet."),
    }
}

fn macos(studio: &Studio) -> Element<'_, Message> {
    let compatible = studio
        .engines
        .iter()
        .filter(|engine| catalog::supports_macos(engine, studio.macos_version.trim()))
        .count();
    theme::card(
        "macOS version",
        column![
            theme::body("Which macOS version will you install?"),
            row![
                text("Version").width(Length::Fixed(120.0)),
                text_input("26.4", &studio.macos_version)
                    .on_input(Message::MacVersion)
                    .size(14.0),
            ],
            theme::muted(format!(
                "{compatible} compatible build(s) for this version."
            )),
            theme::muted("A build must advertise support for the version you select."),
        ]
        .spacing(10)
        .into(),
    )
}

fn vm_settings(studio: &Studio) -> Element<'_, Message> {
    theme::card(
        "Virtual machine",
        column![
            theme::body("Defaults work for most hosts. You can change them later in Settings."),
            field("Name", &studio.vm_name, "vm name", Message::VmName),
            field("RAM MiB", &studio.ram, "8192", Message::Ram),
            field("vCPUs", &studio.vcpus, "4", Message::Vcpus),
            field(
                "Resolution",
                &studio.resolution,
                "1920x1080",
                Message::Resolution
            ),
            theme::muted("System disk: thin 128 GiB on NVMe addr=0xa В· audio on."),
            theme::secondary("Render QEMU command", Message::BuildCommand),
        ]
        .spacing(10)
        .into(),
    )
}

fn install(studio: &Studio) -> Element<'_, Message> {
    let applied = if studio.post_install_applied {
        theme::badge("Applied", Tone::Success)
    } else {
        theme::badge("Pending", Tone::Warning)
    };
    theme::card(
        "Install macOS",
        column![
            theme::body("1 В· Boot the VM with the macOS installer attached."),
            theme::body("2 В· In Recovery Terminal run:"),
            text(POST_INSTALL_COMMAND).size(12.0),
            theme::secondary("Copy command", Message::CopyPostInstall),
            theme::body("3 В· Eject the installer and apply the OpenCore settings."),
            row![
                theme::secondary("Apply post-install settings", Message::PostInstall),
                applied
            ]
            .spacing(10),
            theme::muted("ScanPolicy 983299 В· Timeout 2 В· ShowPicker true"),
        ]
        .spacing(10)
        .into(),
    )
}

fn summary(studio: &Studio) -> Element<'_, Message> {
    let engine = selected(studio).map_or_else(
        || String::from("none"),
        |engine| {
            format!(
                "{} ({}, ABI {})",
                engine.engine_id, engine.channel, engine.abi_version
            )
        },
    );
    let verdict = studio.preflight.as_ref().map_or_else(
        || String::from("not run"),
        |report| {
            format!(
                "{} blockers В· {} warnings В· {} passed",
                report
                    .findings
                    .iter()
                    .filter(|f| f.level == Level::Blocker)
                    .count(),
                report
                    .findings
                    .iter()
                    .filter(|f| f.level == Level::Warning)
                    .count(),
                report
                    .findings
                    .iter()
                    .filter(|f| f.level == Level::Ok)
                    .count(),
            )
        },
    );
    let mut rows: Vec<Element<'_, Message>> = vec![
        row![theme::muted("Engine"), theme::body(engine)]
            .spacing(12)
            .into(),
        row![
            theme::muted("macOS"),
            theme::body(studio.macos_version.clone())
        ]
        .spacing(12)
        .into(),
        row![
            theme::muted("VM"),
            theme::body(format!(
                "{} В· {} MiB В· {} vCPUs В· {}x{}",
                studio.vm_name,
                studio.ram,
                studio.vcpus,
                studio.resolution.split('x').next().unwrap_or("1920"),
                studio.resolution.split('x').nth(1).unwrap_or("1080"),
            ))
        ]
        .spacing(12)
        .into(),
        row![theme::muted("Host checks"), theme::body(verdict)]
            .spacing(12)
            .into(),
        row![
            theme::muted("Ports"),
            theme::body(format!(
                "bridge {} В· QMP {}",
                studio.bridge_port, studio.qmp_port
            ))
        ]
        .spacing(12)
        .into(),
        row![
            theme::muted("Post-install"),
            theme::body(if studio.post_install_applied {
                "applied"
            } else {
                "not applied"
            })
        ]
        .spacing(12)
        .into(),
        row![
            theme::muted("Portable folder"),
            theme::body(studio.portable_root.display().to_string())
        ]
        .spacing(12)
        .into(),
    ];
    if !studio
        .preflight
        .as_ref()
        .is_some_and(|report| report.has_blocker())
    {
        rows.insert(0, theme::badge("Setup complete", Tone::Success));
    }
    theme::card("Everything is ready", column(rows).spacing(8).into())
}

fn dashboard(studio: &Studio) -> Element<'_, Message> {
    let engine =
        selected(studio).map_or_else(|| String::from("No engine selected."), catalog::line);
    let canvas = canvas(Telemetry {
        usage: studio.ram_usage,
    })
    .width(Length::Fill)
    .height(Length::Fixed(64.0));
    column![
        theme::card_headed(
            "Active setup",
            "Current setup",
            "Portable",
            column![
                theme::badge("Ready", Tone::Success),
                theme::body(engine),
                theme::muted(format!("Channel: {}", studio.channel)),
                theme::secondary("Change engine", Message::Stage(Stage::Settings)),
            ]
            .spacing(10)
            .into(),
        ),
        theme::card("Host / VM RAM and present mode", canvas.into()),
        log_panel(studio),
    ]
    .spacing(theme::GAP)
    .into()
}

fn settings(studio: &Studio) -> Element<'_, Message> {
    const CHANNELS: [&str; 3] = ["master", "development", "patched"];
    let index = match studio.channel {
        Channel::Master => 0,
        Channel::Development => 1,
        Channel::Patched => 2,
    };
    let channels = theme::segmented(&CHANNELS, index, |index| {
        Message::Channel(match index {
            0 => Channel::Master,
            1 => Channel::Development,
            _ => Channel::Patched,
        })
    });
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
    column![
        theme::card(
            "Engine",
            column![
                theme::muted("Choose a different build, then save settings."),
                channels,
                column(rows).spacing(6),
                row![
                    theme::secondary("Refresh from GitHub", Message::RefreshCatalog),
                    theme::secondary("Open release page", Message::OpenRelease),
                ]
                .spacing(10),
            ]
            .spacing(10)
            .into(),
        ),
        theme::card(
            "Ports and saves",
            column![
                field(
                    "Bridge port",
                    &studio.bridge_port,
                    "8080",
                    Message::BridgePort
                ),
                field("QMP port", &studio.qmp_port, "4444", Message::QmpPort),
                theme::primary("Save settings", Message::SaveSettings),
                theme::secondary("Re-run checks", Message::RunDoctor),
                theme::secondary("Rerun setup", Message::RerunSetup),
            ]
            .spacing(10)
            .into(),
        ),
    ]
    .spacing(theme::GAP)
    .into()
}

fn log_panel(studio: &Studio) -> Element<'_, Message> {
    let log_text = studio.logs.page(studio.log_page, LOG_LINES);
    let max_page = studio.logs.len().saturating_sub(1) as f32;
    theme::card(
        "Engine log",
        column![
            row![
                theme::secondary(
                    if studio.paused { "Resume" } else { "Pause" },
                    Message::Pause
                ),
                theme::secondary("Copy page", Message::CopyLogs),
            ]
            .spacing(8),
            theme::scroll_box(text(log_text).size(12.0).width(Length::Fill), 120.0),
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
