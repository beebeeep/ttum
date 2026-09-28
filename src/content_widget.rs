use ratatui::{
    style::Style,
    widgets::{Block, Widget},
};

use crate::color_scheme::COLOR_SCHEME;

pub(crate) struct Content {
    pub(crate) focused: bool,
}

impl Widget for &mut Content {
    fn render(self, area: ratatui::prelude::Rect, buf: &mut ratatui::prelude::Buffer)
    where
        Self: Sized,
    {
        let default_style = Style::default()
            .fg(COLOR_SCHEME.text_fg)
            .bg(COLOR_SCHEME.text_bg);
        let highlight_style = Style::default()
            .fg(COLOR_SCHEME.cursor_fg)
            .bg(COLOR_SCHEME.cursor_bg);

        let block = Block::bordered()
            .title(" Message ")
            .title_style(if self.focused {
                highlight_style.bold()
            } else {
                default_style
            })
            .title_alignment(ratatui::layout::Alignment::Left)
            .border_type(ratatui::widgets::BorderType::Rounded)
            .border_style(default_style);

        block.render(area, buf);
    }
}
