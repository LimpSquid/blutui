use super::prelude::*;

#[derive(Debug)]
pub struct NotificationDialog {
    message: String,
}

impl NotificationDialog {
    pub fn new<M: Into<String>>(message: M) -> Self {
        Self {
            message: message.into(),
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

impl Component for NotificationDialog {
    fn render(&self, area: Rect, ctx: &mut ComponentContext<'_>) {
        let dialog = Popup::new()
            // TODO: choose based on message length
            .constraints([Constraint::Length(60), Constraint::Length(20)])
            .block(
                Block::bordered()
                    .border_type(BorderType::Thick)
                    .bg(ctx.stylesheet.popup_background_color),
            );
        let message = Paragraph::new(
            Text::from(self.message.as_str())
                .fg(ctx.stylesheet.text_color)
                .bold(),
        )
        .wrap(Wrap { trim: false })
        .alignment(Alignment::Center);
        let keybindings = Keybindings::new(&[("ESC", "Close")]);

        let [_, body, footer] = dialog.drawable_area(area).layout(&Layout::new(
            Direction::Vertical,
            [Constraint::Max(1), Constraint::Fill(1), Constraint::Max(1)],
        ));

        dialog.render(area, ctx.buffer);
        message.render(body, ctx.buffer);
        keybindings.render(footer, ctx);
    }
}
