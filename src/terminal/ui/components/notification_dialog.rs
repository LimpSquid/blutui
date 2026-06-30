use ratatui::layout::{Alignment, Constraint, Direction, Layout};
use ratatui::style::Stylize;
use ratatui::text::Text;
use ratatui::widgets::{Block, BorderType, Paragraph, Widget, WidgetRef, Wrap};

use super::{DialogComponent, Keybindings, prelude::*};

#[derive(Debug)]
pub struct NotificationDialog {
    message: String,
    stylesheet: Stylesheet,
}

impl NotificationDialog {
    pub fn new<M: Into<String>>(message: M, stylesheet: Stylesheet) -> Self {
        Self {
            message: message.into(),
            stylesheet,
        }
    }
}

impl DialogComponent for NotificationDialog {
    fn on_key_press(&mut self, code: KeyCode, _modifiers: KeyModifiers) -> Option<DialogEvent> {
        match code {
            KeyCode::Esc => Some(DialogEvent::Closed),
            _ => None,
        }
    }
}

impl WidgetRef for NotificationDialog {
    fn render_ref(&self, area: Rect, buf: &mut Buffer) {
        let dialog = Popup::new()
            // TODO: choose based on message length
            .constraints([Constraint::Length(60), Constraint::Length(20)])
            .block(
                Block::bordered()
                    .border_type(BorderType::Thick)
                    .bg(self.stylesheet.popup_background_color),
            );
        let message = Paragraph::new(
            Text::from(self.message.as_str())
                .fg(self.stylesheet.text_color)
                .bold(),
        )
        .wrap(Wrap { trim: false })
        .alignment(Alignment::Center);
        let keybindings = Keybindings::new(&[("ESC", "Close")], self.stylesheet);

        let [_, body, footer] = dialog.drawable_area(area).layout(&Layout::new(
            Direction::Vertical,
            [Constraint::Max(1), Constraint::Fill(1), Constraint::Max(1)],
        ));

        dialog.render(area, buf);
        message.render(body, buf);
        keybindings.render(footer, buf);
    }
}
