use std::collections::{HashMap, HashSet};

use ratatui::{
    style::Style,
    text::Text,
    widgets::{Block, List, ListState, StatefulWidget, Widget},
};

use crate::{
    color_scheme::COLOR_SCHEME,
    model::{Mailbox, Message, ScrollDirection},
};

#[derive(Default)]
pub(crate) struct MailboxesList {
    pub(crate) mailboxes: Vec<(Box<str>, Option<Box<str>>)>,
    pub(crate) selected_mailbox: usize,
    pub(crate) list_state: ListState,
    pub(crate) focused: bool,
}

impl MailboxesList {
    pub(crate) fn new(mailboxes: &[Mailbox]) -> Self {
        let mut mm: HashMap<&str, Vec<&str>> = HashMap::new();
        for m in mailboxes {
            mm.entry(&m.account)
                .and_modify(|e| e.push(m.mailbox.as_ref()))
                .or_insert(vec![m.mailbox.as_ref()]);
        }
        let mut mailboxes = Vec::with_capacity(mm.len() * 3);
        for (acc, mboxes) in mm {
            mailboxes.push((Box::from(acc), None));
            for mbox in mboxes {
                mailboxes.push((Box::from(acc), Some(Box::from(mbox))));
            }
        }

        Self {
            mailboxes,
            selected_mailbox: 0,
            list_state: ListState::default(),
            focused: true,
        }
    }

    pub(crate) fn hide_current_mailbox(&mut self) -> Option<Message> {
        self.mailboxes.remove(self.selected_mailbox);
        if self.selected_mailbox >= self.mailboxes.len() {
            self.selected_mailbox = self.mailboxes.len() - 1;
        }
        Some(Message::MailboxChange(self.current_mailbox()))
    }

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
            _ => return None,
        };

        Some(Message::MailboxChange(self.current_mailbox()))
    }

    pub(crate) fn current_mailbox(&self) -> Mailbox {
        Mailbox {
            account: self.mailboxes[self.selected_mailbox].0.clone(),
            mailbox: self.mailboxes[self.selected_mailbox]
                .1
                .as_ref()
                .unwrap()
                .clone(),
        }
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
