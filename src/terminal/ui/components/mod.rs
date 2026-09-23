mod prelude {
    #![allow(unused)]
    pub use super::super::{theme::*, widgets::*};
    pub use ratatui::buffer::Buffer;
    pub use ratatui::layout::Rect;
    pub use ratatui::widgets::WidgetRef;
}

pub mod dialog;
mod keybindings;

pub use keybindings::Keybindings;

pub trait BoxedComponent: Sized {
    fn boxed(self) -> Box<Self> {
        Box::new(self)
    }
}
impl<T: prelude::WidgetRef> BoxedComponent for T {}
