use ratatui::buffer::Buffer;
use ratatui::layout::{Alignment, Margin, Rect};
use ratatui::style::{Style, Styled, Stylize};
use ratatui::text::Line;
use ratatui::widgets::{Block, BorderType, Paragraph, StatefulWidget, Widget, Wrap};

use super::super::event::KeyCode;

#[derive(Debug, Clone, Default)]
pub struct TextFieldState {
    value: String,
}

impl TextFieldState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_value<S: Into<String>>(value: S) -> Self {
        Self {
            value: value.into(),
        }
    }

    pub fn value(&self) -> &str {
        &self.value
    }

    pub fn on_key_press(&mut self, code: KeyCode) {
        match code {
            KeyCode::Char(c) => self.value.push(c),
            KeyCode::Backspace => {
                self.value.pop();
            }
            _ => {}
        }
    }
}

#[derive(Debug)]
pub struct TextField<'a> {
    block: Block<'a>,
    label_style: Style,
    text_style: Style,
    label: Option<&'static str>,
}

impl<'a> TextField<'a> {
    pub fn new() -> Self {
        Self {
            block: Block::bordered().border_type(BorderType::Rounded),
            label: None,
            label_style: Style::new().bold(),
            text_style: Style::new(),
        }
    }

    pub fn block(mut self, block: Block<'a>) -> Self {
        self.block = block;
        self
    }

    pub fn with_label(label: &'static str) -> Self {
        Self {
            block: Block::bordered().border_type(BorderType::Rounded),
            label: Some(label),
            label_style: Style::new().bold(),
            text_style: Style::new(),
        }
    }

    pub fn label_style(mut self, style: Style) -> Self {
        self.label_style = style;
        self
    }

    pub fn text_style(mut self, style: Style) -> Self {
        self.text_style = style;
        self
    }
}

impl<'a> StatefulWidget for TextField<'a> {
    type State = TextFieldState;

    fn render(self, area: Rect, buf: &mut Buffer, state: &mut Self::State) {
        self.block
            .title(self.label.map(|s| format!(" {s} ")).unwrap_or_default())
            .title_style(self.label_style)
            .title_alignment(Alignment::Left)
            .render(area, buf);

        let area = area.inner(Margin {
            vertical: 1,
            horizontal: 2,
        });

        let text = Paragraph::new(Line::from(vec![
            state.value.as_str().set_style(self.text_style),
            "█".set_style(self.text_style).slow_blink(),
        ]))
        .wrap(Wrap { trim: false });

        text.render(area, buf);
    }
}
