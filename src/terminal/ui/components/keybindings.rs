use ratatui::{
    layout::Alignment,
    style::Stylize,
    text::{Line, Span},
    widgets::{Paragraph, Widget, Wrap},
};

use super::prelude::*;

pub struct Keybindings<'a, T> {
    keybindings: &'a [(T, T)],
    stylesheet: Stylesheet,
}

impl<'a, T: AsRef<str>> Keybindings<'a, T> {
    pub fn new(keybindings: &'a [(T, T)], stylesheet: Stylesheet) -> Self {
        Self {
            keybindings,
            stylesheet,
        }
    }
}

impl<'a, T: AsRef<str>> Widget for Keybindings<'a, T> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let n_keybindings = self.keybindings.len();
        let line = Line::from(
            self.keybindings
                .iter()
                .enumerate()
                .flat_map(|(i, (keys, desc))| {
                    vec![
                        "[".fg(self.stylesheet.text_color_sub),
                        keys.as_ref().fg(self.stylesheet.highlight_color),
                        format!("{NON_BREAKING_SPACE}→{NON_BREAKING_SPACE}")
                            .fg(self.stylesheet.text_color_sub),
                        desc.as_ref().fg(self.stylesheet.text_color),
                        "]".fg(self.stylesheet.text_color_sub),
                        if i != n_keybindings - 1 { " " } else { "" }.into(),
                    ]
                })
                .collect::<Vec<Span>>(),
        );

        Paragraph::new(line.alignment(Alignment::Center))
            .wrap(Wrap { trim: false })
            .render(area, buf);
    }
}
