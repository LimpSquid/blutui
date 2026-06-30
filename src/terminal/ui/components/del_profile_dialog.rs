use ratatui::layout::{Alignment, Constraint, Direction, Layout};
use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::widgets::{Block, BorderType, Paragraph, Widget, WidgetRef, Wrap};

use super::{DialogComponent, Keybindings, prelude::*};
use crate::profman::StoredProfile;

#[derive(Debug)]
pub struct DeleteProfileDialog {
    profile: StoredProfile,
    stylesheet: Stylesheet,
}

impl DeleteProfileDialog {
    pub fn new(profile: StoredProfile, stylesheet: Stylesheet) -> Self {
        Self {
            profile,
            stylesheet,
        }
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

impl WidgetRef for DeleteProfileDialog {
    fn render_ref(&self, area: Rect, buf: &mut Buffer) {
        let dialog = Popup::with_title("Delete a profile")
            .constraints([Constraint::Length(60), Constraint::Length(10)])
            .block(
                Block::bordered()
                    .border_type(BorderType::Thick)
                    .bg(self.stylesheet.popup_background_color),
            );
        let message = Paragraph::new(
            Line::from(format!(
                "Are you sure you want to delete '{}'?",
                self.profile.name()
            ))
            .fg(self.stylesheet.error_color)
            .bold(),
        )
        .wrap(Wrap { trim: false })
        .alignment(Alignment::Center);
        let keybindings = Keybindings::new(&[("ESC", "No"), ("ENTER", "Yes")], self.stylesheet);

        let [_, body, footer] = dialog.drawable_area(area).layout(&Layout::new(
            Direction::Vertical,
            [
                Constraint::Max(1),
                Constraint::Fill(1),
                Constraint::Length(1),
            ],
        ));

        dialog.render(area, buf);
        message.render(body, buf);
        keybindings.render(footer, buf);
    }
}
