use std::{
    fmt::Display,
    time::{Duration, Instant},
};

use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent};
use rusqlite::Connection;
use time::OffsetDateTime;

use crate::{
    compose_widget::ComposeView,
    content_widget::Content,
    emails_widget::EmailsList,
    mail::{Email, Mailbox, MailboxWithMetadata},
    mailboxes_widget::MailboxesList,
};

#[derive(Debug, Clone)]
pub(crate) enum EmailSelector {
    Latest,
    Before {
        ts: OffsetDateTime,
        uid: u32,
        inclusive: bool,
    },
    After {
        ts: OffsetDateTime,
        uid: u32,
        inclusive: bool,
    },
}

#[derive(Default, Debug)]
pub(crate) struct ReindexStatus {
    pub(crate) mailbox: Mailbox,
    pub(crate) current: usize,
    pub(crate) total: usize,
    pub(crate) done: bool,
}

pub(crate) struct Model<'a> {
    pub(crate) running_state: RunningState,
    pub(crate) active_pane: ActivePane,
    pub(crate) reindex_status: ReindexStatus,
    pub(crate) status_bar_text: String,
    pub(crate) last_key: LastPressedKey,
    pub(crate) mbox_pane: MailboxesList,
    pub(crate) emails_pane: EmailsList,
    pub(crate) content_pane: Content,
    pub(crate) compose_window: ComposeView<'a>,
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
    MailboxRefresh(Mailbox),
    MailboxMetadata(MailboxWithMetadata),
    ReindexStatus(ReindexStatus),
    StatusMessage(Box<str>),
    NewEmails {
        mailbox: Mailbox,
        emails: Vec<Email>,
    },
    VanishedEmails {
        mailbox: Mailbox,
        uids: Vec<u32>,
    },
    Scroll(ScrollDirection),
    FocusNext,
    FocusPrev,
    SelectedEmail(Option<u32>),
    Batch(Vec<Message>),
    ToggleMailboxVisibility {
        mailbox: Mailbox,
        visible: bool,
    },
    ShowComposeWindow(Box<str>),
    CloseComposeWindow,
    Quit,
}

pub(crate) struct LastPressedKey {
    key: KeyCode,
    ts: Instant,
}

impl LastPressedKey {
    const DOUBLE_PRESS_TIME: Duration = Duration::from_millis(300);

    pub(crate) fn save(&mut self, key: &KeyCode) {
        self.key = *key;
        self.ts = Instant::now();
    }

    pub(crate) fn is_double(&self, key: &KeyCode) -> bool {
        self.key == *key && self.ts.elapsed() <= Self::DOUBLE_PRESS_TIME
    }
}

impl Default for LastPressedKey {
    fn default() -> Self {
        Self {
            key: KeyCode::Backspace,
            ts: Instant::now(),
        }
    }
}
