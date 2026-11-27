//! Design tokens and reusable widgets for the launcher.
//!
//! The palette is dark-only and meets WCAG AA for text on cards. The app runs
//! on the `tiny-skia` software renderer, so effects are limited to flat fills,
//! borders and spacing that render identically on every host.

use iced::widget::{button, column, container, progress_bar, text};
use iced::{Background, Border, Color, Element, Font, Length, Theme, font};

/// Page background.
pub const PAGE: Color = Color::from_rgb(0.051, 0.067, 0.090);
/// Raised card background.
pub const CARD: Color = Color::from_rgb(0.086, 0.106, 0.133);
/// Hover and selected surface.
pub const CARD_ALT: Color = Color::from_rgb(0.110, 0.129, 0.157);
/// Subtle border around surfaces.
pub const BORDER: Color = Color::from_rgb(0.188, 0.212, 0.239);
/// Primary text; 14.6:1 on `CARD`.
pub const TEXT: Color = Color::from_rgb(0.902, 0.929, 0.953);
/// Secondary text; 8.2:1 on `CARD`.
pub const MUTED: Color = Color::from_rgb(0.663, 0.706, 0.761);
/// Interactive accent; 6.9:1 on `CARD`.
pub const ACCENT: Color = Color::from_rgb(0.345, 0.651, 1.000);
/// Accent on hover.
pub const ACCENT_HOVER: Color = Color::from_rgb(0.475, 0.722, 1.000);
/// Text on the accent color; 7.5:1 on `ACCENT`.
pub const ON_ACCENT: Color = Color::from_rgb(0.031, 0.067, 0.122);
/// Success; 6.8:1 on `CARD`.
pub const SUCCESS: Color = Color::from_rgb(0.247, 0.725, 0.314);
/// Warning; 6.9:1 on `CARD`.
pub const WARNING: Color = Color::from_rgb(0.824, 0.600, 0.133);
/// Error; 5.2:1 on `CARD`.
pub const DANGER: Color = Color::from_rgb(0.973, 0.319, 0.286);
/// Disabled text.
pub const DISABLED: Color = Color::from_rgb(0.431, 0.463, 0.506);

/// Spacing unit used everywhere in the app.
pub const GAP: f32 = 12.0;
/// Card padding.
pub const PAD: f32 = 16.0;

/// Semantic tone of a notice or finding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    /// Neutral information.
    Info,
    /// Something succeeded.
    Success,
    /// Something needs attention but is not fatal.
    Warning,
    /// Something blocks progress.
    Error,
}

impl Tone {
    /// Returns the matching color.
    pub fn color(self) -> Color {
        match self {
            Self::Info => ACCENT,
            Self::Success => SUCCESS,
            Self::Warning => WARNING,
            Self::Error => DANGER,
        }
    }
}

/// A user-facing status line with an explicit tone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Notice {
    /// Severity of the message.
    pub tone: Tone,
    /// Message text.
    pub text: String,
}

impl Notice {
    /// Creates an informational notice.
    pub fn info(text: impl Into<String>) -> Self {
        Self {
            tone: Tone::Info,
            text: text.into(),
        }
    }
    /// Creates a success notice.
    pub fn success(text: impl Into<String>) -> Self {
        Self {
            tone: Tone::Success,
            text: text.into(),
        }
    }
    /// Creates a warning notice.
    pub fn warning(text: impl Into<String>) -> Self {
        Self {
            tone: Tone::Warning,
            text: text.into(),
        }
    }
    /// Creates an error notice.
    pub fn error(text: impl Into<String>) -> Self {
        Self {
            tone: Tone::Error,
            text: text.into(),
        }
    }
}

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
        text_color: Some(TEXT),
        border: Border {
            color: BORDER,
            width: 1.0,
            radius: 8.0.into(),
        },
        ..container::Style::default()
    })
    .into()
}

/// Returns a section heading.
pub fn section_title<'a, Message: 'a>(title: &'static str) -> Element<'a, Message, Theme> {
    text(title)
        .size(16.0)
        .font(Font {
            weight: font::Weight::Semibold,
            ..Font::default()
        })
        .color(TEXT)
        .into()
}

/// Returns muted secondary text.
pub fn muted<'a, Message: 'a>(value: impl Into<String>) -> Element<'a, Message, Theme> {
    text(value.into()).size(13.0).color(MUTED).into()
}

/// Returns primary text at body size.
pub fn body<'a, Message: 'a>(value: impl Into<String>) -> Element<'a, Message, Theme> {
    text(value.into()).size(14.0).color(TEXT).into()
}

/// Returns a colored badge with a tinted background.
pub fn badge<'a, Message: 'a>(value: impl Into<String>, tone: Tone) -> Element<'a, Message, Theme> {
    let color = tone.color();
    container(text(value.into()).size(12.0).color(color))
        .padding([4.0, 8.0])
        .style(move |_theme| container::Style {
            background: Some(Background::Color(Color { a: 0.16, ..color })),
            border: Border {
                color,
                width: 1.0,
                radius: 4.0.into(),
            },
            ..container::Style::default()
        })
        .into()
}

/// Returns a status badge whose background tint fades with `tint` (0..=1).
pub fn notice_alpha<'a, Message: 'a>(value: &Notice, tint: f32) -> Element<'a, Message, Theme> {
    let color = value.tone.color();
    let tint = tint.clamp(0.0, 1.0);
    container(text(value.text.clone()).size(12.0).color(color))
        .padding([4.0, 8.0])
        .style(move |_theme| container::Style {
            background: Some(Background::Color(Color {
                a: 0.16 * tint,
                ..color
            })),
            border: Border {
                color,
                width: 1.0,
                radius: 4.0.into(),
            },
            ..container::Style::default()
        })
        .into()
}

fn button_style(active: bool, hovered: bool) -> button::Style {
    button::Style {
        background: Some(Background::Color(if active {
            ACCENT
        } else if hovered {
            CARD_ALT
        } else {
            CARD
        })),
        text_color: if active { ON_ACCENT } else { TEXT },
        border: Border {
            color: if active { ACCENT } else { BORDER },
            width: 1.0,
            radius: 6.0.into(),
        },
        ..button::Style::default()
    }
}

/// Returns a filled button for the primary action.
pub fn primary<'a, Message: Clone + 'a>(
    label: &'static str,
    message: Message,
) -> Element<'a, Message, Theme> {
    primary_enabled(label, Some(message))
}

/// Returns a primary button that is visibly disabled when there is no action.
pub fn primary_enabled<'a, Message: Clone + 'a>(
    label: &'static str,
    message: Option<Message>,
) -> Element<'a, Message, Theme> {
    let control = button(text(label).size(14.0)).style(|_theme, status| {
        let disabled = matches!(status, button::Status::Disabled);
        button::Style {
            background: Some(Background::Color(if disabled {
                CARD_ALT
            } else {
                match status {
                    button::Status::Hovered | button::Status::Pressed => ACCENT_HOVER,
                    _ => ACCENT,
                }
            })),
            text_color: if disabled { DISABLED } else { ON_ACCENT },
            border: Border {
                color: if disabled { BORDER } else { ACCENT },
                width: 1.0,
                radius: 6.0.into(),
            },
            ..button::Style::default()
        }
    });
    match message {
        Some(message) => control.on_press(message).into(),
        None => control.into(),
    }
}

/// Returns a bordered button for secondary actions.
pub fn secondary<'a, Message: Clone + 'a>(
    label: &'static str,
    message: Message,
) -> Element<'a, Message, Theme> {
    button(text(label).size(14.0))
        .on_press(message)
        .style(|_theme, status| button::Style {
            background: Some(Background::Color(match status {
                button::Status::Hovered | button::Status::Pressed => CARD_ALT,
                _ => CARD,
            })),
            text_color: TEXT,
            border: Border {
                color: BORDER,
                width: 1.0,
                radius: 6.0.into(),
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
    button(text(label).size(14.0))
        .on_press(message)
        .style(move |_theme, status| {
            button_style(
                active,
                matches!(status, button::Status::Hovered | button::Status::Pressed),
            )
        })
        .into()
}

/// Returns a selectable list row highlighted when it is the current choice.
pub fn choice<'a, Message: Clone + 'a>(
    label: String,
    selected: bool,
    message: Message,
) -> Element<'a, Message, Theme> {
    button(text(label).size(13.0))
        .on_press(message)
        .width(Length::Fill)
        .style(move |_theme, status| {
            button_style(
                selected,
                matches!(status, button::Status::Hovered | button::Status::Pressed),
            )
        })
        .into()
}

/// Returns a byte-progress bar for a download.
pub fn bytes<'a, Message: 'a>(downloaded: u64, total: u64) -> Element<'a, Message, Theme> {
    let fraction = if total == 0 {
        0.0
    } else {
        (downloaded as f32 / total as f32).clamp(0.0, 1.0)
    };
    progress_bar(0.0..=1.0, fraction)
        .girth(Length::Fixed(8.0))
        .style(|_theme| progress_bar::Style {
            background: Background::Color(CARD_ALT),
            bar: Background::Color(ACCENT),
            border: Border {
                radius: 4.0.into(),
                ..Border::default()
            },
        })
        .into()
}

/// Returns the step progress bar for the setup flow.
pub fn progress<'a, Message: 'a>(current: usize, total: usize) -> Element<'a, Message, Theme> {
    progress_bar(0.0..=total as f32, current as f32)
        .girth(Length::Fixed(6.0))
        .style(|_theme| progress_bar::Style {
            background: Background::Color(CARD_ALT),
            bar: Background::Color(ACCENT),
            border: Border {
                radius: 3.0.into(),
                ..Border::default()
            },
        })
        .into()
}
