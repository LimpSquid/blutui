mod prelude {
    pub use super::super::Keybindings;
    pub use super::super::prelude::*;
    pub use super::{DialogComponent, DialogEvent};
    pub use crate::terminal::app::AppState;
    pub use crate::terminal::ui::UserAction;
    pub use crate::terminal::ui::event::{KeyCode, KeyModifiers};
    pub use crate::terminal::ui::utils::*;
    pub use ratatui::layout::{Alignment, Constraint, Direction, Layout};
    pub use ratatui::style::{Style, Stylize};
    pub use ratatui::text::{Line, Span, Text};
    pub use ratatui::widgets::{
        Block, BorderType, List, ListState, Paragraph, StatefulWidget, Widget, Wrap,
    };
}

mod delete_profile;
mod new_profile;
mod notification;

pub use delete_profile::DeleteProfileDialog;
pub use new_profile::NewProfileDialog;
pub use notification::NotificationDialog;

#[allow(unused)]
#[derive(Debug)]
pub enum DialogEvent {
    Actions(Vec<prelude::UserAction>),
    Submitted(Vec<prelude::UserAction>),
    Closed,
    ClosedErr(anyhow::Error),
}

pub trait DialogComponent: prelude::Component {
    fn on_key_press(
        &mut self,
        code: prelude::KeyCode,
        modifiers: prelude::KeyModifiers,
        state: &prelude::AppState,
    ) -> Option<DialogEvent>;
}
