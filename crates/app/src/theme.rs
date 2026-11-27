//! Shared visual tokens for the launcher.
//!
//! The UI runs on the software `tiny-skia` backend, so the design system uses
//! flat colors, consistent spacing and simple boxes instead of effects that
//! need a GPU renderer.

use iced::widget::{button, column, container, row, text};
use iced::{Background, Border, Color, Element, Length, Theme};

/// Accent color for primary actions and the active step.
pub const ACCENT: Color = Color::from_rgb(0.16, 0.42, 0.68);
/// Muted text for secondary information.
pub const MUTED: Color = Color::from_rgb(0.55, 0.59, 0.65);
/// Success color for decoded checks.
pub const SUCCESS: Color = Color::from_rgb(0.18, 0.62, 0.38);
/// Warning color for degraded states.
pub const WARNING: Color = Color::from_rgb(0.85, 0.58, 0.16);
/// Error color for blockers.
pub const DANGER: Color = Color::from_rgb(0.82, 0.30, 0.30);
/// Background of a raised card.
pub const CARD: Color = Color::from_rgb(0.10, 0.12, 0.15);
/// Foreground used on top of the accent color.
pub const ON_ACCENT: Color = Color::from_rgb(0.96, 0.97, 0.98);

/// Vertical rhythm between sections.
pub const GAP: f32 = 12.0;
/// Padding inside a card.
pub const PAD: f32 = 16.0;

/// Returns a titled card with one body element.
pub fn card<'a, Message: 'a>(
    title: &'static str,
    body: Element<'a, Message, Theme>,
) -> Element<'a, Message, Theme> {
    container(
        column![section_title(title), body]
            .spacing(GAP)
            .width(Length::Fill),
    )
    .padding(PAD)
    .width(Length::Fill)
    .style(|_theme| container::Style {
        background: Some(Background::Color(CARD)),
        border: Border {
            color: Color { a: 0.35, ..ACCENT },
            width: 1.0,
            radius: 8.0.into(),
        },
        ..container::Style::default()
    })
    .into()
}

/// Returns a section heading.
pub fn section_title<'a, Message: 'a>(title: &'static str) -> Element<'a, Message, Theme> {
    text(title).size(18).color(ACCENT).into()
}

/// Returns muted secondary text.
pub fn muted<'a, Message: 'a>(value: impl Into<String>) -> Element<'a, Message, Theme> {
    text(value.into()).size(13).color(MUTED).into()
}

/// Returns a short status badge.
pub fn badge<'a, Message: 'a>(
    value: impl Into<String>,
    color: Color,
) -> Element<'a, Message, Theme> {
    text(value.into()).size(13).color(color).into()
}

/// Returns the status line colored by outcome.
pub fn status<'a, Message: 'a>(value: &str) -> Element<'a, Message, Theme> {
    let lower = value.to_ascii_lowercase();
    let color = if lower.contains("unavailable") || lower.contains("not created") {
        WARNING
    } else if lower.contains("fail")
        || lower.contains("error")
        || lower.starts_with("choose")
        || lower.starts_with("no ")
    {
        DANGER
    } else if lower.contains("created")
        || lower.contains("applied")
        || lower.contains("refreshed")
        || lower.contains("selected")
    {
        SUCCESS
    } else {
        MUTED
    };
    text(value.to_owned()).size(13).color(color).into()
}

/// Returns a filled button for the primary action.
pub fn primary<'a, Message: Clone + 'a>(
    label: &'static str,
    message: Message,
) -> Element<'a, Message, Theme> {
    button(text(label))
        .on_press(message)
        .style(|_theme, status| button::Style {
            background: Some(Background::Color(match status {
                button::Status::Hovered | button::Status::Pressed => Color { a: 0.85, ..ACCENT },
                _ => ACCENT,
            })),
            text_color: ON_ACCENT,
            border: Border {
                radius: 6.0.into(),
                ..Border::default()
            },
            ..button::Style::default()
        })
        .into()
}

/// Returns a sidebar button, highlighted when it is the active screen.
pub fn nav<'a, Message: Clone + 'a>(
    label: &'static str,
    active: bool,
    message: Message,
) -> Element<'a, Message, Theme> {
    button(text(label).size(14))
        .on_press(message)
        .style(move |_theme, status| button::Style {
            background: Some(Background::Color(if active {
                Color { a: 0.30, ..ACCENT }
            } else {
                match status {
                    button::Status::Hovered | button::Status::Pressed => {
                        Color { a: 0.12, ..ACCENT }
                    }
                    _ => Color { a: 0.0, ..ACCENT },
                }
            })),
            text_color: if active { ON_ACCENT } else { MUTED },
            border: Border {
                radius: 6.0.into(),
                ..Border::default()
            },
            ..button::Style::default()
        })
        .into()
}

/// Returns a selectable list row highlighted when it is the current choice.
pub fn choice<'a, Message: Clone + 'a>(
    label: String,
    selected: bool,
    message: Message,
) -> Element<'a, Message, Theme> {
    button(text(label).size(13))
        .on_press(message)
        .width(Length::Fill)
        .style(move |_theme, status| button::Style {
            background: Some(Background::Color(if selected {
                Color { a: 0.32, ..ACCENT }
            } else {
                match status {
                    button::Status::Hovered | button::Status::Pressed => {
                        Color { a: 0.14, ..ACCENT }
                    }
                    _ => Color { a: 0.06, ..ACCENT },
                }
            })),
            text_color: if selected { ON_ACCENT } else { MUTED },
            border: Border {
                radius: 6.0.into(),
                ..Border::default()
            },
            ..button::Style::default()
        })
        .into()
}

/// Returns the horizontal progress indicator for the staged wizard.
pub fn steps<'a, Message: 'a>(
    labels: &'static [&'static str],
    active: usize,
) -> Element<'a, Message, Theme> {
    let items = labels
        .iter()
        .enumerate()
        .map(|(index, label)| {
            text(format!("{}. {}", index + 1, label))
                .size(13)
                .color(if index == active {
                    ON_ACCENT
                } else if index < active {
                    SUCCESS
                } else {
                    MUTED
                })
                .into()
        })
        .collect::<Vec<Element<'a, Message, Theme>>>();
    row(items).spacing(GAP).into()
}
