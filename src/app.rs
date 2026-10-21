use std::{collections::VecDeque, sync::mpsc, thread, time::Duration};

use crate::{
    color_scheme::COLOR_SCHEME,
    content_widget::Content,
    emails_widget::EmailsList,
    imap::{self, get_account_mailboxes},
    mail::{self, Account, Address, Email, Envelope, Mailbox, MailboxWithMetadata},
    mailboxes_widget::MailboxesList,
    model::{
        self, ActivePane, EmailSelector, Message, Model, ReindexStatus, RunningState,
        ScrollDirection,
    },
    util::centered_rect,
};
use anyhow::{Context, Result, anyhow};
use ratatui::{
    Frame,
    crossterm::event::{self, Event, KeyCode},
    layout::{Constraint, Layout, Margin, Rect},
    style::Style,
    widgets::{Block, Clear, LineGauge, ListState, Paragraph, TableState, Widget},
};
use time::OffsetDateTime;
use tracing::{debug, error};

pub struct App {
    model: Model,

    db: rusqlite::Connection,
    events_rx: mpsc::Receiver<Message>,
    events_tx: mpsc::Sender<Message>,
}

impl App {
    pub fn load(mut db: rusqlite::Connection, update: bool) -> Result<Self> {
        let mailboxes =
            MailboxWithMetadata::load_all(update, &mut db).context("loading mailboxes")?;

        let (events_tx, events_rx) = mpsc::channel();
        let r = Self {
            model: Model {
                running_state: RunningState::MainView,
                active_pane: ActivePane::Mailboxes,
                status_bar_text: String::new(),
                mbox_pane: MailboxesList::new(
                    &(mailboxes
                        .iter()
                        .map(|v| v.inner.clone())
                        .collect::<Vec<Mailbox>>()),
                ),
                emails_pane: EmailsList {
                    emails: VecDeque::new(),
                    selected_email: 0,
                    table_state: TableState::default(),
                    focused: false,
                },
                content_pane: Content::new(),
                reindex_status: ReindexStatus::default(),
            },
            db,
            events_rx,
            events_tx,
        };

        // r.model.emails_pane.table_state.select_next();

        Ok(r)
    }

    pub fn run(mut self) -> Result<()> {
        let mut terminal = ratatui::try_init()?;

        let key_events = self.events_tx.clone();
        thread::spawn(move || Self::event_poller(key_events));

        self.start_mailbox_updaters()
            .context("starting background mailbox updaters")?;
        self.start_inbox_watchers()
            .context("starting background inbox watchers")?;

        terminal.draw(|f| self.view(f))?;
        while self.model.running_state != RunningState::Done {
            let mut current_msg = Some(self.events_rx.recv().context("event channel died")?);

            while let Some(msg) = current_msg.take() {
                current_msg = match self.update(msg) {
                    Ok(msg) => msg,
                    Err(e) => {
                        error!("error while processing message: {e:#}");
                        continue;
                    }
                };
            }
            // Render the current view
            terminal.draw(|f| self.view(f))?;
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
            .title(format!(" Reindexing {} ", status.mailbox))
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
                KeyCode::Char('r') | KeyCode::F(5) => Some(Message::ReindexMailbox(
                    self.model.mbox_pane.current_mailbox(),
                )),
                KeyCode::Char('v') if matches!(self.model.active_pane, ActivePane::Mailboxes) => {
                    Some(Message::ToggleMailboxVisibility {
                        mailbox: self.model.mbox_pane.current_mailbox(),
                        visible: false,
                    })
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
            Message::Scroll(direction) => self.handle_scroll(direction),
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
            Message::ReindexMailbox(mailbox) => self
                .start_mailbox_reindex(&mailbox)
                .context("starting mailbox reindex")?,
            Message::LoadMoreMails(selector) => self
                .load_more_emails(&self.model.mbox_pane.current_mailbox(), &selector)
                .context("loading more emails to view")?,
            Message::MailboxChange(mailbox) => {
                self.load_mailbox(&mailbox).context("loading mailbox")?
            }
            Message::Batch(messages) => self
                .process_batch(messages)
                .context("processing batch message")?,
            Message::SelectedEmail(uid) => self.load_email(uid).context("loading email content")?,
            Message::MailboxMetadata(mailbox) => {
                mailbox
                    .save_to_db(&self.db)
                    .context("updating mailbox metadata")?;
                None
            }
            Message::NewEmails { mailbox, emails } => self
                .save_emails(&mailbox, emails)
                .context("saving new emails")?,
            Message::ToggleMailboxVisibility { mailbox, visible } => self
                .set_mailbox_visibility(&mailbox, visible)
                .context("setting mailbox visibility")?,
            Message::VanishedEmails { mailbox, uids } => {
                Email::drop_uids(&mailbox, &uids, &mut self.db)
                    .context("dropping vanished emails")?;
                None
            }
            Message::StatusMessage(msg) => {
                self.model.status_bar_text = msg.into_string();
                None
            }
        })
    }

    fn process_batch(&mut self, messages: Vec<Message>) -> Result<Option<Message>> {
        let mut results = Vec::new();
        for msg in messages {
            if let Some(r) = self.update(msg)? {
                results.push(r)
            }
        }
        if results.len() > 1 {
            Ok(Some(Message::Batch(results)))
        } else {
            Ok(results.pop())
        }
    }

    fn set_mailbox_visibility(
        &mut self,
        mailbox: &Mailbox,
        visible: bool,
    ) -> Result<Option<Message>> {
        let hidden = if visible { 0 } else { 1 };
        self.model.status_bar_text = format!("hiding {mailbox}");
        self.db.execute(
            "UPDATE mailboxes SET hidden=?1 WHERE account = ?2 AND name = ?3",
            (hidden, &mailbox.account, &mailbox.mailbox),
        )?;
        if !visible {
            Ok(self.model.mbox_pane.hide_current_mailbox())
        } else {
            Ok(None)
        }
    }

    fn load_email(&mut self, uid: u32) -> Result<Option<Message>> {
        let mailbox = self.model.mbox_pane.current_mailbox();
        self.model.content_pane.email = Email::load_from_db(&mailbox, uid, &self.db)?;
        Ok(None)
    }

    fn save_emails(&mut self, mailbox: &Mailbox, emails: Vec<Email>) -> Result<Option<Message>> {
        for email in emails {
            email.save_to_db(mailbox, &mut self.db)?;
        }
        Ok(None)
    }

    fn handle_scroll(&mut self, d: ScrollDirection) -> Option<Message> {
        match self.model.active_pane {
            ActivePane::Mailboxes => self.model.mbox_pane.scroll(d),
            ActivePane::Emails => self.model.emails_pane.scroll(d),
            ActivePane::Content => self.model.content_pane.scroll(d),
        }
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

    fn load_mailbox(&mut self, mailbox: &Mailbox) -> Result<Option<Message>> {
        let emails = {
            self.load_mbox_emails(mailbox, &EmailSelector::Latest, 50)
                .context("loading mailbox")?
        };
        self.model.emails_pane.emails = VecDeque::from(emails);
        self.model.emails_pane.selected_email = 0;
        self.model.emails_pane.table_state.select(Some(0));

        let account = Account::load(&mailbox.account, &self.db)
            .context(format!("loading account {mailbox}"))?;
        let notifications = self.events_tx.clone();
        // thread::spawn(move || {
        //     if let Err(e) = imap::watch_mailbox(&account, &mailbox, notifications.clone()) {
        //         error!("watch thread for {mailbox} failed: {e}");
        //     }
        // });

        Ok(None)
    }

    fn load_more_emails(
        &mut self,
        mailbox: &Mailbox,
        selector: &EmailSelector,
    ) -> Result<Option<Message>> {
        let emails = {
            self.load_mbox_emails(mailbox, selector, 10)
                .context("loading mailbox")?
        };
        match selector {
            EmailSelector::Latest => {}
            EmailSelector::Before { .. } => self.model.emails_pane.append_emails(emails),
            EmailSelector::After { .. } => self.model.emails_pane.prepend_emails(emails),
        }
        Ok(None)
    }

    fn start_mailbox_reindex(&mut self, mailbox: &Mailbox) -> Result<Option<Message>> {
        self.model.running_state = RunningState::Reindex;
        self.model.reindex_status = ReindexStatus {
            mailbox: mailbox.clone(),
            ..Default::default()
        };
        let notifications = self.events_tx.clone();
        self.db
            .execute(
                "DELETE FROM emails WHERE account=?1 AND mailbox=?2",
                [&mailbox.account, &mailbox.mailbox],
            )
            .context("cleaning up db")?;
        self.db
            .execute(
                "DELETE FROM email_attachements WHERE account=?1 AND mailbox=?2",
                [&mailbox.account, &mailbox.mailbox],
            )
            .context("cleaning up db")?;

        let mb = mailbox.clone();
        let account = Account::load(&mb.account, &self.db)?;
        thread::spawn(
            move || match imap::reindex_mailbox(&account, &mb, notifications.clone()) {
                Ok(_) => {
                    let _ = notifications.send(Message::ReindexStatus(ReindexStatus {
                        mailbox: mb,
                        current: 0,
                        total: 0,
                        done: true,
                    }));
                }
                Err(e) => {
                    println!("{e:#}");
                }
            },
        );
        Ok(None)
    }

    fn load_mbox_emails(
        &self,
        mailbox: &Mailbox,
        selector: &EmailSelector,
        amount: i64,
    ) -> Result<Vec<Envelope>> {
        let condition = match selector {
            EmailSelector::Latest => "1",
            EmailSelector::Before { ts, uid } => {
                &format!("ts <= {} AND e.uid < {}", ts.unix_timestamp(), uid)
            }
            EmailSelector::After { ts, uid } => {
                &format!("ts >= {} AND e.uid > {}", ts.unix_timestamp(), uid)
            }
        };
        let mut stmt = self.db.prepare(&format!("SELECT e.uid, a.name, a.email, message_id, in_reply_to, COALESCE(timestamp, internal_timestamp) ts, subject
            FROM emails e
            JOIN email_addresses a
            ON e.account = a.account AND e.mailbox = a.mailbox AND e.uid = a.uid
            WHERE a.type = 1 AND e.account = ?1 AND e.mailbox = ?2 AND ( {} )
            ORDER BY ts DESC, e.uid DESC
            LIMIT ?3", condition)
        )?;
        let mut rows = stmt.query((&mailbox.account, &mailbox.mailbox, amount))?;
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

    fn start_mailbox_updaters(&mut self) -> Result<()> {
        let accounts = Account::load_all(&self.db).context("loading accounts")?;
        for (_, account) in accounts {
            let notifications = self.events_tx.clone();
            let acc_mailboxes = account
                .get_mailboxes(&self.db)
                .context("loading mailboxes for account")?;
            thread::spawn(move || {
                let Ok(client) = account.get_client().context("connecting to account") else {
                    error!("failed to get client for {}", account.name);
                    return;
                };
                imap::mailboxes_update_loop(
                    client,
                    acc_mailboxes,
                    Duration::from_secs(60),
                    notifications,
                );
            });
        }
        Ok(())
    }

    fn start_inbox_watchers(&mut self) -> Result<()> {
        let accounts = Account::load_all(&self.db).context("loading accounts")?;
        for (_, account) in accounts {
            let notifications = self.events_tx.clone();
            thread::spawn(move || {
                let mailbox = Mailbox {
                    account: account.name.clone(),
                    mailbox: Box::from("Inbox"),
                };
                if let Err(e) = imap::watch_mailbox(&account, &mailbox, notifications) {
                    error!("inbox watcher terminated: {e:#}");
                }
            });
        }
        Ok(())
    }
}
