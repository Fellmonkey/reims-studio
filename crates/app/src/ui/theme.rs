//! shadcn/ui-flavoured theme helpers for the launcher.
//!
//! Every visual decision comes from the current iced theme's shadcn tokens, so
//! dark and light modes share exactly the same layout.

use iced::widget::{container, text, tooltip};
use iced::{Background, Element, Length, Theme};
use shadcn_iced::button as si_button;
use shadcn_iced::{
    badge as si_badge, card as si_card, empty as si_empty, progress as si_progress, scroll_area,
    separator, tokens,
};

use crate::ActionFeedback;

/// Semantic tone of a status message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    /// Neutral information.
    Info,
    /// Successful action.
    Success,
    /// Advisory finding.
    Warning,
    /// Blocking failure.
    Error,
}

/// A status message with an explicit tone.
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

/// Surface style that paints the whole window with the background token.
pub fn page_style(theme: &Theme) -> container::Style {
    let palette = tokens(theme).palette;
    container::Style {
        background: Some(Background::Color(palette.background)),
        text_color: Some(palette.foreground),
        ..container::Style::default()
    }
}

/// Returns a shadcn card with a title and one body element.
pub fn card<'a, Message: 'a>(
    title: &'static str,
    body: Element<'a, Message, Theme>,
) -> Element<'a, Message, Theme> {
    si_card::card()
        .title(title)
        .content(body)
        .width(Length::Fill)
        .into()
}

/// Returns a shadcn card with eyebrow, title, metadata and a header divider.
pub fn card_headed<'a, Message: 'a>(
    eyebrow_text: &'static str,
    title: &'static str,
    meta: &'static str,
    body: Element<'a, Message, Theme>,
) -> Element<'a, Message, Theme> {
    si_card::card()
        .title(format!("{eyebrow_text} · {title}"))
        .description(meta)
        .header_border(true)
        .content(body)
        .width(Length::Fill)
        .into()
}

/// Returns secondary text using the muted-foreground token.
pub fn muted<'a, Message: 'a>(
    theme: &Theme,
    value: impl Into<String>,
) -> Element<'a, Message, Theme> {
    text(value.into())
        .size(13.0)
        .color(tokens(theme).palette.muted_foreground)
        .into()
}

/// Returns body text using the foreground token.
pub fn body<'a, Message: 'a>(
    theme: &Theme,
    value: impl Into<String>,
) -> Element<'a, Message, Theme> {
    text(value.into())
        .size(14.0)
        .color(tokens(theme).palette.foreground)
        .into()
}

/// Returns a primary action button.
pub fn primary<'a, Message: Clone + 'a>(
    label: &'static str,
    message: Message,
) -> Element<'a, Message, Theme> {
    si_button::button(label).on_press(message).into()
}

/// Returns a primary button that can be visibly disabled.
pub fn primary_enabled<'a, Message: Clone + 'a>(
    label: &'static str,
    message: Option<Message>,
) -> Element<'a, Message, Theme> {
    si_button::button(label)
        .on_press_maybe(message.clone())
        .disabled(message.is_none())
        .into()
}

/// Returns a secondary action button.
pub fn secondary<'a, Message: Clone + 'a>(
    label: &'static str,
    message: Message,
) -> Element<'a, Message, Theme> {
    si_button::button(label)
        .variant(si_button::Variant::Secondary)
        .on_press(message)
        .into()
}

/// Returns a badge for a semantic tone.
pub fn badge<'a, Message: Clone + 'a>(
    value: impl Into<String>,
    tone: Tone,
) -> Element<'a, Message, Theme> {
    let variant = match tone {
        Tone::Success => si_badge::Variant::Default,
        Tone::Warning => si_badge::Variant::Outline,
        Tone::Error => si_badge::Variant::Destructive,
        Tone::Info => si_badge::Variant::Secondary,
    };
    si_badge::badge(value.into()).variant(variant).into()
}

/// Returns a persistent status line with the last action's duration.
pub fn feedback_line<'a, Message: Clone + 'a>(
    _theme: &Theme,
    status: &Notice,
    action: Option<&ActionFeedback>,
) -> Element<'a, Message, Theme> {
    let mut items = vec![badge(status.text.clone(), status.tone)];
    if let Some(action) = action {
        let tone = if action.ok {
            Tone::Success
        } else {
            Tone::Error
        };
        let verb = if action.ok { "done in" } else { "failed in" };
        items.push(badge(
            format!(
                "{} · {} {} ms",
                action.kind.label(),
                verb,
                action.elapsed_ms
            ),
            tone,
        ));
    }
    iced::widget::row(items).spacing(8).into()
}

/// Returns a 1px separator.
pub fn divider<'a, Message: 'a>() -> Element<'a, Message, Theme> {
    separator::separator().into()
}

/// Returns a ScrollArea that fills the space left by sibling widgets.
pub fn scroll_box<'a, Message: 'a>(
    content: impl Into<Element<'a, Message, Theme>>,
) -> Element<'a, Message, Theme> {
    scroll_area::scroll_area(content)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

/// Returns an empty state with a call to action.
pub fn empty_state<'a, Message: Clone + 'a>(
    title: &'static str,
    hint: &'static str,
    action_label: &'static str,
    message: Message,
) -> Element<'a, Message, Theme> {
    si_empty::empty(title)
        .description(hint)
        .extra(primary(action_label, message))
        .bordered(true)
        .into()
}

/// Returns a shadcn progress bar from a fraction (0..=1).
pub fn progress<'a, Message: 'a>(fraction: f32) -> Element<'a, Message, Theme> {
    si_progress::progress(fraction.clamp(0.0, 1.0) * 100.0).into()
}

/// Returns a byte progress bar.
pub fn bytes<'a, Message: 'a>(downloaded: u64, total: u64) -> Element<'a, Message, Theme> {
    progress(if total == 0 {
        0.0
    } else {
        downloaded as f32 / total as f32
    })
}

/// Returns a tooltip-wrapped element, used for icon-only navigation.
pub fn hint<'a, Message: 'a>(
    content: impl Into<Element<'a, Message, Theme>>,
    label: &'static str,
    theme: &Theme,
) -> Element<'a, Message, Theme> {
    let caption = text(label)
        .size(12.0)
        .color(tokens(theme).palette.primary_foreground);
    tooltip(content, caption, tooltip::Position::Right)
        .gap(4.0)
        .into()
}

/// Returns the RAM/present-mode meter as a shadcn progress bar.
pub fn meter<'a, Message: 'a>(_theme: &Theme, usage: f32) -> Element<'a, Message, Theme> {
    progress(usage)
}

/// Step progress bar for the setup flow.
pub fn steps<'a, Message: 'a>(current: usize, total: usize) -> Element<'a, Message, Theme> {
    progress(current as f32 / total as f32)
}
