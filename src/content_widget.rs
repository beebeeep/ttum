use ratatui::{
    style::Style,
    text::Text,
    widgets::{Block, Paragraph, Widget, Wrap},
};

use crate::color_scheme::COLOR_SCHEME;

pub(crate) struct Content {
    pub(crate) focused: bool,
    pub(crate) content: Box<str>,
    scroll: (u16, u16),
}

impl Content {
    pub(crate) fn new(content: Box<str>) -> Self {
        Self {
            focused: false,
            content,
            scroll: (0, 0),
        }
    }
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

        let paragraph = Paragraph::new(self.content.as_ref())
            .style(default_style)
            .wrap(Wrap { trim: true })
            .scroll(self.scroll);
        let content_area = block.inner(area);

        block.render(area, buf);
        paragraph.render(content_area, buf);
    }
}
