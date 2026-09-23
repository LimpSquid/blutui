mod prelude {
    #![allow(unused)]
    pub use super::super::{theme::*, widgets::*};
    pub use super::{Component, ComponentContext};
    pub use ratatui::buffer::Buffer;
    pub use ratatui::layout::Rect;
}

pub mod dialog;
mod keybindings;

pub use keybindings::Keybindings;

pub struct ComponentContext<'a> {
    pub buffer: &'a mut prelude::Buffer,
    pub state: &'a super::super::app::AppState,
    pub stylesheet: &'a prelude::Stylesheet,
}

pub trait Component {
    fn render(&self, area: prelude::Rect, ctx: &mut ComponentContext<'_>);
}

pub trait BoxedComponent: Sized {
    fn boxed(self) -> Box<Self> {
        Box::new(self)
    }
}
impl<T: Component> BoxedComponent for T {}
