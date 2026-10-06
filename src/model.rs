use std::fmt::Display;

use crossterm::event::KeyEvent;
use time::OffsetDateTime;

use crate::{
    content_widget::Content, emails_widget::EmailsList, mail::Email,
    mailboxes_widget::MailboxesList,
};

#[derive(Debug, Default, Clone)]
pub(crate) struct Mailbox {
    pub(crate) account: Box<str>,
    pub(crate) mailbox: Box<str>,
}

impl Display for Mailbox {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}/{}", self.account, self.mailbox)
    }
}

#[derive(Debug, Clone)]
pub(crate) enum EmailSelector {
    Latest,
    Before { ts: OffsetDateTime, uid: u32 },
    After { ts: OffsetDateTime, uid: u32 },
}

#[derive(Default, Debug)]
pub(crate) struct ReindexStatus {
    pub(crate) mailbox: Mailbox,
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
    ReindexMailbox(Mailbox),
    LoadMoreMails(EmailSelector),
    MailboxChange(Mailbox),
    MailboxMetadata {
        mailbox: Mailbox,
        uid_validity: u32,
        highest_mod_seq: u64,
    },
    ReindexStatus(ReindexStatus),
    NewEmails {
        mailbox: Mailbox,
        emails: Vec<Email>,
    },
    Scroll(ScrollDirection),
    FocusNext,
    FocusPrev,
    SelectedEmail(u32),
    Batch(Vec<Message>),
    Quit,
}
