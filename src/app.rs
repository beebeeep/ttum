use crate::{
    mail,
    mailboxes_widget::MailboxesList,
    model::{ActivePane, Model, RunningState},
};
use anyhow::{Context, Result};
use tui_widget_list::ListState;

pub struct App {
    model: Model,
    db: rusqlite::Connection,
}

impl App {
    pub fn load(db: rusqlite::Connection) -> Result<Self> {
        let mut connections =
            mail::connect_to_accounts(&db).context("connecting to all accounts")?;
        let mut mailboxes: Vec<(String, Vec<String>)> = Vec::with_capacity(connections.len());
        for (account, conn) in connections.iter_mut() {
            let list = conn.list(None, Some("*")).context("listing mailboxes")?;
            mailboxes.push((
                String::from(account.as_ref()),
                list.iter()
                    .map(|v| utf7_imap::decode_utf7_imap(String::from(v.name())))
                    .collect(),
            ));
        }
        println!("{:?}", mailboxes);
        let mut r = Self {
            model: Model {
                running_state: RunningState::MainView,
                active_pane: ActivePane::Mailboxes,
                mbox_list: MailboxesList {
                    mailboxes,
                    active_mailbox: (0, 0),
                    list_state: ListState::default(),
                },
            },
            db,
        };

        Ok(r)
    }
}
