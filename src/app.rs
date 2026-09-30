use std::{
    collections::{HashMap, VecDeque},
    net::TcpStream,
    sync::{Arc, Mutex, RwLock, mpsc},
    thread,
    time::Duration,
};

use crate::{
    color_scheme::COLOR_SCHEME,
    content_widget::Content,
    emails_widget::EmailsList,
    mail::{self, Address, Email, get_mailboxes},
    mailboxes_widget::MailboxesList,
    model::{self, ActivePane, Message, Model, ReindexStatus, RunningState, ScrollDirection},
    util::centered_rect,
};
use anyhow::{Context, Result, anyhow};
use native_tls::TlsStream;
use ratatui::{
    Frame,
    crossterm::event::{self, Event, KeyCode},
    layout::{Constraint, Layout, Margin, Rect},
    style::{self, Style},
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

pub(crate) struct Envelope {
    pub(crate) uid: u32,
    pub(crate) subject: Box<str>,
    pub(crate) timestamp: OffsetDateTime,
    pub(crate) addresses: Vec<Address>, // from address for inboxes, to address for outboxes
    pub(crate) seen: bool,
    pub(crate) id: Option<Box<str>>,
    pub(crate) in_reply_to: Option<Box<str>>,
    pub(crate) thread_root: Option<Box<str>>,
}

enum EmailSelector {
    Latest,
    Before { ts: OffsetDateTime, uid: u32 },
    After { ts: OffsetDateTime, uid: u32 },
}

impl App {
    pub fn load(mut db: rusqlite::Connection, update: bool) -> Result<Self> {
        let mut sessions = mail::connect_to_accounts(&db).context("connecting to all accounts")?;
        let mut mailboxes = Vec::with_capacity(sessions.len());
        for (account, conn) in sessions.iter_mut() {
            let mailbox_names =
                get_mailboxes(account, update, conn, &mut db).context("listing mailboxes")?;
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
                status_bar_text: String::new(),
                mbox_pane: MailboxesList {
                    mailboxes,
                    selected_mailbox: 0,
                    list_state: ListState::default(),
                    focused: true,
                },
                emails_pane: EmailsList {
                    emails: VecDeque::new(),
                    selected_email: 0,
                    table_state: TableState::default(),
                    focused: false,
                },
                content_pane: Content::new(Box::from("")),
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
        r.select_next_mailbox()?;
        // r.model.emails_pane.table_state.select_next();

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
        self.status_bar(frame, layout[1]);
        match &self.model.running_state {
            RunningState::MainView => self.main_view(frame, layout[0]),
            RunningState::Done => {}
            RunningState::ComposeMail => todo!(),
            RunningState::Reindex => self.reindex_view(frame, layout[0]),
        }
    }

    fn status_bar(&self, frame: &mut Frame, area: Rect) {
        let c = Paragraph::new(self.model.status_bar_text.as_str()).style(
            Style::default()
                .bg(COLOR_SCHEME.status_bar_bg)
                .fg(COLOR_SCHEME.status_bar_fg),
        );
        frame.render_widget(c, area);
    }

    fn main_view(&mut self, frame: &mut Frame, area: Rect) {
        let columns =
            Layout::horizontal([Constraint::Percentage(30), Constraint::Fill(1)]).split(area);
        let right_rows = Layout::vertical([Constraint::Percentage(40), Constraint::Percentage(60)])
            .split(columns[1]);
        frame.render_widget(&mut self.model.mbox_pane, columns[0]);
        frame.render_widget(&mut self.model.emails_pane, right_rows[0]);
        frame.render_widget(&mut self.model.content_pane, right_rows[1]);
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
        let window_area = centered_rect(area, 50, 10, 8, 20);
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
        let label = Paragraph::new(format!("{}/{}", status.current, status.total))
            .style(style)
            .alignment(ratatui::layout::HorizontalAlignment::Left);
        let ratio = if status.total != 0 {
            status.current as f64 / status.total as f64
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
                KeyCode::Left | KeyCode::Char('h') => Some(Message::Scroll(ScrollDirection::Left)),
                KeyCode::Down | KeyCode::Char('j') => Some(Message::Scroll(ScrollDirection::Down)),
                KeyCode::Up | KeyCode::Char('k') => Some(Message::Scroll(ScrollDirection::Up)),
                KeyCode::Right | KeyCode::Char('l') => {
                    Some(Message::Scroll(ScrollDirection::Right))
                }
                KeyCode::Char('r') | KeyCode::F(5) => {
                    let (account, mailbox) = self.model.mbox_pane.current_mailbox();
                    Some(Message::ReindexMailbox { account, mailbox })
                }
                KeyCode::Tab => Some(Message::FocusNext),
                KeyCode::BackTab => Some(Message::FocusPrev),
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
            Message::Scroll(direction) => self.handle_scroll(direction)?,

            Message::PrevMailbox => self.select_prev_mailbox()?,
            Message::NextMessage => self.select_next_message()?,
            Message::PrevMessage => self.select_prev_message()?,
            Message::FocusNext => self.change_focus(true)?,
            Message::FocusPrev => self.change_focus(false)?,
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

    fn handle_scroll(&mut self, d: ScrollDirection) -> Result<Option<Message>> {
        match self.model.active_pane {
            ActivePane::Mailboxes => todo!(),
            ActivePane::Emails => todo!(),
            ActivePane::Content => todo!(),
        }
        todo!()
    }

    fn change_focus(&mut self, forward: bool) -> Result<Option<Message>> {
        self.model.active_pane = if forward {
            match self.model.active_pane {
                ActivePane::Mailboxes => ActivePane::Emails,
                ActivePane::Emails => ActivePane::Content,
                ActivePane::Content => ActivePane::Mailboxes,
            }
        } else {
            match self.model.active_pane {
                ActivePane::Mailboxes => ActivePane::Content,
                ActivePane::Emails => ActivePane::Mailboxes,
                ActivePane::Content => ActivePane::Emails,
            }
        };
        self.model.mbox_pane.focused = false;
        self.model.emails_pane.focused = false;
        self.model.content_pane.focused = false;
        match self.model.active_pane {
            ActivePane::Mailboxes => self.model.mbox_pane.focused = true,
            ActivePane::Emails => self.model.emails_pane.focused = true,
            ActivePane::Content => self.model.content_pane.focused = true,
        }
        Ok(None)
    }

    fn select_next_message(&mut self) -> Result<Option<Message>> {
        let last_email = self.model.emails_pane.select_next_email();
        let selected_email = &self.model.emails_pane.emails[self.model.emails_pane.selected_email];
        let (account, mailbox) = self.model.mbox_pane.current_mailbox();
        let mail = Email::load_from_db(
            &account,
            &mailbox,
            selected_email.uid,
            &self.db.lock().expect("poisoned"),
        )?;
        self.model.content_pane = Content::new(mail.body.unwrap_or_default());
        match last_email {
            None => {
                self.model.status_bar_text = format!(
                    "current {} len {}",
                    self.model.emails_pane.selected_email,
                    self.model.emails_pane.emails.len()
                );
                return Ok(None);
            }
            Some((ts, uid)) => {
                // widget returned last UID, so we need to load more emails and append them to its list
                let (account, mailbox) = self.model.mbox_pane.current_mailbox();
                let emails = self.load_mbox_emails(
                    &account,
                    &mailbox,
                    EmailSelector::Before { ts, uid },
                    10,
                )?;
                self.model.status_bar_text = format!(
                    "current {} len {}, loaded {} more",
                    self.model.emails_pane.selected_email,
                    self.model.emails_pane.emails.len(),
                    emails.len()
                );
                self.model.emails_pane.push_back_emails(emails);
            }
        }
        Ok(None)
    }

    fn select_prev_message(&mut self) -> Result<Option<Message>> {
        let first_email = self.model.emails_pane.select_prev_email();
        let selected_email = &self.model.emails_pane.emails[self.model.emails_pane.selected_email];
        let (account, mailbox) = self.model.mbox_pane.current_mailbox();
        let mail = Email::load_from_db(
            &account,
            &mailbox,
            selected_email.uid,
            &self.db.lock().expect("poisoned"),
        )?;
        self.model.content_pane = Content::new(mail.body.unwrap_or_default());
        match first_email {
            None => {
                self.model.status_bar_text = format!(
                    "current {} len {}",
                    self.model.emails_pane.selected_email,
                    self.model.emails_pane.emails.len()
                );
                return Ok(None);
            }
            Some((ts, uid)) => {
                // widget returned first UID, so we need to load more emails and add them to its list
                let (account, mailbox) = self.model.mbox_pane.current_mailbox();
                let emails = self.load_mbox_emails(
                    &account,
                    &mailbox,
                    EmailSelector::After { ts, uid },
                    10,
                )?;

                self.model.status_bar_text = format!(
                    "current {} len {}, loaded {} more",
                    self.model.emails_pane.selected_email,
                    self.model.emails_pane.emails.len(),
                    emails.len()
                );
                self.model.emails_pane.pop_front_emails(emails);
            }
        }
        Ok(None)
    }

    fn select_next_mailbox(&mut self) -> Result<Option<Message>> {
        self.model.mbox_pane.select_next_mailbox();
        let emails = {
            let (account, Some(mailbox)) =
                &self.model.mbox_pane.mailboxes[self.model.mbox_pane.selected_mailbox]
            else {
                return Err(anyhow!("empty mailbox"));
            };
            self.load_mbox_emails(account, mailbox, EmailSelector::Latest, 50)
                .context("loading mailbox")?
        };
        self.model.emails_pane.emails = VecDeque::from(emails);
        self.model.emails_pane.selected_email = 0;
        self.model.emails_pane.table_state.select(Some(0));
        Ok(None)
    }

    fn select_prev_mailbox(&mut self) -> Result<Option<Message>> {
        self.model.mbox_pane.select_prev_mailbox();
        let emails = {
            let (account, Some(mailbox)) =
                &self.model.mbox_pane.mailboxes[self.model.mbox_pane.selected_mailbox]
            else {
                return Err(anyhow!("empty mailbox"));
            };
            self.load_mbox_emails(account, mailbox, EmailSelector::Latest, 50)
                .context("loading mailbox")?
        };
        self.model.emails_pane.emails = VecDeque::from(emails);
        self.model.emails_pane.selected_email = 0;
        self.model.emails_pane.table_state.select(Some(0));
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
                        current: 0,
                        total: 0,
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

    fn load_mbox_emails(
        &self,
        account: &str,
        mailbox: &str,
        selector: EmailSelector,
        amount: i64,
    ) -> Result<Vec<Envelope>> {
        let db = self.db.lock().expect("poisoned");
        let condition = match selector {
            EmailSelector::Latest => "1",
            EmailSelector::Before { ts, uid } => {
                &format!("ts <= {} AND e.uid < {}", ts.unix_timestamp(), uid)
            }
            EmailSelector::After { ts, uid } => {
                &format!("ts >= {} AND e.uid > {}", ts.unix_timestamp(), uid)
            }
        };
        let mut stmt = db.prepare(&format!("SELECT e.uid, a.name, a.email, message_id, in_reply_to, COALESCE(timestamp, internal_timestamp) ts, subject
            FROM emails e
            JOIN email_addresses a
            ON e.account = a.account AND e.mailbox = a.mailbox AND e.uid = a.uid
            WHERE a.type = 1 AND e.account = ?1 AND e.mailbox = ?2 AND ( {} )
            ORDER BY ts DESC, e.uid DESC
            LIMIT ?3", condition)
        )?;
        let mut rows = stmt.query((account, mailbox, amount))?;
        let mut result = Vec::with_capacity(10);
        while let Some(row) = rows.next()? {
            let ts: i64 = row.get(5)?;
            let timestamp = OffsetDateTime::from_unix_timestamp(ts)?;
            let subject: Option<Box<str>> = row.get(6)?;
            let msg = Envelope {
                uid: row.get(0)?,
                subject: subject.unwrap_or(Box::from("")),
                timestamp,
                addresses: vec![Address {
                    email: row.get(2)?,
                    name: row.get(1)?,
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
