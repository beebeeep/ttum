use std::sync::LazyLock;

use ratatui::style::Color;

pub(crate) static COLOR_SCHEME: LazyLock<ColorScheme, fn() -> ColorScheme> =
    LazyLock::new(ColorScheme::load);

pub(crate) struct ColorScheme {
    pub(crate) text_fg: Color,
    pub(crate) text_bg: Color,
    pub(crate) cursor_fg: Color,
    pub(crate) cursor_bg: Color,
    pub(crate) status_bar_bg: Color,
    pub(crate) status_bar_fg: Color,
}

impl ColorScheme {
    fn load() -> Self {
        Self {
            text_fg: Color::Reset,
            text_bg: Color::Reset,
            cursor_fg: Color::Rgb(0, 0, 0),
            cursor_bg: Color::Rgb(0xbf, 0xdb, 0xfe),
            status_bar_fg: Color::Rgb(0, 0, 0),
            status_bar_bg: Color::Rgb(0xd0, 0xd0, 0xd0),
        }
    }
}
