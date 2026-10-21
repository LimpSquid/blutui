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
    fn on_key_press(
        &mut self,
        code: KeyCode,
        _: KeyModifiers,
        _: &AppState,
    ) -> Option<DialogEvent> {
        match code {
            KeyCode::Esc => Some(DialogEvent::Closed),
            _ => None,
        }
    }
}

impl Component for NotificationDialog {
    fn render(&self, area: Rect, ctx: &mut ComponentContext<'_>) {
        let dialog = Popup::with_title("Notification")
            .constraints([Constraint::Ratio(1, 3), Constraint::Ratio(1, 3)])
            .block(
                Block::bordered()
                    .border_type(BorderType::Thick)
                    .bg(ctx.stylesheet.popup_background_color),
            );
        let [body, footer] = dialog.drawable_area(area).layout(
            &Layout::new(
                Direction::Vertical,
                [Constraint::Fill(1), Constraint::Length(1)],
            )
            .margin(1),
        );
        dialog.render(area, ctx.buffer);

        let message = Paragraph::new(
            Text::from(self.message.as_str())
                .fg(ctx.stylesheet.text_color)
                .bold(),
        )
        .wrap(Wrap { trim: false })
        .centered();
        message.render(body, ctx.buffer);

        Keybindings::new(&[("ESC", "Close")]).render(footer, ctx);
    }
}
