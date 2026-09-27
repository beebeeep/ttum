use ratatui::{
    style::Style,
    text::Text,
    widgets::{Block, List, ListState, StatefulWidget, Widget},
};

use crate::color_scheme::COLOR_SCHEME;

pub(crate) struct MailboxesList {
    pub(crate) mailboxes: Vec<(Box<str>, Option<Box<str>>)>,
    pub(crate) selected_mailbox: usize,
    pub(crate) list_state: ListState,
}

impl MailboxesList {
    pub(crate) fn select_next_mailbox(&mut self) {
        self.selected_mailbox = (self.selected_mailbox + 1) % self.mailboxes.len();
        self.list_state.select(Some(self.selected_mailbox));
        if self.mailboxes[self.selected_mailbox].1.is_none() {
            self.select_next_mailbox();
        }
    }
    pub(crate) fn select_prev_mailbox(&mut self) {
        self.selected_mailbox =
            (self.selected_mailbox + self.mailboxes.len() - 1) % self.mailboxes.len();
        self.list_state.select(Some(self.selected_mailbox));
        if self.mailboxes[self.selected_mailbox].1.is_none() {
            {
                self.select_prev_mailbox();
            }
        }
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
        let list = List::from_iter(self.mailboxes.iter().map(|(account, mbox)| match mbox {
            Some(mbox) => Text::styled(format!("  {mbox}"), Style::default()),
            None => Text::styled(String::from(account.as_ref()), Style::default().bold()),
        }))
        .style(
            Style::default()
                .fg(COLOR_SCHEME.text_fg)
                .bg(COLOR_SCHEME.text_bg),
        )
        .highlight_style(
            Style::default()
                .fg(COLOR_SCHEME.cursor_fg)
                .bg(COLOR_SCHEME.cursor_bg),
        );
        let mut block = Block::bordered()
            .title("Mailboxes")
            .title_alignment(ratatui::layout::Alignment::Left);
        block = block
            .border_type(ratatui::widgets::BorderType::Rounded)
            .title_style(
                Style::default()
                    .fg(COLOR_SCHEME.text_fg)
                    .bg(COLOR_SCHEME.text_bg),
            )
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
