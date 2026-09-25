use std::borrow::Cow;

use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Layout, Rect},
    style::{Style, Stylize},
    text::Line,
    widgets::{Block, StatefulWidget, Widget},
};
use tui_widget_list::{ListBuilder, ListState, ListView};

use crate::color_scheme::COLOR_SCHEME;

struct MailboxItem<'a> {
    label: Cow<'a, str>,
    unread: u64,
    is_acc: bool,
    style: Style,
}

impl<'a> Widget for MailboxItem<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if self.is_acc {
            Line::styled(self.label, self.style.bold()).render(area, buf);
            return;
        }

        let areas = Layout::horizontal([
            Constraint::Fill(1), // mailbox labels are padded a bit, to be under account
            Constraint::Fill(8),
            Constraint::Fill(1),
        ])
        .split(area);
        Line::styled(self.label, self.style).render(areas[1], buf);
        Line::styled(format!("{}", self.unread), self.style)
            .right_aligned()
            .render(areas[2], buf);
    }
}

pub(crate) struct MailboxesList {
    pub(crate) mailboxes: Vec<(String, Vec<String>)>,
    pub(crate) active_mailbox: (usize, usize),
    pub(crate) list_state: ListState,
}

impl MailboxesList {
    fn select_next_mailbox(&mut self) {
        self.list_state.next();
        self.active_mailbox.1 += 1;
        if self.active_mailbox.1 >= self.mailboxes[self.active_mailbox.0].1.len() {
            self.active_mailbox.0 += 1;
            self.active_mailbox.1 = 0;
            self.list_state.next();
        }
        if self.active_mailbox.0 >= self.mailboxes.len() {
            self.active_mailbox.0 = 0
        }
    }
    fn select_prev_mailbox(&mut self) {
        self.list_state.previous();
        if self.active_mailbox.1 > 0 {
            self.active_mailbox.1 -= 1;
            return;
        } else if self.active_mailbox.0 > 0 {
            self.active_mailbox.0 -= 1;
            self.active_mailbox.1 = self.mailboxes[self.active_mailbox.0].1.len() - 1;
            self.list_state.previous();
        } else {
            self.active_mailbox.0 = self.mailboxes.len() - 1;
            self.active_mailbox.1 = self.mailboxes[self.active_mailbox.0].1.len() - 1;
            self.list_state.previous();
        }
    }
}

impl Widget for &mut MailboxesList {
    fn render(self, area: ratatui::prelude::Rect, buf: &mut ratatui::prelude::Buffer)
    where
        Self: Sized,
    {
        if self.list_state.selected.is_none() {
            self.list_state.next();
        }
        let mut entries = Vec::with_capacity(self.mailboxes.len() * 3);
        for account in &self.mailboxes {
            entries.push((true, &account.0));
            for mbox in &account.1 {
                entries.push((false, mbox));
            }
        }
        let builder = ListBuilder::new(|context| {
            let mut style = Style::default()
                .fg(COLOR_SCHEME.text_fg)
                .bg(COLOR_SCHEME.text_bg);
            if context.is_selected {
                style = style.fg(COLOR_SCHEME.cursor_fg).bg(COLOR_SCHEME.cursor_bg);
            }
            let e = entries[context.index];
            let item = MailboxItem {
                label: Cow::from(e.1),
                unread: 0,
                is_acc: e.0,
                style,
            };
            (item, 1)
        });
        let list = ListView::new(builder, entries.len());
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
        list.render(list_area, buf, &mut self.list_state);
    }
}
