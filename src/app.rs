use std::{
    collections::HashMap,
    net::TcpStream,
    sync::{Arc, Mutex, RwLock, mpsc},
    thread,
    time::Duration,
};

use crate::{
    color_scheme::COLOR_SCHEME,
    emails_widget::EmailsList,
    mail::{self, Address, Envelope, get_mailboxes},
    mailboxes_widget::MailboxesList,
    model::{self, ActivePane, Message, Model, ReindexStatus, RunningState},
    util::centered_rect,
};
use anyhow::{Context, Result, anyhow};
use native_tls::TlsStream;
use ratatui::{
    Frame,
    crossterm::event::{self, Event, KeyCode},
    layout::{Constraint, Layout, Margin, Rect},
    style::Style,
    widgets::{Block, Clear, LineGauge, ListState, Paragraph, TableState, Widget},
};
use time::OffsetDateTime;

pub struct App {
    model: Model,

    db: Arc<Mutex<rusqlite::Connection>>,
    events_rx: mpsc::Receiver<Message>,
    events_tx: mpsc::Sender<Message>,
    imap_sessions: HashMap<Box<str>, Arc<Mutex<imap::Session<TlsStream<TcpStream>>>>>,
}

impl App {
    pub fn load(db: rusqlite::Connection) -> Result<Self> {
        let mut sessions = mail::connect_to_accounts(&db).context("connecting to all accounts")?;
        let mut mailboxes = Vec::with_capacity(sessions.len());
        for (account, conn) in sessions.iter_mut() {
            let mailbox_names =
                get_mailboxes(account, false, conn, &db).context("listing mailboxes")?;
            mailboxes.push((account.clone(), None));
            for n in mailbox_names {
                mailboxes.push((account.clone(), Some(n.into_boxed_str())));
            }
        }
        let (events_tx, events_rx) = mpsc::channel();
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
                reindex_status: ReindexStatus::default(),
            },
            db: Arc::new(Mutex::new(db)),
            events_rx,
            events_tx,
            imap_sessions: HashMap::from_iter(
                sessions
                    .into_iter()
                    .map(|(account, sess)| (account, Arc::new(Mutex::new(sess)))),
            ),
        };
        r.model.mbox_list.select_next_mailbox();
        r.model.emails_list.table_state.select_next();

        Ok(r)
    }

    pub fn run(mut self) -> Result<()> {
        let mut terminal = ratatui::try_init()?;

        let key_events = self.events_tx.clone();
        thread::spawn(move || Self::event_poller(key_events));

        // self.model.running_state = RunningState::Reindex;
        // self.model.reindex_status = ReindexStatus {
        //     account: Box::from("test"),
        //     mailbox: Box::from("mbox"),
        //     current_uid: 1000,
        //     max_uid: 10000,
        //     done: false,
        // };
        while self.model.running_state != RunningState::Done {
            // Render the current view
            terminal.draw(|f| self.view(f))?;

            let mut current_msg = match self.events_rx.recv_timeout(Duration::from_millis(66)) {
                Ok(msg) => Some(msg),
                Err(e) => match e {
                    mpsc::RecvTimeoutError::Timeout => None,
                    mpsc::RecvTimeoutError::Disconnected => {
                        return Err(anyhow!("channel dead"));
                    }
                },
            };

            while current_msg.is_some() {
                current_msg = self.update(current_msg.unwrap())?;
            }
        }
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
            RunningState::Reindex => self.reindex_view(frame, layout[0]),
        }
    }

    fn main_view(&mut self, frame: &mut Frame, area: Rect) {
        let columns =
            Layout::horizontal([Constraint::Percentage(30), Constraint::Fill(1)]).split(area);
        frame.render_widget(&mut self.model.mbox_list, columns[0]);
        frame.render_widget(&mut self.model.emails_list, columns[1]);
    }

    fn event_poller(key_events: mpsc::Sender<Message>) {
        loop {
            match event::read() {
                Ok(Event::Key(key)) => {
                    if key.kind == event::KeyEventKind::Press {
                        if let Err(_) = key_events.send(Message::KeyPress(key)) {
                            return;
                        }
                    }
                }
                Ok(_) => {}
                Err(_) => return,
            }
        }
    }

    fn reindex_view(&mut self, frame: &mut Frame, area: Rect) {
        self.main_view(frame, area);
        let window_area = centered_rect(area, 50, 10);
        let style = Style::default()
            .fg(COLOR_SCHEME.text_fg)
            .bg(COLOR_SCHEME.text_bg);
        let status = &self.model.reindex_status;

        let block = Block::bordered()
            .title(format!(
                " Reindexing {}/{} ",
                status.account, status.mailbox
            ))
            .title_alignment(ratatui::layout::Alignment::Left)
            .border_type(ratatui::widgets::BorderType::Rounded)
            .border_style(style);
        let layout = Layout::horizontal(vec![Constraint::Fill(1), Constraint::Fill(9)])
            .spacing(1)
            .split(block.inner(window_area).inner(Margin::new(2, 2)));
        let label = Paragraph::new(format!("{}/{}", status.current_uid, status.max_uid))
            .style(style)
            .alignment(ratatui::layout::HorizontalAlignment::Left);
        let ratio = if status.max_uid != 0 {
            status.current_uid as f64 / status.max_uid as f64
        } else {
            0f64
        };
        let bar = LineGauge::default()
            .style(style)
            .filled_symbol("⣿")
            .unfilled_symbol("⣿")
            .filled_style(
                style
                    .fg(COLOR_SCHEME.progress_fg)
                    .bg(COLOR_SCHEME.progress_bg),
            )
            .unfilled_style(style)
            .ratio(ratio);

        frame.render_widget(Clear::default(), window_area);
        frame.render_widget(block, window_area);
        frame.render_widget(label, layout[0]);
        frame.render_widget(bar, layout[1]);
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
                KeyCode::Char('r') | KeyCode::F(5) => {
                    let (account, mailbox) = self.model.mbox_list.current_mailbox();
                    Some(Message::ReindexMailbox { account, mailbox })
                }
                _ => None,
            },
            RunningState::Done => None,
            RunningState::ComposeMail => None,
            RunningState::Reindex => match key.code {
                KeyCode::Char('q') => Some(Message::Quit),
                _ => None,
            },
        }
    }

    fn update(&mut self, msg: Message) -> Result<Option<Message>> {
        Ok(match msg {
            Message::KeyPress(k) => self.handle_key(k),
            Message::NextMailbox => self.select_next_mailbox()?,

            Message::PrevMailbox => self.select_prev_mailbox()?,
            Message::NextMessage => todo!(),
            Message::PrevMessage => todo!(),
            Message::FocusNext => todo!(),
            Message::FocusPrev => todo!(),
            Message::Quit => {
                self.model.running_state = RunningState::Done;
                None
            }
            Message::ReindexStatus(status) => {
                if status.done {
                    self.model.running_state = RunningState::MainView
                }
                self.model.reindex_status = status;
                None
            }
            Message::ReindexMailbox { account, mailbox } => {
                self.start_mailbox_reindex(&account, &mailbox)?
            }
        })
    }

    fn select_next_mailbox(&mut self) -> Result<Option<Message>> {
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
        Ok(None)
    }

    fn select_prev_mailbox(&mut self) -> Result<Option<Message>> {
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
        Ok(None)
    }

    fn start_mailbox_reindex(&mut self, account: &str, mailbox: &str) -> Result<Option<Message>> {
        self.model.running_state = RunningState::Reindex;
        self.model.reindex_status = ReindexStatus {
            account: Box::from(account),
            mailbox: Box::from(mailbox),
            ..Default::default()
        };
        let notifications = self.events_tx.clone();
        let session = self
            .imap_sessions
            .get(account)
            .context("unknown mailbox")?
            .clone();
        let db = self.db.clone();
        let account = Box::from(account);
        let mailbox = Box::from(mailbox);
        thread::spawn(move || {
            let mut session = session.lock().expect("poisoned mutex");
            let mut db = db.lock().expect("poiosoned mutext");
            match mail::reindex_mailbox(
                &account,
                &mailbox,
                &mut session,
                &mut db,
                notifications.clone(),
            ) {
                Ok(_) => {
                    let _ = notifications.send(Message::ReindexStatus(ReindexStatus {
                        account: account,
                        mailbox: mailbox,
                        current_uid: 0,
                        max_uid: 0,
                        done: true,
                    }));
                }
                Err(e) => {
                    println!("{e:#}");
                }
            }
        });
        Ok(None)
    }

    fn load_mbox_emails(&self, account: &str, mailbox: &str) -> Result<Vec<Envelope>> {
        let db = self.db.lock().expect("poisoned");
        let mut stmt = db.prepare("SELECT a.name, a.email, message_id, in_reply_to, timestamp, internal_timestamp, subject
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
