//! Sequential first-run setup screens.

use crate::ui::theme::{self as t, Tone};
use crate::ui::widgets::field;
use crate::{Message, POST_INSTALL_COMMAND, SetupStep, Studio, can_continue};
use crate::{catalog, selected, warning_signature};
use iced::alignment::{Horizontal, Vertical};
use iced::widget::{checkbox, column, container, row, text};
use iced::{Element, Length, Theme};
use reims_platform::preflight::Level;

/// Labels of the pinned navigation bar, used by tests.
#[cfg(test)]
pub(crate) fn nav_labels(step: SetupStep) -> Vec<&'static str> {
    let mut labels = Vec::new();
    if step.previous().is_some() {
        labels.push("Back");
    }
    if step == SetupStep::Summary {
        labels.push("Open dashboard");
        labels.push("Settings");
    } else {
        labels.push(match step {
            SetupStep::Welcome => "Run checks",
            SetupStep::Doctor | SetupStep::Engine | SetupStep::MacOs => "Continue",
            SetupStep::Vm => "Create VM and continue",
            SetupStep::Install => "Review summary",
            SetupStep::Summary => unreachable!(),
        });
    }
    labels
}

/// Renders the current setup step.
pub fn view<'a>(
    studio: &'a Studio,
    theme: &'a Theme,
    step: SetupStep,
) -> Element<'a, Message, Theme> {
    let number = step.index() + 1;
    let header = column![
        text("Set up Reims Studio").size(22.0),
        t::muted(
            theme,
            format!("Step {number}/7 · {}", SetupStep::TITLES[step.index()])
        ),
        t::steps(number, SetupStep::ALL.len()),
    ]
    .spacing(8);

    let body = match step {
        SetupStep::Welcome => welcome(theme),
        SetupStep::Doctor => doctor(studio, theme),
        SetupStep::Engine => engine(studio, theme),
        SetupStep::MacOs => macos(studio, theme),
        SetupStep::Vm => vm(studio, theme),
        SetupStep::Install => install(studio, theme),
        SetupStep::Summary => summary(studio, theme),
    };

    let body = container(body).width(Length::Fill).height(Length::Fill);

    let (allowed, reason) = can_continue(studio, step);
    let mut controls: Vec<Element<'_, Message, Theme>> = Vec::new();
    if step.previous().is_some() {
        controls.push(t::secondary("Back", Message::SetupBack));
    }
    if step == SetupStep::Summary {
        controls.push(t::primary("Open dashboard", Message::FinishSetup));
        controls.push(t::secondary(
            "Settings",
            Message::Stage(crate::Stage::Settings),
        ));
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
        controls.push(t::primary_enabled(
            label,
            allowed.then_some(Message::SetupNext),
        ));
        if !allowed {
            controls.push(t::muted(theme, reason));
        }
    }

    let footer = row![
        row(controls).spacing(10),
        container(t::feedback_line(
            theme,
            &studio.status,
            studio.last_action.as_ref()
        ))
        .width(Length::Fill)
        .align_x(Horizontal::Right),
    ]
    .spacing(16)
    .align_y(Vertical::Bottom);

    column![header, t::divider(), body, t::divider(), footer,]
        .spacing(12)
        .height(Length::Fill)
        .into()
}

fn welcome<'a>(theme: &'a Theme) -> Element<'a, Message, Theme> {
    t::card(
        "Welcome",
        column![
            t::body(
                theme,
                "This wizard sets up a portable macOS VM in four short stages."
            ),
            t::body(theme, "1 · Check this PC for blockers."),
            t::body(
                theme,
                "2 · Pick an engine build from the published release."
            ),
            t::body(theme, "3 · Choose macOS and size the VM."),
            t::body(
                theme,
                "4 · Follow the install checklist and review the result."
            ),
            t::muted(theme, "Nothing is downloaded until you choose an engine."),
        ]
        .spacing(10)
        .into(),
    )
}

fn doctor<'a>(studio: &'a Studio, theme: &'a Theme) -> Element<'a, Message, Theme> {
    let body: Element<'_, Message, Theme> = match &studio.preflight {
        None => column![
            t::body(theme, "Checks have not run yet."),
            t::muted(theme, "Press Run checks on the previous step."),
        ]
        .spacing(8)
        .into(),
        Some(report) => {
            let blockers = report
                .findings
                .iter()
                .filter(|f| f.level == Level::Blocker)
                .count();
            let warnings = report
                .findings
                .iter()
                .filter(|f| f.level == Level::Warning)
                .count();
            let passed = report
                .findings
                .iter()
                .filter(|f| f.level == Level::Ok)
                .count();
            let mut items: Vec<Element<'_, Message, Theme>> = vec![
                row![
                    t::badge(format!("{blockers} blockers"), Tone::Error),
                    t::badge(format!("{warnings} warnings"), Tone::Warning),
                    t::badge(format!("{passed} passed"), Tone::Success),
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
                items.push(
                    row![
                        t::badge(finding.code, tone),
                        t::body(theme, finding.message.clone())
                    ]
                    .spacing(10)
                    .into(),
                );
            }
            if let Some(signature) = warning_signature(report) {
                let acknowledged = studio.warning_signature.as_deref() == Some(signature.as_str());
                items.push(
                    checkbox(studio.warning_signature.is_some())
                        .label("I understand the advisory findings")
                        .on_toggle(Message::AckWarnings)
                        .into(),
                );
                let _ = acknowledged;
            }
            items.push(t::muted(
                theme,
                "Some host facts are best-effort; blockers are always enforced.",
            ));
            t::scroll_box(column(items).spacing(8))
        }
    };
    t::card_headed(
        "Diagnostics",
        "Host checks",
        "Best effort",
        column![body, t::secondary("Re-run checks", Message::RunDoctor)]
            .spacing(12)
            .into(),
    )
}

fn engine<'a>(studio: &'a Studio, theme: &'a Theme) -> Element<'a, Message, Theme> {
    if studio.engines.is_empty() {
        return t::empty_state(
            "No engine builds yet",
            "Download the published index from GitHub, then pick a build for this host.",
            "Download from GitHub",
            Message::RefreshCatalog,
        );
    }
    let mut items: Vec<Element<'_, Message, Theme>> = Vec::new();
    for (index, engine) in studio.engines.iter().enumerate() {
        let label = format!(
            "{} · {} · {} · macOS {}",
            engine.engine_id,
            engine.channel,
            catalog::size(engine.archive_size),
            engine.supported_macos_range
        );
        items.push(
            shadcn_iced::button::button(label)
                .variant(if studio.selected_engine == Some(index) {
                    shadcn_iced::button::Variant::Secondary
                } else {
                    shadcn_iced::button::Variant::Ghost
                })
                .on_press(Message::SelectEngine(index))
                .into(),
        );
    }
    let detail: Element<'_, Message, Theme> = match selected(studio) {
        Some(engine) => {
            let installed =
                studio.installed_engine_id.as_deref() == Some(engine.engine_id.as_str());
            let install_control: Element<'_, Message, Theme> = if studio.installing {
                column![
                    t::muted(
                        theme,
                        format!(
                            "Downloading {}/{} MiB",
                            studio.install_progress / (1024 * 1024),
                            studio.install_total / (1024 * 1024)
                        )
                    ),
                    t::bytes(studio.install_progress, studio.install_total),
                ]
                .spacing(8)
                .into()
            } else if installed {
                t::badge("Installed", Tone::Success)
            } else {
                t::primary("Download and install", Message::InstallEngine)
            };
            let changelog = engine
                .changelog
                .as_deref()
                .unwrap_or("No changelog published yet.");
            let entries = catalog::parse_changelog(changelog);
            let changelog_view: Element<'_, Message, Theme> = if entries.is_empty() {
                t::scroll_box(text(changelog).size(12.0).width(Length::Fill))
            } else {
                let mut accordion = shadcn_iced::accordion::accordion();
                for entry in entries {
                    accordion = accordion.item(
                        format!("{} · {}", entry.sha, entry.date),
                        column![text(entry.subject).size(13.0), text(entry.body).size(12.0)]
                            .spacing(6),
                    );
                }
                t::scroll_box(accordion)
            };
            column![
                text(&engine.engine_id).size(15.0),
                t::muted(
                    theme,
                    format!(
                        "ABI {} · macOS {} · {}",
                        engine.abi_version,
                        engine.supported_macos_range,
                        catalog::size(engine.archive_size)
                    )
                ),
                t::muted(
                    theme,
                    format!(
                        "Built {} · source {} · QEMU {}",
                        engine.built_at.as_deref().unwrap_or("unknown"),
                        engine.source_revision.as_deref().unwrap_or("unknown"),
                        engine.qemu_revision.as_deref().unwrap_or("unknown")
                    )
                ),
                changelog_view,
                row![
                    install_control,
                    t::secondary("Copy download URL", Message::CopyEngineUrl),
                    t::secondary("Open release page", Message::OpenRelease),
                ]
                .spacing(8),
            ]
            .spacing(8)
            .into()
        }
        None => t::muted(theme, "Select a build to see its details."),
    };
    t::card_headed(
        "Engine selection",
        "Choose an engine",
        "GitHub release",
        column![
            column(items).spacing(6),
            detail,
            row![
                t::secondary("Refresh from GitHub", Message::RefreshCatalog),
                t::muted(
                    theme,
                    match studio.catalog_state {
                        crate::CatalogState::Remote => "Source: GitHub release",
                        crate::CatalogState::Cached => "Source: local cache",
                        crate::CatalogState::Empty => "Source: none",
                    }
                ),
            ]
            .spacing(10),
        ]
        .spacing(12)
        .into(),
    )
}

fn macos<'a>(studio: &'a Studio, theme: &'a Theme) -> Element<'a, Message, Theme> {
    let count = studio
        .engines
        .iter()
        .filter(|engine| catalog::supports_macos(engine, studio.macos_version.trim()))
        .count();
    t::card_headed(
        "Guest",
        "macOS version",
        "Compatibility",
        column![
            t::body(theme, "Which macOS version will you install?"),
            shadcn_iced::select::select(Some(studio.macos_version.clone()), Message::MacVersion,)
                .placeholder("Select macOS version")
                .label("Version")
                .option(String::from("26.4"), "macOS 26.4")
                .option(String::from("26.5"), "macOS 26.5")
                .option(String::from("27.0"), "macOS 27.0")
                .option(String::from("27.1"), "macOS 27.1")
                .option(String::from("27.2"), "macOS 27.2"),
            field(
                "Custom",
                &studio.macos_version,
                "e.g. 28.0",
                Message::MacVersion
            ),
            t::muted(
                theme,
                format!("{count} compatible build(s) for this version.")
            ),
        ]
        .spacing(10)
        .into(),
    )
}

fn vm<'a>(studio: &'a Studio, _theme: &'a Theme) -> Element<'a, Message, Theme> {
    t::card_headed(
        "Virtual machine",
        "VM settings",
        "Defaults work",
        column![
            field("Name", &studio.vm_name, "vm name", Message::VmName),
            field("RAM MiB", &studio.ram, "8192", Message::Ram),
            field("vCPUs", &studio.vcpus, "4", Message::Vcpus),
            field(
                "Resolution",
                &studio.resolution,
                "1920x1080",
                Message::Resolution
            ),
        ]
        .spacing(10)
        .into(),
    )
}

fn install<'a>(studio: &'a Studio, theme: &'a Theme) -> Element<'a, Message, Theme> {
    let host_state = if studio.post_install_applied {
        t::badge("Host settings applied automatically", Tone::Success)
    } else {
        t::badge("Host settings pending", Tone::Warning)
    };
    t::card_headed(
        "Guest setup",
        "Install macOS",
        "Checklist",
        column![
            host_state,
            t::body(theme, "1 · Boot the VM with the macOS installer attached."),
            t::muted(
                theme,
                "2 · The command below is for the guest's macOS Recovery Terminal, NOT for this PC."
            ),
            text(POST_INSTALL_COMMAND).size(12.0),
            t::secondary("Copy guest command", Message::CopyPostInstall),
            t::body(theme, "3 · Eject the installer and reboot into macOS."),
            t::muted(
                theme,
                "OpenCore settings on the host are already written to vm.toml."
            ),
            t::primary("Start VM", Message::StartVm),
            t::secondary("Re-apply host settings", Message::PostInstall),
        ]
        .spacing(10)
        .into(),
    )
}

fn summary<'a>(studio: &'a Studio, theme: &'a Theme) -> Element<'a, Message, Theme> {
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
                "{} blockers · {} warnings · {} passed",
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
    let mut rows: Vec<Element<'_, Message, Theme>> = vec![
        row![t::muted(theme, "Engine"), t::body(theme, engine)]
            .spacing(12)
            .into(),
        row![
            t::muted(theme, "macOS"),
            t::body(theme, studio.macos_version.clone())
        ]
        .spacing(12)
        .into(),
        row![
            t::muted(theme, "VM"),
            t::body(theme, studio.vm_name.clone())
        ]
        .spacing(12)
        .into(),
        row![t::muted(theme, "Host checks"), t::body(theme, verdict)]
            .spacing(12)
            .into(),
        row![
            t::muted(theme, "Ports"),
            t::body(
                theme,
                format!("bridge {} · QMP {}", studio.bridge_port, studio.qmp_port)
            )
        ]
        .spacing(12)
        .into(),
        row![
            t::muted(theme, "Portable folder"),
            t::body(theme, studio.portable_root.display().to_string())
        ]
        .spacing(12)
        .into(),
    ];
    rows.insert(0, t::badge("Setup complete", Tone::Success));
    t::card("Summary", column(rows).spacing(8).into())
}
