use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Layout, Rect},
    style::Style,
    text::Line,
    widgets::{Block, BorderType, Clear, Widget},
};
use ratatui_textarea::TextArea;

use crate::{color_scheme::COLOR_SCHEME, mail, util::centered_rect};

struct LabeledEdit<'a, 'b> {
    label: &'static str,
    edit: &'b TextArea<'a>,
}

impl<'a> LabeledEdit<'a, '_> {
    fn new(label: &'static str, edit: &'a TextArea) -> Self {
        Self { label, edit }
    }
}

impl Widget for &LabeledEdit<'_, '_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let layout = Layout::horizontal(vec![
            Constraint::Length(self.label.len() as u16),
            Constraint::Fill(1),
        ])
        .spacing(2)
        .split(area);
        Line::from(self.label.to_string())
            .style(Style::default().bold())
            .render(layout[0], buf);
        self.edit.render(layout[1], buf);
    }
}

pub(crate) struct ComposeView<'a> {
    account: Box<str>,
    AddrToText: TextArea<'a>,
    AddrCcText: TextArea<'a>,
    AddrBccText: TextArea<'a>,
    SubjectText: TextArea<'a>,
    BodyText: TextArea<'a>,
}

impl<'a> ComposeView<'a> {
    pub(crate) fn new_mail(account: Box<str>) -> Self {
        Self {
            account,
            AddrToText: Default::default(),
            AddrCcText: Default::default(),
            AddrBccText: Default::default(),
            SubjectText: Default::default(),
            BodyText: Default::default(),
        }
    }

    pub(crate) fn reply_to(account: Box<str>, msg: &mail::Email) -> Self {
        let mut s = Self {
            account,
            AddrToText: TextArea::from([msg
                .sender
                .iter()
                .map(|a| a.email.as_ref())
                .collect::<Vec<&str>>()
                .join(", ")]),
            AddrCcText: Default::default(),
            AddrBccText: Default::default(),
            SubjectText: TextArea::from([format!(
                "Re: {}",
                msg.subject.as_ref().unwrap_or(&Box::from(""))
            )]),
            BodyText: Default::default(),
        };
        s.SubjectText.set_style(COLOR_SCHEME.text_style());
        s
    }
}

impl Widget for &ComposeView<'_> {
    fn render(self, area: ratatui::prelude::Rect, buf: &mut ratatui::prelude::Buffer)
    where
        Self: Sized,
    {
        let window_area = centered_rect(area, 70, 70, 20, 60);
        let block = Block::bordered()
            .title("New email")
            .border_type(BorderType::Double);
        block.inner(window_area);
        let layout = Layout::vertical(vec![
            Constraint::Max(1),
            Constraint::Max(1),
            Constraint::Fill(1),
        ])
        .spacing(0)
        .horizontal_margin(1)
        .split(block.inner(window_area));

        Clear.render(window_area, buf);
        block.render(window_area, buf);
        LabeledEdit::new("To: ", &self.AddrToText).render(layout[0], buf);
        LabeledEdit::new("Subject: ", &self.SubjectText).render(layout[1], buf);
        Block::bordered()
            .border_type(BorderType::Rounded)
            .render(layout[2], buf);
    }
}
