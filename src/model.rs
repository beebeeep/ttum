use crossterm::event::KeyEvent;

use crate::{emails_widget::EmailsList, mailboxes_widget::MailboxesList};

pub(crate) struct Model {
    pub(crate) running_state: RunningState,
    pub(crate) active_pane: ActivePane,
    pub(crate) mbox_list: MailboxesList,
    pub(crate) emails_list: EmailsList,
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
    Done,
}

pub(crate) enum Message {
    KeyPress(KeyEvent),
    NextMailbox,
    PrevMailbox,
    NextMessage,
    PrevMessage,
    FocusNext,
    FocusPrev,
    Quit,
}
