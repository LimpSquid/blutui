use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Color;
use ratatui::widgets::Widget;

#[derive(Debug, Default, Clone, Eq, PartialEq, Hash)]
pub struct Fill(pub Color);

impl Widget for Fill {
    fn render(self, area: Rect, buf: &mut Buffer) {
        Widget::render(&self, area, buf);
    }
}

impl Widget for &Fill {
    fn render(self, area: Rect, buf: &mut Buffer) {
        for x in area.left()..area.right() {
            for y in area.top()..area.bottom() {
                buf[(x, y)].reset();
                buf[(x, y)].set_bg(self.0);
            }
        }
    }
}
