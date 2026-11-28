//! The launcher UI, built on shadcn-iced components and tokens.

pub mod screens;
pub mod theme;
pub mod widgets;

use iced::widget::{column, container, row, text};
use iced::{Element, Length, Theme};
use shadcn_iced::button as si_button;
use shadcn_iced::prelude::{Icon, Mode};

use crate::{Message, Stage, Studio};

/// Reads the persisted appearance preference; missing files default to dark.
pub fn load_theme(root: &std::path::Path) -> Mode {
    let raw = std::fs::read_to_string(root.join("ui_settings.toml")).unwrap_or_default();
    match raw.trim() {
        "theme = \"light\"" => Mode::Light,
        _ => Mode::Dark,
    }
}

/// Persists the appearance preference outside the launcher's core contracts.
pub fn save_theme(root: &std::path::Path, mode: Mode) -> Result<(), String> {
    let value = match mode {
        Mode::Light => "light",
        Mode::Dark => "dark",
    };
    std::fs::write(
        root.join("ui_settings.toml"),
        format!("theme = \"{value}\"\n"),
    )
    .map_err(|error| error.to_string())
}

/// Builds the iced theme from the chosen shadcn preset.
pub fn theme_for(mode: Mode) -> Theme {
    match mode {
        Mode::Light => shadcn_iced::theme::zinc(Mode::Light).build(),
        Mode::Dark => shadcn_iced::theme::zinc(Mode::Dark).build(),
    }
}

/// Renders the whole window.
pub fn view(studio: &Studio) -> Element<'_, Message, Theme> {
    let theme = &studio.ui_theme;
    let content = match studio.stage {
        Stage::Setup(step) => screens::setup::view(studio, theme, step),
        Stage::Dashboard => frame(studio, theme, screens::dashboard::view(studio, theme)),
        Stage::Settings => frame(studio, theme, screens::settings::view(studio, theme)),
    };
    container(content)
        .width(Length::Fill)
        .height(Length::Fill)
        .style(theme::page_style)
        .into()
}

/// Post-setup frame with an icon rail and fixed status line.
fn frame<'a>(
    studio: &'a Studio,
    theme: &Theme,
    content: Element<'a, Message, Theme>,
) -> Element<'a, Message, Theme> {
    let rail = column![
        rail_button(
            "Dashboard",
            "house",
            studio.stage == Stage::Dashboard,
            Message::Stage(Stage::Dashboard),
            theme
        ),
        rail_button(
            "Settings",
            "settings",
            studio.stage == Stage::Settings,
            Message::Stage(Stage::Settings),
            theme
        ),
        rail_button(
            "Rerun setup",
            "rotate-ccw",
            false,
            Message::RerunSetup,
            theme
        ),
        rail_button("Exit", "log-out", false, Message::Exit, theme),
    ]
    .spacing(8)
    .width(Length::Fixed(56.0));

    column![
        row![
            text("Reims Studio").size(20.0),
            theme::muted(theme, "Portable macOS virtualization"),
        ]
        .spacing(16),
        theme::divider(),
        row![rail, content].spacing(16).height(Length::Fill),
        theme::divider(),
        theme::feedback_line(theme, &studio.status, studio.last_action.as_ref()),
    ]
    .spacing(12)
    .height(Length::Fill)
    .into()
}

fn rail_button<'a>(
    label: &'static str,
    icon_name: &'static str,
    active: bool,
    message: Message,
    theme: &Theme,
) -> Element<'a, Message, Theme> {
    let glyph: Element<'a, Message, Theme> = match Icon::from_name(icon_name) {
        Some(icon) => icon.sized(18).into(),
        None => text(label).size(12.0).into(),
    };
    let button = si_button::Button::new(glyph)
        .variant(if active {
            si_button::Variant::Secondary
        } else {
            si_button::Variant::Ghost
        })
        .on_press(message);
    theme::hint(button, label, theme)
}
