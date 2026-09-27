use crossterm::event::KeyEvent;

use crate::{emails_widget::EmailsList, mailboxes_widget::MailboxesList};

#[derive(Default, Debug)]
pub(crate) struct ReindexStatus {
    pub(crate) account: Box<str>,
    pub(crate) mailbox: Box<str>,
    pub(crate) current_uid: u32,
    pub(crate) max_uid: u32,
    pub(crate) done: bool,
}

pub(crate) struct Model {
    pub(crate) running_state: RunningState,
    pub(crate) active_pane: ActivePane,
    pub(crate) mbox_list: MailboxesList,
    pub(crate) emails_list: EmailsList,
    pub(crate) reindex_status: ReindexStatus,
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
