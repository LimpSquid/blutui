use ratatui::style::Color;

pub const NON_BREAKING_SPACE: char = '\u{00A0}';
pub const HIGHLIGHT_SYMBOL: char = '🟊';
pub const MASTER_SYMBOL: char = '★';
pub const SLAVE_SYMBOL: char = '●';
pub const ZONE_SLAVE_SYMBOL: char = '¶';
pub const WARNING_SYMBOL: &str = "⚠️";

#[derive(Debug, Clone, Copy)]
pub struct Stylesheet {
    pub background_color: Color,
    pub accent_color: Color,
    pub accent_color_dark: Color,
    pub highlight_color: Color,
    pub text_color: Color,
    pub text_color_sub: Color,
    pub border_color: Color,
    pub error_color: Color,
    pub success_color: Color,
    pub popup_background_color: Color,
}

impl Default for Stylesheet {
    fn default() -> Self {
        Self {
            background_color: Color::Rgb(25, 29, 46),
            accent_color: Color::Rgb(38, 166, 154),
            accent_color_dark: Color::Rgb(19, 83, 77),
            highlight_color: Color::Yellow,
            text_color: Color::Rgb(255, 255, 255),
            text_color_sub: Color::Rgb(150, 150, 150),
            border_color: Color::Rgb(125, 125, 125),
            error_color: Color::LightRed,
            success_color: Color::LightGreen,
            popup_background_color: Color::Rgb(45, 49, 66),
        }
    }
}
