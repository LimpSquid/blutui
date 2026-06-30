mod prelude {
    pub use super::super::UserAction;
    pub use super::super::event::{KeyCode, KeyModifiers};
    pub use super::super::{stylesheet::*, widgets::*};
    pub use super::DialogEvent;
    pub use ratatui::buffer::Buffer;
    pub use ratatui::layout::Rect;
    pub use ratatui::widgets::WidgetRef;
}

mod del_profile_dialog;
mod keybindings;
mod new_profile_dialog;
mod notification_dialog;

pub use del_profile_dialog::DeleteProfileDialog;
pub use keybindings::Keybindings;
pub use new_profile_dialog::NewProfileDialog;
pub use notification_dialog::NotificationDialog;

#[allow(unused)]
#[derive(Debug, Clone)]
pub enum DialogEvent {
    Actions(Vec<prelude::UserAction>),
    Submitted(Vec<prelude::UserAction>),
    Closed,
}

pub trait DialogComponent: prelude::WidgetRef {
    fn on_key_press(
        &mut self,
        code: prelude::KeyCode,
        modifiers: prelude::KeyModifiers,
    ) -> Option<DialogEvent>;
}

pub trait BoxedComponent: Sized {
    fn boxed(self) -> Box<Self> {
        Box::new(self)
    }
}
impl<T: prelude::WidgetRef> BoxedComponent for T {}
