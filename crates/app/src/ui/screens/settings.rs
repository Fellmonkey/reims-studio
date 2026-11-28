//! Settings screen: theme, engine, ports and setup controls.

use iced::widget::column;
use iced::{Element, Length, Theme};
use shadcn_iced::Mode;
use shadcn_iced::button as si_button;

use crate::ui::theme as t;
use crate::ui::widgets::field;
use crate::{Channel, Message, Studio, catalog};

/// Renders the settings screen.
pub fn view<'a>(studio: &'a Studio, theme: &Theme) -> Element<'a, Message, Theme> {
    const CHANNELS: [&str; 3] = ["master", "development", "patched"];
    let index = match studio.channel {
        Channel::Master => 0,
        Channel::Development => 1,
        Channel::Patched => 2,
    };
    let channel_tabs: Element<'_, Message, Theme> = shadcn_iced::tabs::tabs(index, |index| {
        Message::Channel(match index {
            0 => Channel::Master,
            1 => Channel::Development,
            _ => Channel::Patched,
        })
    })
    .tab(0, CHANNELS[0])
    .tab(1, CHANNELS[1])
    .tab(2, CHANNELS[2])
    .into();

    let mut engine_items = Vec::new();
    for (index, engine) in studio.engines.iter().enumerate() {
        engine_items.push(
            si_button::button(catalog::line(engine))
                .variant(if studio.selected_engine == Some(index) {
                    si_button::Variant::Secondary
                } else {
                    si_button::Variant::Ghost
                })
                .on_press(Message::SelectEngine(index))
                .into(),
        );
    }

    column![
        t::card_headed(
            "Preferences",
            "Appearance",
            "Theme",
            column![
                t::muted(theme, "Dark and light use the same shadcn tokens."),
                iced::widget::row![
                    t::secondary("Dark", Message::SetTheme(Mode::Dark)),
                    t::secondary("Light", Message::SetTheme(Mode::Light)),
                ]
                .spacing(8),
            ]
            .spacing(10)
            .into(),
        ),
        t::card_headed(
            "Engine",
            "Active build",
            "GitHub release",
            column![
                channel_tabs,
                column(engine_items).spacing(6),
                iced::widget::row![
                    t::secondary("Refresh from GitHub", Message::RefreshCatalog),
                    t::secondary("Open release page", Message::OpenRelease),
                    t::secondary("Use for VM", Message::ApplyEngine),
                ]
                .spacing(10),
            ]
            .spacing(10)
            .into(),
        ),
        t::card_headed(
            "Advanced",
            "Ports and saves",
            "settings.toml",
            column![
                field(
                    "Bridge port",
                    &studio.bridge_port,
                    "8080",
                    Message::BridgePort
                ),
                field("QMP port", &studio.qmp_port, "4444", Message::QmpPort),
                iced::widget::row![
                    t::primary("Save settings", Message::SaveSettings),
                    t::secondary("Re-run checks", Message::RunDoctor),
                    t::secondary("Rerun setup", Message::RerunSetup),
                ]
                .spacing(10),
            ]
            .spacing(10)
            .into(),
        ),
    ]
    .spacing(12)
    .height(Length::Fill)
    .into()
}
