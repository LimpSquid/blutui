use strum::EnumCount;

use super::{Ui, UserAction, components::*, render::*, utils::*};
use crate::terminal::app::{AppState, BusyFlags};

#[derive(Debug, Clone, Copy)]
pub enum KeyCode {
    Esc,
    Up,
    Down,
    Left,
    Right,
    Tab,
    Home,
    End,
    Enter,
    Backspace,
    Char(char),
}

#[allow(unused)]
#[derive(Debug, Clone, Copy)]
pub struct KeyModifiers {
    pub ctrl: bool,
    pub shift: bool,
}

#[derive(Debug, Clone)]
pub enum UserEvent {
    Key(KeyCode, KeyModifiers),
    FocusGained,
}

pub fn user_event(event: UserEvent, state: &AppState, ui: &mut Ui) {
    use Tab::*;
    use WindowFocus::*;

    match event {
        UserEvent::FocusGained => ui.action(UserAction::RefreshDevices),
        UserEvent::Key(code, modifiers) => match (ui.window_focus, code) {
            // Re-route input to dialog and handle event
            (_, _) if ui.active_dialog.is_some() => {
                match ui
                    .active_dialog
                    .as_mut()
                    .expect("Uhoh, you've escaped the matrix")
                    .as_mut()
                    .on_key_press(code, modifiers)
                {
                    Some(DialogEvent::Actions(actions)) => ui.actions(actions),
                    Some(DialogEvent::Submitted(actions)) => {
                        ui.actions(actions);
                        ui.active_dialog = None;
                    }
                    Some(DialogEvent::Closed) => ui.active_dialog = None,
                    None => {}
                }
            }
            // Quit app
            (_, KeyCode::Char('q' | 'Q')) => ui.quit(),
            // Cycle window focus
            (_, KeyCode::Char(' ')) => {
                ui.window_focus = (((ui.window_focus as usize)
                    .checked_add(1)
                    .unwrap_or_default())
                    % WindowFocus::COUNT)
                    .into();
            }

            // Cycle selected tab
            (Tabs, KeyCode::Tab) => {
                ui.selected_tab = (((ui.selected_tab as usize)
                    .checked_add(1)
                    .unwrap_or_default())
                    % Tab::COUNT)
                    .into();
            }

            (Tabs, KeyCode::Down) if ui.selected_tab == Profile => {
                ui.selected_profile = select_next(
                    state.sorted_profiles_iter(),
                    |(id, _)| id.to_owned(),
                    ui.selected_profile.clone(),
                );
            }
            (Tabs, KeyCode::Up) if ui.selected_tab == Profile => {
                ui.selected_profile = select_previous(
                    state.sorted_profiles_iter(),
                    |(id, _)| id.to_owned(),
                    ui.selected_profile.clone(),
                );
            }
            (Tabs, KeyCode::Home) if ui.selected_tab == Profile => {
                ui.selected_profile =
                    select_first(state.sorted_profiles_iter(), |(id, _)| id.to_owned());
            }
            (Tabs, KeyCode::End) if ui.selected_tab == Profile => {
                ui.selected_profile =
                    select_last(state.sorted_profiles_iter(), |(id, _)| id.to_owned());
            }
            (Tabs, KeyCode::Enter) if ui.selected_tab == Profile => {
                if !state.busy_flags.contains(BusyFlags::PROFILE_TRANSITIONING)
                    && let Some(profile_id) = ui.selected_profile.clone()
                {
                    ui.action(UserAction::ApplyProfile(profile_id));
                }
            }
            (Tabs, KeyCode::Char('n' | 'N')) if ui.selected_tab == Profile => {
                ui.open_dialog(NewProfileDialog::new(ui.stylesheet));
            }
            (Tabs, KeyCode::Char('d' | 'D')) if ui.selected_tab == Profile => {
                if let Some(profile) = ui
                    .selected_profile
                    .as_ref()
                    .and_then(|profile_id| state.find_profile(profile_id))
                {
                    ui.open_dialog(DeleteProfileDialog::new(profile.to_owned(), ui.stylesheet));
                }
            }
            (Tabs, KeyCode::Char('e' | 'E')) if ui.selected_tab == Profile => {
                if let Some(profile_id) = ui.selected_profile.clone() {
                    ui.action(UserAction::EditProfile(profile_id));
                }
            }

            (DiscoveredDevices, KeyCode::Char('r' | 'R')) => ui.action(UserAction::RefreshDevices),
            (DiscoveredDevices, KeyCode::Char('l' | 'L')) => {
                if let Some(device) = ui
                    .selected_device
                    .and_then(|id| state.device_state.get(&id))
                {
                    if !modifiers.ctrl
                        && let Some(group_id) =
                            device.group_status.as_ref().and_then(|s| s.group_id())
                    {
                        ui.action(UserAction::GroupVolumeUp(group_id));
                    } else {
                        ui.action(UserAction::DeviceVolumeUp(device.device.id));
                    }
                }
            }
            (DiscoveredDevices, KeyCode::Char('j' | 'J')) => {
                if let Some(device) = ui
                    .selected_device
                    .and_then(|id| state.device_state.get(&id))
                {
                    if !modifiers.ctrl
                        && let Some(group_id) =
                            device.group_status.as_ref().and_then(|s| s.group_id())
                    {
                        ui.action(UserAction::GroupVolumeDown(group_id));
                    } else {
                        ui.action(UserAction::DeviceVolumeDown(device.device.id));
                    }
                }
            }
            (DiscoveredDevices, KeyCode::Char('p' | 'P')) => {
                if let Some(device_id) = ui.selected_device {
                    ui.action(UserAction::TogglePausePlay(device_id));
                }
            }
            (DiscoveredDevices, KeyCode::Char('n' | 'N')) => {
                if let Some(device_id) = ui.selected_device {
                    ui.action(UserAction::Skip(device_id));
                }
            }
            (DiscoveredDevices, KeyCode::Char('b' | 'B')) => {
                if let Some(device_id) = ui.selected_device {
                    ui.action(UserAction::Back(device_id));
                }
            }
            (DiscoveredDevices, KeyCode::Down) => {
                ui.selected_device = select_next(
                    state.sorted_device_state_iter(),
                    |(id, _)| *id,
                    ui.selected_device,
                );
            }
            (DiscoveredDevices, KeyCode::Up) => {
                ui.selected_device = select_previous(
                    state.sorted_device_state_iter(),
                    |(id, _)| *id,
                    ui.selected_device,
                );
            }
            (DiscoveredDevices, KeyCode::Home) => {
                ui.selected_device = select_first(state.sorted_device_state_iter(), |(id, _)| *id);
            }
            (DiscoveredDevices, KeyCode::End) => {
                ui.selected_device = select_last(state.sorted_device_state_iter(), |(id, _)| *id);
            }
            _ => {}
        },
    };
}
