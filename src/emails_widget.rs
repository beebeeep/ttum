use ratatui::{
    style::Style,
    text::Text,
    widgets::{Block, List, ListState, StatefulWidget, TableState, Widget},
};

use crate::{color_scheme::COLOR_SCHEME, mail::Envelope};

pub(crate) struct EmailsList {
    // List of emails, already sorted by threads.
    // Should be greater than displayable amount, and dynamically extended from both ends as user scrolls
    pub(crate) emails: Vec<Envelope>,
    pub(crate) selected_emails: usize,
    pub(crate) table_state: TableState,
}

impl EmailsList {
    pub(crate) fn select_next_email(&mut self) -> bool {
        self.selected_emails = (self.selected_emails + 1) % self.emails.len();
        self.table_state.select(Some(self.selected_emails));
        return self.selected_emails == 0;
    }
    pub(crate) fn select_prev_email(&mut self) -> bool {
        self.selected_emails = (self.selected_emails + self.emails.len() - 1) % self.emails.len();
        self.table_state.select(Some(self.selected_emails));
        return self.selected_emails == self.emails.len() - 1;
    }
}

impl Widget for &mut EmailsList {
    fn render(self, area: ratatui::prelude::Rect, buf: &mut ratatui::prelude::Buffer)
    where
        Self: Sized,
    {
        let list = List::from_iter(self.emails.iter().map(|(account, mbox)| match mbox {
            Some(mbox) => Text::styled(format!("  {mbox}"), Style::default()),
            None => Text::styled(account.as_str(), Style::default().bold()),
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
