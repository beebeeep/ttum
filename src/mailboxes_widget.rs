use ratatui::{
    style::Style,
    text::Text,
    widgets::{Block, List, ListState, StatefulWidget, Widget},
};

use crate::{
    color_scheme::COLOR_SCHEME,
    model::{Message, ScrollDirection},
};

pub(crate) struct MailboxesList {
    pub(crate) mailboxes: Vec<(Box<str>, Option<Box<str>>)>,
    pub(crate) selected_mailbox: usize,
    pub(crate) list_state: ListState,
    pub(crate) focused: bool,
}

impl MailboxesList {
    pub(crate) fn scroll(&mut self, d: ScrollDirection) -> Option<Message> {
        match d {
            ScrollDirection::Down => {
                self.selected_mailbox = (self.selected_mailbox + 1) % self.mailboxes.len();
                self.list_state.select(Some(self.selected_mailbox));
                if self.mailboxes[self.selected_mailbox].1.is_none() {
                    self.scroll(d);
                }
            }
            ScrollDirection::Up => {
                self.selected_mailbox =
                    (self.selected_mailbox + self.mailboxes.len() - 1) % self.mailboxes.len();
                self.list_state.select(Some(self.selected_mailbox));
                if self.mailboxes[self.selected_mailbox].1.is_none() {
                    {
                        self.scroll(d);
                    }
                }
            }
            _ => {}
        };

        let (account, mailbox) = self.current_mailbox();
        Some(Message::MailboxChange { account, mailbox })
    }

    pub(crate) fn current_mailbox(&self) -> (Box<str>, Box<str>) {
        (
            self.mailboxes[self.selected_mailbox].0.clone(),
            self.mailboxes[self.selected_mailbox]
                .1
                .as_ref()
                .unwrap()
                .clone(),
        )
    }
}

impl Widget for &mut MailboxesList {
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
        let list = List::from_iter(self.mailboxes.iter().map(|(account, mbox)| match mbox {
            Some(mbox) => Text::styled(format!("  {mbox}"), Style::default()),
            None => Text::styled(String::from(account.as_ref()), Style::default().bold()),
        }))
        .style(default_style)
        .highlight_style(highlight_style);
        let block = Block::bordered()
            .title(" Mailboxes ")
            .title_style(if self.focused {
                highlight_style.bold()
            } else {
                default_style
            })
            .title_alignment(ratatui::layout::Alignment::Left)
            .border_type(ratatui::widgets::BorderType::Rounded)
            .border_style(
                Style::default()
                    .fg(COLOR_SCHEME.text_fg)
                    .bg(COLOR_SCHEME.text_bg),
            );
        let list_area = block.inner(area);
        block.render(area, buf);
        StatefulWidget::render(list, list_area, buf, &mut self.list_state);
    }
}
