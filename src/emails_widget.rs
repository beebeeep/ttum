use std::{collections::VecDeque, time::Duration};

use crossterm::event::KeyCode;
use ratatui::{
    layout::Constraint,
    style::Style,
    text::Text,
    widgets::{Block, List, ListState, Row, StatefulWidget, Table, TableState, Widget},
};
use time::{OffsetDateTime, format_description::well_known};

use crate::{
    app::Envelope,
    color_scheme::COLOR_SCHEME,
    model::{Message, ScrollDirection},
};

const FMT_TIME_ONLY: time::format_description::FormatDescriptionV3<'_> =
    time::macros::format_description!(version = 3, "[hour repr:24]:[minute padding:zero]");
const FMT_DATETIME: time::format_description::FormatDescriptionV3<'_> = time::macros::format_description!(
    version = 3,
    "[year repr:full]-[month]-[day] [hour repr:24]:[minute padding:zero]"
);
pub(crate) struct EmailsList {
    // List of emails, already sorted by threads.
    // Should be greater than displayable amount, and dynamically extended from both ends as user scrolls
    pub(crate) emails: VecDeque<Envelope>,
    pub(crate) selected_email: usize,
    pub(crate) table_state: TableState,
    pub(crate) focused: bool,
}

impl EmailsList {
    pub(crate) fn scroll(&mut self, d: ScrollDirection) -> Option<Message> {
        match d {
            ScrollDirection::Up => {
                self.selected_email = self.selected_email.saturating_sub(1);
                self.table_state.select(Some(self.selected_email));
                if self.selected_email == 0 {
                    Some(Message::LoadMoreMails(
                        crate::model::EmailSelector::Before {
                            ts: self.emails[self.selected_email].timestamp,
                            uid: self.emails[self.selected_email].uid,
                        },
                    ))
                } else {
                    None
                }
            }
            ScrollDirection::Down => {
                self.selected_email += 1;
                self.table_state.select(Some(self.selected_email));
                if self.selected_email >= self.emails.len() - 1 {
                    self.selected_email = self.emails.len() - 1;
                    self.table_state.select(Some(self.selected_email));
                    Some(Message::LoadMoreMails(crate::model::EmailSelector::After {
                        ts: self.emails[self.selected_email].timestamp,
                        uid: self.emails[self.selected_email].uid,
                    }))
                } else {
                    None
                }
            }
            ScrollDirection::Right => {
                self.table_state.scroll_right_by(1);
                None
            }
            ScrollDirection::Left => {
                self.table_state.scroll_left_by(1);
                None
            }
        }
    }

    pub(crate) fn select_next_email(&mut self) -> Option<(OffsetDateTime, u32)> {
        self.selected_email += 1;
        self.table_state.select(Some(self.selected_email));
        if self.selected_email >= self.emails.len() - 1 {
            self.selected_email = self.emails.len() - 1;
            self.table_state.select(Some(self.selected_email));
            return Some((
                self.emails[self.selected_email].timestamp,
                self.emails[self.selected_email].uid,
            ));
        }
        None
    }

    pub(crate) fn select_prev_email(&mut self) -> Option<(OffsetDateTime, u32)> {
        self.selected_email = self.selected_email.saturating_sub(1);
        self.table_state.select(Some(self.selected_email));
        if self.selected_email == 0 {
            return Some((
                self.emails[self.selected_email].timestamp,
                self.emails[self.selected_email].uid,
            ));
        }
        None
    }

    pub(crate) fn push_back_emails(&mut self, emails: Vec<Envelope>) {
        let added = emails.len();
        let o = self.table_state.offset_mut();
        if *o > 0 {
            *o -= added;
        }

        if let Some(v) = self.table_state.selected_mut() {
            *v -= added;
        }
        self.selected_email -= added;
        for email in emails {
            self.emails.push_back(email);
            let _ = self.emails.pop_front(); // discard emails from the head
        }
    }
    pub(crate) fn pop_front_emails(&mut self, emails: Vec<Envelope>) {
        let added = emails.len();
        *self.table_state.offset_mut() += added;
        self.table_state
            .selected_mut()
            .as_mut()
            .map(|v| *v += added);
        self.selected_email += added;
        for email in emails {
            self.emails.push_back(email);
            let _ = self.emails.pop_front(); // discard emails from the head
        }
    }
}

impl Widget for &mut EmailsList {
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

        let rows = self.emails.iter().map(|msg| {
            let addresses = msg
                .addresses
                .iter()
                .map(String::from)
                .collect::<Vec<String>>()
                .join(", ");
            let ts = if OffsetDateTime::now_utc() - msg.timestamp < Duration::from_hours(24) {
                msg.timestamp.format(&FMT_TIME_ONLY)
            } else {
                msg.timestamp.format(&FMT_DATETIME)
            }
            .unwrap();
            Row::new(vec![ts, addresses, msg.subject.clone().into_string()])
        });
        let widths = [
            Constraint::Length(20),
            Constraint::Fill(40),
            Constraint::Fill(60),
        ];
        let table = Table::new(rows, widths)
            .header(Row::new(vec!["Date", "From", "Subject"]).style(default_style.bold()))
            .style(default_style)
            .row_highlight_style(highlight_style);

        let block = Block::bordered()
            .title(" Messages in mailbox ")
            .title_style(if self.focused {
                highlight_style.bold()
            } else {
                default_style
            })
            .title_alignment(ratatui::layout::Alignment::Left)
            .border_type(ratatui::widgets::BorderType::Rounded)
            .border_style(default_style);
        let table_area = block.inner(area);
        block.render(area, buf);
        StatefulWidget::render(table, table_area, buf, &mut self.table_state);
    }
}
