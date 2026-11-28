//! Small reusable widgets for the launcher.

use iced::widget::{canvas, row, text, text_input};
use iced::{Element, Length, Rectangle, Size, Theme};

use crate::Message;
use crate::theme;

pub(crate) fn field<'a>(
    label: &'static str,
    value: &'a str,
    placeholder: &'static str,
    on_input: fn(String) -> Message,
) -> Element<'a, Message> {
    row![
        text(label).size(14.0).width(Length::Fixed(120.0)),
        text_input(placeholder, value).on_input(on_input).size(14.0),
    ]
    .spacing(8)
    .into()
}

#[derive(Debug)]
pub(crate) struct Telemetry {
    pub(crate) usage: f32,
}

impl canvas::Program<Message> for Telemetry {
    type State = ();

    fn draw(
        &self,
        _state: &(),
        renderer: &iced::Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: iced::mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let mut frame = canvas::Frame::new(renderer, bounds.size());
        let track = canvas::Path::rectangle(iced::Point::new(0.0, 0.0), bounds.size());
        frame.fill(&track, theme::CARD_ALT);
        let bar = canvas::Path::rectangle(
            iced::Point::new(0.0, 0.0),
            Size::new(bounds.width * self.usage.clamp(0.0, 1.0), bounds.height),
        );
        frame.fill(&bar, theme::ACCENT);
        vec![frame.into_geometry()]
    }
}
