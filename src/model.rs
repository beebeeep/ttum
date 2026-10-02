use crossterm::event::KeyEvent;
use time::OffsetDateTime;

use crate::{content_widget::Content, emails_widget::EmailsList, mailboxes_widget::MailboxesList};

#[derive(Debug, Clone)]
pub(crate) enum EmailSelector {
    Latest,
    Before { ts: OffsetDateTime, uid: u32 },
    After { ts: OffsetDateTime, uid: u32 },
}

#[derive(Default, Debug)]
pub(crate) struct ReindexStatus {
    pub(crate) account: Box<str>,
    pub(crate) mailbox: Box<str>,
    pub(crate) current: usize,
    pub(crate) total: usize,
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
pub(crate) enum ScrollDirection {
    Up,
    Down,
    Right,
    Left,
}

#[derive(Debug)]
pub(crate) enum Message {
    KeyPress(KeyEvent),
    ReindexMailbox {
        account: Box<str>,
        mailbox: Box<str>,
    },
    LoadMoreMails(EmailSelector),
    MailboxChange {
        account: Box<str>,
        mailbox: Box<str>,
    },
    ReindexStatus(ReindexStatus),
    Scroll(ScrollDirection),
    FocusNext,
    FocusPrev,
    SelectedEmail(u32),
    Batch(Vec<Message>),
    Quit,
}
