//! Post-setup home screen.

use iced::widget::{column, row, slider, text};
use iced::{Element, Length, Theme};

use crate::ui::theme as t;
use crate::ui::theme::Tone;
use crate::{Message, Stage, Studio, catalog, selected};

/// Renders the compact dashboard.
pub fn view<'a>(studio: &'a Studio, theme: &Theme) -> Element<'a, Message, Theme> {
    let engine =
        selected(studio).map_or_else(|| String::from("No engine selected."), catalog::line);
    let verdict = studio.preflight.as_ref().map_or_else(
        || String::from("checks not run"),
        |report| {
            let blockers = report
                .findings
                .iter()
                .filter(|f| f.level == reims_platform::preflight::Level::Blocker)
                .count();
            if blockers == 0 {
                "host ready".to_string()
            } else {
                "host has blockers".to_string()
            }
        },
    );
    let tone = if verdict == "host ready" {
        Tone::Success
    } else {
        Tone::Warning
    };
    column![
        t::card_headed(
            "Active setup",
            "Current engine",
            "Portable",
            column![
                t::badge(verdict, tone),
                t::body(theme, engine),
                t::muted(theme, format!("Channel: {}", studio.channel)),
                row![
                    t::secondary("Change engine", Message::Stage(Stage::Settings)),
                    t::secondary("Open release page", Message::OpenRelease),
                    t::secondary("Render QEMU command", Message::BuildCommand),
                    t::primary("Start VM", Message::StartVm),
                ]
                .spacing(10),
            ]
            .spacing(10)
            .into(),
        ),
        t::card_headed(
            "Telemetry",
            "Host / VM RAM",
            "Present mode",
            t::meter(theme, studio.ram_usage),
        ),
        t::card_headed(
            "Diagnostics",
            "Engine log",
            "Latest",
            column![
                t::scroll_box(text(studio.logs.page(studio.log_page, crate::LOG_LINES)).size(12.0)),
                slider(
                    0.0..=studio.logs.len().saturating_sub(1) as f32,
                    studio.log_page as f32,
                    Message::LogPage,
                ),
                row![
                    t::secondary(
                        if studio.paused { "Resume" } else { "Pause" },
                        Message::Pause
                    ),
                    t::secondary("Copy page", Message::CopyLogs),
                ]
                .spacing(8),
            ]
            .spacing(8)
            .into(),
        ),
    ]
    .spacing(12)
    .height(Length::Fill)
    .into()
}
