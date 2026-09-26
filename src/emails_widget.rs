use std::time::Duration;

use ratatui::{
    layout::Constraint,
    style::Style,
    text::Text,
    widgets::{Block, List, ListState, Row, StatefulWidget, Table, TableState, Widget},
};
use time::{OffsetDateTime, format_description::well_known};

use crate::{color_scheme::COLOR_SCHEME, mail::Envelope};

const FMT_TIME_ONLY: time::format_description::FormatDescriptionV3<'_> =
    time::macros::format_description!(version = 3, "[hour repr:24]:[minute padding:zero]");
const FMT_DATETIME: time::format_description::FormatDescriptionV3<'_> = time::macros::format_description!(
    version = 3,
    "[year repr:full]-[month]-[day] [hour repr:24]:[minute padding:zero]"
);
pub(crate) struct EmailsList {
    // List of emails, already sorted by threads.
    // Should be greater than displayable amount, and dynamically extended from both ends as user scrolls
    pub(crate) emails: Vec<Envelope>,
    pub(crate) selected_email: usize,
    pub(crate) table_state: TableState,
}

impl EmailsList {
    pub(crate) fn select_next_email(&mut self) -> bool {
        self.selected_email = (self.selected_email + 1) % self.emails.len();
        self.table_state.select(Some(self.selected_email));
        return self.selected_email == 0;
    }
    pub(crate) fn select_prev_email(&mut self) -> bool {
        self.selected_email = (self.selected_email + self.emails.len() - 1) % self.emails.len();
        self.table_state.select(Some(self.selected_email));
        return self.selected_email == self.emails.len() - 1;
    }
}

impl Widget for &mut EmailsList {
    fn render(self, area: ratatui::prelude::Rect, buf: &mut ratatui::prelude::Buffer)
    where
        Self: Sized,
    {
        let text_style = Style::default()
            .fg(COLOR_SCHEME.text_fg)
            .bg(COLOR_SCHEME.text_bg);
        let cursor_style = Style::default()
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
            .header(Row::new(vec!["Date", "From", "Subject"]).style(text_style.bold()))
            .style(text_style)
            .row_highlight_style(cursor_style);

        let mut block = Block::bordered()
            .title("Messages")
            .title_alignment(ratatui::layout::Alignment::Left);
        block = block
            .border_type(ratatui::widgets::BorderType::Rounded)
            .title_style(text_style)
            .border_style(text_style);
        let table_area = block.inner(area);
        block.render(area, buf);
        StatefulWidget::render(table, table_area, buf, &mut self.table_state);
    }
}
