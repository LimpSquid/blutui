use super::prelude::*;
use crate::profile::StoredProfile;

#[derive(Debug)]
pub struct DeleteProfileDialog {
    profile: StoredProfile,
}

impl DeleteProfileDialog {
    pub fn new(profile: StoredProfile) -> Self {
        Self { profile }
    }
}

impl DialogComponent for DeleteProfileDialog {
    fn on_key_press(&mut self, code: KeyCode, _modifiers: KeyModifiers) -> Option<DialogEvent> {
        match code {
            KeyCode::Esc => Some(DialogEvent::Closed),
            KeyCode::Enter => Some(DialogEvent::Submitted(vec![UserAction::DeleteProfile(
                self.profile.clone(),
            )])),
            _ => None,
        }
    }
}

impl Component for DeleteProfileDialog {
    fn render(&self, area: Rect, ctx: &mut ComponentContext<'_>) {
        let dialog = Popup::with_title("Delete a profile")
            .constraints([Constraint::Length(60), Constraint::Length(10)])
            .block(
                Block::bordered()
                    .border_type(BorderType::Thick)
                    .bg(ctx.stylesheet.popup_background_color),
            );
        let message = Paragraph::new(
            Line::from(format!(
                "Are you sure you want to delete '{}'?",
                self.profile.name()
            ))
            .fg(ctx.stylesheet.error_color)
            .bold(),
        )
        .wrap(Wrap { trim: false })
        .alignment(Alignment::Center);
        let keybindings = Keybindings::new(&[("ESC", "No"), ("ENTER", "Yes")]);

        let [_, body, footer] = dialog.drawable_area(area).layout(&Layout::new(
            Direction::Vertical,
            [
                Constraint::Max(1),
                Constraint::Fill(1),
                Constraint::Length(1),
            ],
        ));

        dialog.render(area, ctx.buffer);
        message.render(body, ctx.buffer);
        keybindings.render(footer, ctx);
    }
}
