use crossterm::event::KeyEvent;

use crate::{content_widget::Content, emails_widget::EmailsList, mailboxes_widget::MailboxesList};

#[derive(Default, Debug)]
pub(crate) struct ReindexStatus {
    pub(crate) account: Box<str>,
    pub(crate) mailbox: Box<str>,
    pub(crate) progress: f64,
    pub(crate) done: bool,
}

pub(crate) struct Model {
    pub(crate) running_state: RunningState,
    pub(crate) active_pane: ActivePane,
    pub(crate) reindex_status: ReindexStatus,
    pub(crate) status_bar_text: String,

    pub(crate) mbox_pane: MailboxesList,
    pub(crate) emails_pane: EmailsList,
    pub(crate) content_pane: Content,
}

#[derive(Default, PartialEq)]
pub(crate) enum ActivePane {
    Mailboxes,
    #[default]
    Emails,
    Content,
}

#[derive(Default, PartialEq)]
pub(crate) enum RunningState {
    #[default]
    MainView,
    ComposeMail,
    Reindex,
    Done,
}

#[derive(Debug)]
pub(crate) enum Message {
    KeyPress(KeyEvent),
    ReindexMailbox {
        account: Box<str>,
        mailbox: Box<str>,
    },
    ReindexStatus(ReindexStatus),
    NextMailbox,
    PrevMailbox,
    NextMessage,
    PrevMessage,
    FocusNext,
    FocusPrev,
    Quit,
}
