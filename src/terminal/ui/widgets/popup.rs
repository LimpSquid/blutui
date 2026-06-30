use ratatui::Frame;
use ratatui::buffer::Buffer;
use ratatui::layout::{Constraint, Margin, Rect};
use ratatui::style::Color;
use ratatui::widgets::{Block, BorderType, Clear, Widget};

#[derive(Debug)]
pub struct Popup<'a> {
    block: Block<'a>,
    horizontal_constraint: Constraint,
    vertical_constraint: Constraint,
    title: Option<&'static str>,
}

impl<'a> Popup<'a> {
    pub fn new() -> Self {
        Self {
            block: Block::bordered().border_type(BorderType::Thick),
            horizontal_constraint: Constraint::Percentage(60),
            vertical_constraint: Constraint::Percentage(60),
            title: None,
        }
    }

    pub fn with_title(title: &'static str) -> Self {
        Self {
            block: Block::bordered().border_type(BorderType::Thick),
            horizontal_constraint: Constraint::Percentage(60),
            vertical_constraint: Constraint::Percentage(60),
            title: Some(title),
        }
    }

    pub fn block(mut self, block: Block<'a>) -> Self {
        self.block = block;
        self
    }

    pub fn constraints(mut self, constraints: [Constraint; 2]) -> Self {
        self.horizontal_constraint = constraints[0];
        self.vertical_constraint = constraints[1];
        self
    }

    pub fn drawable_area(&self, area: Rect) -> Rect {
        area.centered(self.horizontal_constraint, self.vertical_constraint)
            .inner(Margin::new(1, 1))
    }
}

impl<'a> Widget for Popup<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let mut block = self.block;
        if let Some(title) = self.title {
            block = block.title(format!(" {title} "));
        }
        let centered_area = area.centered(self.horizontal_constraint, self.vertical_constraint);
        Clear.render(centered_area, buf);
        block.render(centered_area, buf);
    }
}
