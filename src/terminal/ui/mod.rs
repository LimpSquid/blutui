mod components;
mod event;
mod render;
mod theme;
mod utils;
mod widgets;

use std::collections::VecDeque;
use std::sync::Arc;
use std::time::Instant;

pub use event::{KeyCode, KeyModifiers, UserEvent, user_event};
pub use render::{after_render, before_render, render};
use tokio::sync::Notify;

use self::components::BoxedComponent;
use self::components::dialog::{DialogComponent, NotificationDialog};
use crate::profile::StoredProfile;
use crate::types::{DeviceId, GroupId, ProfileId};

#[derive(Debug, Clone)]
#[non_exhaustive]
#[allow(unused, clippy::large_enum_variant)]
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
    UngroupAll,
}

#[derive(Debug, Clone, Default)]
pub struct Redraw {
    notify: Arc<Notify>,
}

impl Redraw {
    #[allow(unused)]
    pub fn redraw_ui(&self) {
        self.notify.notify_one();
    }

    pub async fn wait(&self) {
        self.notify.notified().await;
    }
}

pub struct Ui {
    pub(super) should_quit: bool,
    pub(super) pending_actions: Vec<UserAction>,
    pub(super) redraw: Redraw,

    active_dialogs: VecDeque<Box<dyn DialogComponent>>,
    selected_device: Option<DeviceId>,
    selected_profile: Option<ProfileId>,
    selected_tab: render::Tab,
    window_focus: render::WindowFocus,
    render_start: Instant,
    stylesheet: theme::Stylesheet,
    #[cfg(feature = "ui-enable-image")]
    music_image: widgets::ImageState,
}

impl Ui {
    pub fn query_for_graphics_capabilities(&mut self) {
        #[cfg(feature = "ui-enable-image")]
        {
            if let Ok(picker) = ratatui_image::picker::Picker::from_query_stdio() {
                self.music_image.set_picker(picker);
            } else {
                tracing::warn!("failed to query image capabilities")
            }
        }
    }

    pub fn show_notification<M: Into<String>>(&mut self, message: M) {
        self.open_dialog(NotificationDialog::new(message));
    }

    fn open_dialog<D: DialogComponent + 'static>(&mut self, dialog: D) {
        self.active_dialogs.push_front(dialog.boxed())
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
        let redraw = Redraw::default();

        Self {
            should_quit: false,
            pending_actions: Default::default(),
            redraw: redraw.clone(),
            active_dialogs: Default::default(),
            selected_device: None,
            selected_profile: None,
            selected_tab: Default::default(),
            window_focus: Default::default(),
            stylesheet: Default::default(),
            render_start: Instant::now(),
            #[cfg(feature = "ui-enable-image")]
            music_image: widgets::ImageState::new(redraw),
        }
    }
}
