use std::time::Duration;

use crate::{
    emails_widget::EmailsList,
    mail::{self, Address, Envelope},
    mailboxes_widget::MailboxesList,
    model::{self, ActivePane, Message, Model, RunningState},
};
use anyhow::{Context, Result, anyhow};
use ratatui::{
    Frame,
    crossterm::event::{self, Event, KeyCode},
    layout::{Constraint, Layout, Rect},
    widgets::{ListState, TableState},
};
use time::OffsetDateTime;

pub struct App {
    model: Model,
    db: rusqlite::Connection,
}

impl App {
    pub fn load(db: rusqlite::Connection) -> Result<Self> {
        let mut connections =
            mail::connect_to_accounts(&db).context("connecting to all accounts")?;
        let mut mailboxes = Vec::with_capacity(connections.len());
        for (account, conn) in connections.iter_mut() {
            let list = conn.list(None, Some("*")).context("listing mailboxes")?;
            mailboxes.push((String::from(account.as_ref()), None));
            for v in &list {
                mailboxes.push((
                    String::from(account.as_ref()),
                    Some(utf7_imap::decode_utf7_imap(String::from(v.name()))),
                ));
            }
        }
        let mut r = Self {
            model: Model {
                running_state: RunningState::MainView,
                active_pane: ActivePane::Mailboxes,
                mbox_list: MailboxesList {
                    mailboxes,
                    selected_mailbox: 0,
                    list_state: ListState::default(),
                },
                emails_list: EmailsList {
                    emails: Vec::with_capacity(10),
                    selected_email: 0,
                    table_state: TableState::default(),
                },
            },
            db,
        };
        r.model.mbox_list.select_next_mailbox();
        r.model.emails_list.table_state.select_next();

        Ok(r)
    }

    pub fn run(mut self) -> Result<()> {
        let mut terminal = ratatui::try_init()?;

        while self.model.running_state != RunningState::Done {
            // Render the current view
            terminal.draw(|f| self.view(f))?;

            // Handle events and map to a Message
            let mut current_msg = self.handle_event()?;

            // Process updates as long as they return a non-None message
            while current_msg.is_some() {
                current_msg = self.update(current_msg.unwrap())?;
            }
        }
        ratatui::restore();
        Ok(())
    }

    fn view(&mut self, frame: &mut Frame) {
        let layout =
            Layout::vertical([Constraint::Fill(1), Constraint::Max(1)]).split(frame.area());
        // self.status_bar(frame, layout[1]);
        match &self.model.running_state {
            RunningState::MainView => self.main_view(frame, layout[0]),
            RunningState::Done => {}
            RunningState::ComposeMail => todo!(),
        }
    }

    fn main_view(&mut self, frame: &mut Frame, area: Rect) {
        let columns =
            Layout::horizontal([Constraint::Percentage(30), Constraint::Fill(1)]).split(area);
        frame.render_widget(&mut self.model.mbox_list, columns[0]);
        frame.render_widget(&mut self.model.emails_list, columns[1]);
    }

    fn handle_event(&mut self) -> Result<Option<Message>> {
        if event::poll(Duration::from_millis(250))? {
            if let Event::Key(key) = event::read()? {
                if key.kind == event::KeyEventKind::Press {
                    return Ok(self.handle_key(key));
                }
            }
        }
        Ok(None)
    }

    fn handle_key(&self, key: event::KeyEvent) -> Option<Message> {
        match self.model.running_state {
            RunningState::MainView => match key.code {
                KeyCode::Char('q') => Some(Message::Quit),
                KeyCode::Down | KeyCode::Char('j')
                    if self.model.active_pane == ActivePane::Mailboxes =>
                {
                    Some(Message::NextMailbox)
                }
                KeyCode::Up | KeyCode::Char('k')
                    if self.model.active_pane == ActivePane::Mailboxes =>
                {
                    Some(Message::PrevMailbox)
                }
                _ => None,
            },
            RunningState::Done => None,
            RunningState::ComposeMail => None,
        }
    }

    fn update(&mut self, msg: Message) -> Result<Option<Message>> {
        Ok(match msg {
            Message::KeyPress(_) => None,
            Message::NextMailbox => {
                self.select_next_mailbox()?;
                None
            }

            Message::PrevMailbox => {
                self.select_prev_mailbox()?;
                None
            }
            Message::NextMessage => todo!(),
            Message::PrevMessage => todo!(),
            Message::FocusNext => todo!(),
            Message::FocusPrev => todo!(),
            Message::Quit => {
                self.model.running_state = RunningState::Done;
                None
            }
        })
    }

    fn select_next_mailbox(&mut self) -> Result<()> {
        self.model.mbox_list.select_next_mailbox();
        let emails = {
            let (account, Some(mailbox)) =
                &self.model.mbox_list.mailboxes[self.model.mbox_list.selected_mailbox]
            else {
                return Err(anyhow!("empty mailbox"));
            };
            self.load_mbox_emails(account, mailbox)
                .context("loading mailbox")?
        };
        self.model.emails_list.emails = emails;
        Ok(())
    }

    fn select_prev_mailbox(&mut self) -> Result<()> {
        self.model.mbox_list.select_prev_mailbox();
        let emails = {
            let (account, Some(mailbox)) =
                &self.model.mbox_list.mailboxes[self.model.mbox_list.selected_mailbox]
            else {
                return Err(anyhow!("empty mailbox"));
            };
            self.load_mbox_emails(account, mailbox)
                .context("loading mailbox")?
        };
        self.model.emails_list.emails = emails;
        Ok(())
    }

    fn load_mbox_emails(&self, account: &str, mailbox: &str) -> Result<Vec<Envelope>> {
        let mut stmt = self.db.prepare("SELECT a.name, a.email, message_id, in_reply_to, timestamp, internal_timestamp, subject
            FROM emails e
            JOIN email_addresses a
            ON e.mailbox = a.mailbox AND e.uid = a.uid
            WHERE a.type = 1 AND e.mailbox = ?1
            LIMIT 10"
        )?;
        let mut rows = stmt.query([mailbox])?;
        let mut result = Vec::with_capacity(10);
        while let Some(row) = rows.next()? {
            let ts: Option<i64> = row.get(4)?;
            let internal_ts: i64 = row.get(5)?;
            let timestamp = OffsetDateTime::from_unix_timestamp(ts.unwrap_or(internal_ts))?;
            let subject: Option<Box<str>> = row.get(6)?;
            let msg = Envelope {
                subject: subject.unwrap_or(Box::from("")),
                timestamp,
                addresses: vec![Address {
                    email: row.get(1)?,
                    name: row.get(0)?,
                }],
                seen: false,
                id: row.get(2)?,
                in_reply_to: row.get(3)?,
                thread_root: None,
            };
            result.push(msg);
        }

        Ok(result)
    }
}
