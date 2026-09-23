use strum::IntoEnumIterator;

use super::prelude::*;
use crate::profile::validate_profile_name;
use crate::terminal::app::DeviceState;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, strum_macros::EnumIter)]
enum ProfileType {
    #[default]
    SingleDevice,
    NormalGroup,
    MultiplayerGroup,
    HomeTheaterGroup,
}

impl ProfileType {
    fn list_text(&self) -> &'static str {
        match self {
            Self::SingleDevice => "Single Device",
            Self::NormalGroup => "Normal Group",
            Self::MultiplayerGroup => "Multi Player Group",
            Self::HomeTheaterGroup => "Home Cinema Group",
        }
    }

    fn description_text(&self) -> &'static str {
        match self {
            Self::SingleDevice => "A single BluOS player.",
            Self::NormalGroup => {
                "A standard group of BluOS players that can be controlled individually."
            }
            Self::MultiplayerGroup => "Multiple BluOS players playing the same audio in sync.",
            Self::HomeTheaterGroup => {
                "Multiple BluOS players in a home-theater surround sound setup."
            }
        }
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum WizardStep {
    #[default]
    ChooseProfileType,
    AvailableDevices,
    ChooseProfileName,
}

impl WizardStep {
    fn next(&self) -> Self {
        match self {
            Self::ChooseProfileType => Self::AvailableDevices,
            Self::AvailableDevices => Self::ChooseProfileName,
            Self::ChooseProfileName => Self::ChooseProfileName,
        }
    }

    fn header(&self) -> &'static str {
        match self {
            Self::ChooseProfileType => "choose a profile type",
            Self::AvailableDevices => "available devices",
            Self::ChooseProfileName => "choose a profile name",
        }
    }
}

#[derive(Debug)]
pub struct NewProfileDialog {
    step: WizardStep,
    selected_profile_type: ProfileType,
    profile_name_input: TextFieldState,
}

impl NewProfileDialog {
    pub fn new() -> Self {
        Self {
            step: Default::default(),
            selected_profile_type: Default::default(),
            profile_name_input: TextFieldState::new(),
        }
    }

    fn render_choose_profile_type(&self, area: Rect, ctx: &mut ComponentContext<'_>) {
        let [body, footer] = area.layout(
            &Layout::new(
                Direction::Vertical,
                [Constraint::Fill(1), Constraint::Length(1)],
            )
            .margin(1),
        );
        let [list_area, description_area] = body.layout(
            &Layout::new(
                Direction::Vertical,
                [
                    Constraint::Length(ProfileType::iter().len() as u16),
                    Constraint::Fill(1),
                ],
            )
            .spacing(1)
            .margin(1),
        );

        let mut selected = None;
        let list = ProfileType::iter()
            .enumerate()
            .map(|(index, ty)| {
                if ty == self.selected_profile_type {
                    selected = Some(index);
                }

                ty.list_text().fg(ctx.stylesheet.text_color)
            })
            .collect::<List>()
            .highlight_symbol("> ".fg(ctx.stylesheet.highlight_color).bold())
            .highlight_style(Style::new().bg(ctx.stylesheet.accent_color_dark).bold());
        StatefulWidget::render(
            list,
            list_area,
            ctx.buffer,
            &mut ListState::default().with_selected(selected),
        );

        let description = Paragraph::new(
            self.selected_profile_type
                .description_text()
                .fg(ctx.stylesheet.text_color_sub),
        )
        .wrap(Wrap { trim: false })
        .alignment(Alignment::Center);
        description.render(description_area, ctx.buffer);

        let keybindings =
            Keybindings::new(&[("🡳/🡱", "Selection"), ("ESC", "Close"), ("ENTER", "Next")]);
        keybindings.render(footer, ctx);
    }

    fn render_available_devices(&self, area: Rect, ctx: &mut ComponentContext<'_>) {
        let [body, footer] = area.layout(
            &Layout::new(
                Direction::Vertical,
                [Constraint::Fill(1), Constraint::Length(1)],
            )
            .margin(1),
        );

        if ctx.state.device_state.is_empty() {
            let text = Line::from(
                if ctx.state.device_state.is_empty() {
                    "Detecting devices... ⏳"
                } else {
                    "Select a device."
                }
                .fg(ctx.stylesheet.accent_color),
            );
            let area = body.centered(
                Constraint::Length(text.width() as u16),
                Constraint::Length(1),
            );
            let description = Paragraph::new(text).wrap(Wrap { trim: false });
            description.render(area, ctx.buffer);
        } else {
            let list = ctx
                .state
                .sorted_device_state_iter()
                .map(
                    |(
                        _,
                        DeviceState {
                            device,
                            group_status,
                            ..
                        },
                    )| {
                        let device_name = group_status
                            .as_ref()
                            .and_then(|s| s.name.clone())
                            .unwrap_or_else(|| {
                                device
                                    .attributes
                                    .first()
                                    .and_then(|a| a.fields.get("name").cloned())
                                    .unwrap_or_else(|| device.id.to_string())
                            });
                        let device_model = group_status
                            .as_ref()
                            .map(|s| s.model.clone())
                            .unwrap_or_else(|| {
                                device
                                    .attributes
                                    .iter()
                                    .flat_map(|a| a.fields.iter())
                                    .find(|(k, _)| *k == "model")
                                    .map(|(_, v)| v.to_owned())
                                    .unwrap_or("N/A".to_string())
                            });

                        vec![Line::from(vec![
                            device_name.to_string().fg(ctx.stylesheet.text_color),
                            format!(" ({device_model})").fg(ctx.stylesheet.text_color_sub),
                            if group_status.as_ref().is_some_and(|s| s.am_i_grouped()) {
                                " - grouped".fg(ctx.stylesheet.text_color_sub)
                            } else {
                                "".into()
                            },
                        ])]
                    },
                )
                .collect::<List>()
                .highlight_style(Style::new().bg(ctx.stylesheet.accent_color_dark).bold());
            Widget::render(list, body, ctx.buffer);
        }

        let keybindings = Keybindings::new(&[
            ("u", "Ungroup all devices"),
            ("ESC", "Close"),
            ("ENTER", "Next"),
        ]);
        keybindings.render(footer, ctx);
    }

    fn render_choose_profile_name(&self, area: Rect, ctx: &mut ComponentContext<'_>) {
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
                    .fg(ctx.stylesheet.highlight_color),
            )
            .label_style(Style::new().fg(ctx.stylesheet.text_color_sub))
            .text_style(Style::new().fg(ctx.stylesheet.text_color));
        input.render(input_area, ctx.buffer, &mut self.profile_name_input.clone());

        let (hint, keybindings) =
            if let Err(error) = validate_profile_name(self.profile_name_input.value()) {
                (
                    Line::from(format!("{error}"))
                        .fg(ctx.stylesheet.error_color)
                        .bold()
                        .alignment(Alignment::Center),
                    vec![("ESC", "Close")],
                )
            } else {
                (
                    Line::from("profile name OK")
                        .fg(ctx.stylesheet.success_color)
                        .alignment(Alignment::Center),
                    vec![("ESC", "Close"), ("ENTER", "Confirm")],
                )
            };
        let keybindings = Keybindings::new(&keybindings);
        hint.render(hint_area, ctx.buffer);
        keybindings.render(footer, ctx);
    }
}

impl DialogComponent for NewProfileDialog {
    fn on_key_press(&mut self, code: KeyCode, modifiers: KeyModifiers) -> Option<DialogEvent> {
        match (self.step, code) {
            (_, KeyCode::Esc) => Some(DialogEvent::Closed),

            // Choose profile type
            (WizardStep::ChooseProfileType, KeyCode::Enter) => {
                self.step = self.step.next();
                None
            }
            (WizardStep::ChooseProfileType, KeyCode::Up) => {
                self.selected_profile_type =
                    select_previous(ProfileType::iter(), |t| t, Some(self.selected_profile_type))
                        .unwrap_or_default();
                None
            }
            (WizardStep::ChooseProfileType, KeyCode::Down) => {
                self.selected_profile_type =
                    select_next(ProfileType::iter(), |t| t, Some(self.selected_profile_type))
                        .unwrap_or_default();
                None
            }

            // Available devices
            (WizardStep::AvailableDevices, KeyCode::Enter) => {
                self.step = self.step.next();
                None
            }
            (WizardStep::AvailableDevices, KeyCode::Char('u') | KeyCode::Char('U')) => {
                Some(DialogEvent::Actions(vec![UserAction::UngroupAll]))
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
                self.profile_name_input.on_key_press(code, modifiers);
                None
            }

            _ => None,
        }
    }
}

impl Component for NewProfileDialog {
    fn render(&self, area: Rect, ctx: &mut ComponentContext<'_>) {
        let dialog = Popup::with_title(&format!("Create a new profile - {}", self.step.header()))
            .constraints([Constraint::Ratio(1, 2), Constraint::Ratio(1, 2)])
            .block(
                Block::bordered()
                    .border_type(BorderType::Thick)
                    .bg(ctx.stylesheet.popup_background_color),
            );
        let drawable_area = dialog.drawable_area(area);
        dialog.render(area, ctx.buffer);

        match self.step {
            WizardStep::ChooseProfileType => self.render_choose_profile_type(drawable_area, ctx),
            WizardStep::AvailableDevices => self.render_available_devices(drawable_area, ctx),
            WizardStep::ChooseProfileName => self.render_choose_profile_name(drawable_area, ctx),
        }
    }
}
