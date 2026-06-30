use ratatui::layout::{Alignment, Constraint, Direction, Layout};
use ratatui::style::{Style, Stylize};
use ratatui::text::Line;
use ratatui::widgets::{Block, BorderType, StatefulWidget, Widget, WidgetRef};

use super::{DialogComponent, Keybindings, prelude::*};
use crate::profman::validate_profile_name;

#[derive(Debug)]
pub struct NewProfileDialog {
    input: TextFieldState,
    stylesheet: Stylesheet,
}

impl NewProfileDialog {
    pub fn new(stylesheet: Stylesheet) -> Self {
        Self {
            input: TextFieldState::new(),
            stylesheet,
        }
    }
}

impl DialogComponent for NewProfileDialog {
    fn on_key_press(&mut self, code: KeyCode, _modifiers: KeyModifiers) -> Option<DialogEvent> {
        match code {
            KeyCode::Esc => Some(DialogEvent::Closed),
            KeyCode::Enter if validate_profile_name(self.input.value()).is_ok() => Some(
                DialogEvent::Submitted(vec![UserAction::NewProfile(self.input.value().to_owned())]),
            ),
            KeyCode::Enter => None,
            code => {
                self.input.on_key_press(code);
                None
            }
        }
    }
}

impl WidgetRef for NewProfileDialog {
    fn render_ref(&self, area: Rect, buf: &mut Buffer) {
        let dialog = Popup::with_title("Create a new profile")
            .constraints([Constraint::Length(60), Constraint::Length(10)])
            .block(
                Block::bordered()
                    .border_type(BorderType::Thick)
                    .bg(self.stylesheet.popup_background_color),
            );
        let input = TextField::with_label("profile name")
            .block(
                Block::bordered()
                    .border_type(BorderType::Thick)
                    .fg(self.stylesheet.highlight_color),
            )
            .label_style(Style::new().fg(self.stylesheet.text_color_sub))
            .text_style(Style::new().fg(self.stylesheet.text_color));
        let (hint, keybindings) = if let Err(error) = validate_profile_name(self.input.value()) {
            (
                Line::from(format!("{error}"))
                    .fg(self.stylesheet.error_color)
                    .bold()
                    .alignment(Alignment::Center),
                vec![("ESC", "Close")],
            )
        } else {
            (
                Line::from("profile name OK")
                    .fg(self.stylesheet.success_color)
                    .alignment(Alignment::Center),
                vec![("ESC", "Close"), ("ENTER", "Confirm")],
            )
        };
        let keybindings = Keybindings::new(&keybindings, self.stylesheet);

        let [body, footer] = dialog.drawable_area(area).layout(&Layout::new(
            Direction::Vertical,
            [Constraint::Fill(1), Constraint::Length(1)],
        ));
        let [input_area, hint_area] = body.layout(
            &Layout::new(
                Direction::Vertical,
                [Constraint::Length(3), Constraint::Length(1)],
            )
            .spacing(1)
            .margin(1),
        );

        dialog.render(area, buf);
        input.render(input_area, buf, &mut self.input.clone());
        hint.render(hint_area, buf);
        keybindings.render(footer, buf);
    }
}
