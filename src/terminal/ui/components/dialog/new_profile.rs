use ratatui::layout::{Alignment, Constraint, Direction, Layout};
use ratatui::style::{Style, Stylize};
use ratatui::text::Line;
use ratatui::widgets::{Block, BorderType, List, ListState, StatefulWidget, Widget, WidgetRef};
use strum::IntoEnumIterator;

use super::prelude::*;
use crate::profile::validate_profile_name;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, strum_macros::EnumIter)]
enum ProfileType {
    #[default]
    Group,
    MultiplayerGroup,
}

impl ProfileType {
    fn list_text(&self) -> &'static str {
        match self {
            Self::Group => "group",
            Self::MultiplayerGroup => "multi-player group",
        }
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum WizardStep {
    #[default]
    ChooseProfileType,
    ChooseProfileName,
}

impl WizardStep {
    fn next(&self) -> Self {
        match self {
            Self::ChooseProfileType => Self::ChooseProfileName,
            Self::ChooseProfileName => Self::ChooseProfileName,
        }
    }

    fn header(&self) -> &'static str {
        match self {
            Self::ChooseProfileType => "choose a profile type",
            Self::ChooseProfileName => "choose a profile name",
        }
    }
}

#[derive(Debug)]
pub struct NewProfileDialog {
    step: WizardStep,
    selected_profile_type: ProfileType,
    profile_name_input: TextFieldState,
    stylesheet: Stylesheet,
}

impl NewProfileDialog {
    pub fn new(stylesheet: Stylesheet) -> Self {
        Self {
            step: Default::default(),
            selected_profile_type: Default::default(),
            profile_name_input: TextFieldState::new(),
            stylesheet,
        }
    }

    fn render_choose_profile_type(&self, area: Rect, buf: &mut Buffer) {
        let [body, footer] = area.layout(
            &Layout::new(
                Direction::Vertical,
                [Constraint::Fill(1), Constraint::Length(1)],
            )
            .margin(1),
        );

        let mut selected = None;
        let list = ProfileType::iter()
            .enumerate()
            .map(|(index, ty)| {
                if ty == self.selected_profile_type {
                    selected = Some(index);
                }

                ty.list_text()
            })
            .collect::<List>()
            .highlight_style(Style::new().bg(self.stylesheet.accent_color_dark).bold());

        StatefulWidget::render(
            list,
            body,
            buf,
            &mut ListState::default().with_selected(selected),
        );

        let keybindings = Keybindings::new(
            &[("🡳/🡱", "Selection"), ("ESC", "Close"), ("ENTER", "Next")],
            self.stylesheet,
        );
        keybindings.render(footer, buf);
    }

    fn render_choose_profile_name(&self, area: Rect, buf: &mut Buffer) {
        let [body, footer] = area.layout(&Layout::new(
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

        let input = TextField::with_label("profile name")
            .block(
                Block::bordered()
                    .border_type(BorderType::Thick)
                    .fg(self.stylesheet.highlight_color),
            )
            .label_style(Style::new().fg(self.stylesheet.text_color_sub))
            .text_style(Style::new().fg(self.stylesheet.text_color));
        input.render(input_area, buf, &mut self.profile_name_input.clone());

        let (hint, keybindings) =
            if let Err(error) = validate_profile_name(self.profile_name_input.value()) {
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
        hint.render(hint_area, buf);
        keybindings.render(footer, buf);
    }
}

impl DialogComponent for NewProfileDialog {
    fn on_key_press(&mut self, code: KeyCode, _modifiers: KeyModifiers) -> Option<DialogEvent> {
        match (self.step, code) {
            (_, KeyCode::Esc) => Some(DialogEvent::Closed),

            // Choose profile type
            (WizardStep::ChooseProfileType, KeyCode::Enter) => {
                self.step = self.step.next();
                None
            }
            (WizardStep::ChooseProfileType, KeyCode::Up) => {
                self.selected_profile_type =
                    select_next(ProfileType::iter(), |t| t, Some(self.selected_profile_type))
                        .unwrap_or_default();
                None
            }
            (WizardStep::ChooseProfileType, KeyCode::Down) => {
                self.selected_profile_type =
                    select_previous(ProfileType::iter(), |t| t, Some(self.selected_profile_type))
                        .unwrap_or_default();
                None
            }

            // Choose profile name
            (WizardStep::ChooseProfileName, KeyCode::Enter) => {
                validate_profile_name(self.profile_name_input.value())
                    .ok()
                    .map(|_| {
                        DialogEvent::Submitted(vec![UserAction::NewProfile(
                            self.profile_name_input.value().to_owned(),
                        )])
                    })
            }
            (WizardStep::ChooseProfileName, code) => {
                self.profile_name_input.on_key_press(code);
                None
            }
            _ => None,
        }
    }
}

impl WidgetRef for NewProfileDialog {
    fn render_ref(&self, area: Rect, buf: &mut Buffer) {
        let dialog = Popup::with_title(&format!("Create a new profile - {}", self.step.header()))
            .constraints([Constraint::Ratio(2, 3), Constraint::Ratio(2, 3)])
            .block(
                Block::bordered()
                    .border_type(BorderType::Thick)
                    .bg(self.stylesheet.popup_background_color),
            );
        let drawable_area = dialog.drawable_area(area);
        dialog.render(area, buf);

        match self.step {
            WizardStep::ChooseProfileType => self.render_choose_profile_type(drawable_area, buf),
            WizardStep::ChooseProfileName => self.render_choose_profile_name(drawable_area, buf),
        }
    }
}
