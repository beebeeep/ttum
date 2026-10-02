use ratatui::{
    style::Style,
    text::Text,
    widgets::{Block, Paragraph, Widget, Wrap},
};

use crate::{
    color_scheme::COLOR_SCHEME,
    mail::Email,
    model::{Message, ScrollDirection},
};

pub(crate) struct Content {
    pub(crate) focused: bool,
    pub(crate) email: Email,
    scroll: (u16, u16),
}

impl Content {
    pub(crate) fn new() -> Self {
        Self {
            focused: false,
            email: Email::default(),
            scroll: (0, 0),
        }
    }

    pub(crate) fn scroll(&mut self, d: ScrollDirection) -> Option<Message> {
        match d {
            ScrollDirection::Up => {
                self.scroll.0 = self.scroll.0.saturating_sub(1);
            }
            ScrollDirection::Down => self.scroll.0 += 1,
            ScrollDirection::Right => self.scroll.1 = self.scroll.1.saturating_sub(1),
            ScrollDirection::Left => self.scroll.1 += 1,
        }
        None
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

        let body = self.email.body.as_ref().map(AsRef::as_ref).unwrap_or("");
        let paragraph = Paragraph::new(body)
            .style(default_style)
            .wrap(Wrap { trim: true })
            .scroll(self.scroll);
        let content_area = block.inner(area);

        block.render(area, buf);
        paragraph.render(content_area, buf);
    }
}
