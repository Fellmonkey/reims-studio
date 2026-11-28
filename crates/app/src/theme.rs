//! Design tokens and reusable widgets for the launcher.
//!
//! The palette is dark-only and meets WCAG AA for text on cards. The app runs
//! on the `tiny-skia` software renderer, so effects are limited to flat fills,
//! borders and spacing that render identically on every host.

use iced::widget::{button, column, container, progress_bar, row, rule, scrollable, text};
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
/// Sunken surface for tracks and inputs.
pub const SUNKEN: Color = Color::from_rgb(0.043, 0.059, 0.078);
/// Raised surface for segmented controls.
pub const RAISED: Color = Color::from_rgb(0.133, 0.153, 0.180);
/// Hairline border for subtle separators.
pub const BORDER_SUBTLE: Color = Color::from_rgb(0.129, 0.153, 0.176);
/// Strong border for scroll rails and active controls.
pub const BORDER_STRONG: Color = Color::from_rgb(0.239, 0.267, 0.302);
/// Tertiary text for eyebrows and metadata.
pub const SUBTLE: Color = Color::from_rgb(0.545, 0.580, 0.620);
/// Low-opacity accent fill for active items.
pub const ACCENT_SUBTLE: Color = Color { a: 0.12, ..ACCENT };
/// Fully transparent color for inactive controls.
pub const CLEAR: Color = Color { a: 0.0, ..CARD };

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

/// Returns a 1px hairline separator.
pub fn divider<'a, Message: 'a>() -> Element<'a, Message, Theme> {
    rule::horizontal(1)
        .style(|_theme| rule::Style {
            color: BORDER_SUBTLE,
            radius: 0.0.into(),
            fill_mode: rule::FillMode::Full,
            snap: true,
        })
        .into()
}

/// Returns a small uppercase eyebrow label.
pub fn eyebrow<'a, Message: 'a>(value: &'static str) -> Element<'a, Message, Theme> {
    text(value.to_uppercase())
        .size(11.0)
        .color(SUBTLE)
        .font(Font {
            weight: font::Weight::Bold,
            ..Font::default()
        })
        .into()
}

/// Returns a card with an eyebrow, title, metadata and a hairline header.
pub fn card_headed<'a, Message: 'a>(
    eyebrow_text: &'static str,
    title: &'static str,
    meta: &'static str,
    body: Element<'a, Message, Theme>,
) -> Element<'a, Message, Theme> {
    let header = column![
        eyebrow(eyebrow_text),
        row![section_title(title), text(meta).size(11.0).color(SUBTLE),].spacing(12),
        divider(),
    ]
    .spacing(6);
    container(column![header, body].spacing(GAP).width(Length::Fill))
        .padding(PAD)
        .width(Length::Fill)
        .style(|_theme| container::Style {
            background: Some(Background::Color(CARD)),
            text_color: Some(TEXT),
            border: Border {
                color: BORDER,
                width: 1.0,
                radius: 10.0.into(),
            },
            ..container::Style::default()
        })
        .into()
}

/// Returns a fixed-height viewport with an embedded, themed scrollbar.
pub fn scroll_box<'a, Message: 'a>(
    content: impl Into<Element<'a, Message, Theme>>,
    height: f32,
) -> Element<'a, Message, Theme> {
    let rail = scrollable::Rail {
        background: Some(Background::Color(SUNKEN)),
        border: Border {
            color: BORDER_SUBTLE,
            width: 1.0,
            radius: 4.0.into(),
        },
        scroller: scrollable::Scroller {
            background: Background::Color(BORDER_STRONG),
            border: Border {
                radius: 4.0.into(),
                ..Border::default()
            },
        },
    };
    scrollable(content.into())
        .width(Length::Fill)
        .height(Length::Fixed(height))
        .spacing(6.0)
        .style(move |theme, status| {
            let mut style = scrollable::default(theme, status);
            style.vertical_rail = rail;
            style.horizontal_rail = rail;
            style.container = container::Style {
                background: Some(Background::Color(CARD)),
                ..container::Style::default()
            };
            style
        })
        .into()
}

/// Returns a segmented control for a small set of mutually exclusive choices.
pub fn segmented<'a, Message: Clone + 'a>(
    labels: &'static [&'static str],
    selected: usize,
    choose: impl Fn(usize) -> Message + 'a,
) -> Element<'a, Message, Theme> {
    let items = labels
        .iter()
        .enumerate()
        .map(|(index, label)| {
            let active = index == selected;
            button(text(*label).size(12.0))
                .on_press(choose(index))
                .style(move |_theme, status| button::Style {
                    background: Some(Background::Color(if active {
                        RAISED
                    } else if matches!(status, button::Status::Hovered) {
                        CARD_ALT
                    } else {
                        CARD
                    })),
                    text_color: if active { ACCENT } else { TEXT },
                    border: Border {
                        color: BORDER,
                        width: 1.0,
                        radius: 6.0.into(),
                    },
                    ..button::Style::default()
                })
                .into()
        })
        .collect::<Vec<Element<'a, Message, Theme>>>();
    row(items).spacing(4).into()
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

/// Returns a status pill with a colored dot and tinted background.
pub fn badge<'a, Message: 'a>(value: impl Into<String>, tone: Tone) -> Element<'a, Message, Theme> {
    let color = tone.color();
    let dot = container(text("•").size(12.0).color(color))
        .style(move |_theme| container::Style::default());
    container(row![dot, text(value.into()).size(12.0).color(color)].spacing(6))
        .padding([4.0, 10.0])
        .style(move |_theme| container::Style {
            background: Some(Background::Color(Color { a: 0.12, ..color })),
            border: Border {
                color: Color { a: 0.35, ..color },
                width: 1.0,
                radius: 99.0.into(),
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
    button(text(label).size(13.0))
        .on_press(message)
        .width(Length::Fill)
        .style(move |_theme, status| {
            let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
            button::Style {
                background: Some(Background::Color(if active {
                    ACCENT_SUBTLE
                } else if hovered {
                    CARD_ALT
                } else {
                    CLEAR
                })),
                text_color: if active { ACCENT } else { TEXT },
                border: Border {
                    color: if active { ACCENT } else { CLEAR },
                    width: if active { 2.0 } else { 0.0 },
                    radius: 6.0.into(),
                },
                ..button::Style::default()
            }
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
