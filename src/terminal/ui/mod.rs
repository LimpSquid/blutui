mod components;
mod event;
mod render;
mod stylesheet;
mod utils;
mod widgets;

use std::time::Instant;

pub use event::{KeyCode, KeyModifiers, UserEvent, user_event};
pub use render::{after_render, before_render, render};

use crate::profman::StoredProfile;
use crate::terminal::ui::components::BoxedComponent;
use crate::types::{DeviceId, GroupId, ProfileId};

#[derive(Debug, Clone)]
#[non_exhaustive]
#[allow(unused)]
pub enum UserAction {
    RefreshDevices,
    DeviceVolumeUp(DeviceId),
    DeviceVolumeDown(DeviceId),
    GroupVolumeUp(GroupId),
    GroupVolumeDown(GroupId),
    TogglePausePlay(DeviceId),
    Skip(DeviceId),
    Back(DeviceId),
    Mute(DeviceId),
    Unmute(DeviceId),
    ApplyProfile(ProfileId),
    EditProfile(ProfileId),
    NewProfile(String),
    DeleteProfile(StoredProfile),
}

pub struct Ui {
    pub(super) should_quit: bool,
    pub(super) pending_actions: Vec<UserAction>,

    active_dialog: Option<Box<dyn components::DialogComponent>>,
    selected_device: Option<DeviceId>,
    selected_profile: Option<ProfileId>,
    selected_tab: render::Tab,
    window_focus: render::WindowFocus,
    render_start: Instant,
    stylesheet: stylesheet::Stylesheet,
}

impl Ui {
    pub fn show_notification<M: Into<String>>(&mut self, message: M) {
        self.open_dialog(components::NotificationDialog::new(
            message,
            self.stylesheet,
        ));
    }

    fn open_dialog<D: components::DialogComponent + 'static>(&mut self, dialog: D) {
        if self.active_dialog.is_some() {
            return;
        }

        self.active_dialog = Some(dialog.boxed())
    }

    fn quit(&mut self) {
        self.should_quit = true;
    }

    fn action(&mut self, action: UserAction) {
        self.pending_actions.push(action);
    }

    fn actions<I: IntoIterator<Item = UserAction>>(&mut self, iter: I) {
        self.pending_actions.extend(iter);
    }
}

impl Default for Ui {
    fn default() -> Self {
        Self {
            should_quit: false,
            pending_actions: vec![],
            active_dialog: None,
            selected_device: None,
            selected_profile: None,
            selected_tab: Default::default(),
            window_focus: Default::default(),
            stylesheet: Default::default(),
            render_start: Instant::now(),
        }
    }
}
