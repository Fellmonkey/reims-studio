//! Reusable widgets for the launcher UI.

use iced::widget::{row, text};
use iced::{Element, Length};
use shadcn_iced::prelude::input;

use crate::Message;

/// Labelled input using the shadcn Input component.
pub fn field<'a>(
    label: &'static str,
    value: &'a str,
    placeholder: &'static str,
    on_input: fn(String) -> Message,
) -> Element<'a, Message> {
    row![
        text(label).size(13.0).width(Length::Fixed(120.0)),
        input(placeholder, value).on_input(on_input),
    ]
    .spacing(8)
    .into()
}
