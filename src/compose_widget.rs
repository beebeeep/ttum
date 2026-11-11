use ratatui::widgets::Widget;
use ratatui_textarea::TextArea;

use crate::{mail, util::centered_rect};

pub(crate) struct ComposeView<'a> {
    account: Box<str>,
    AddrToText: TextArea<'a>,
    AddrCcText: TextArea<'a>,
    AddrBccText: TextArea<'a>,
    SubjectText: TextArea<'a>,
    BodyText: TextArea<'a>,
}

impl<'a> ComposeView<'a> {
    fn new_mail(account: Box<str>) -> Self {
        Self {
            account,
            AddrToText: Default::default(),
            AddrCcText: Default::default(),
            AddrBccText: Default::default(),
            SubjectText: Default::default(),
            BodyText: Default::default(),
        }
    }

    fn reply_to(account: Box<str>, msg: &mail::Email) -> Self {
        Self {
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
        }
    }
}

impl Widget for &ComposeView<'_> {
    fn render(self, area: ratatui::prelude::Rect, buf: &mut ratatui::prelude::Buffer)
    where
        Self: Sized,
    {
        let window_area = centered_rect(area, 70, 70, 20, 60);
        let block = Block::bordered()
            .title(
                self.task_id
                    .map_or_else(|| "New task".to_string(), |id| format!("Task #{id}")),
            )
            .border_type(BorderType::Double);
    }
}
