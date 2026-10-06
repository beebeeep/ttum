use std::collections::HashMap;
use std::fmt::Display;
use std::net::TcpStream;
use std::sync::mpsc;

use anyhow::Context;
use anyhow::Result;
use anyhow::anyhow;
use io_imap::client::ImapClient;
use io_imap::client::ImapClientStd;
use io_imap::client::ImapClientStdConnectOptions;
use io_imap::rfc3501::fetch::ImapMessageFetchOptions;
use io_imap::rfc3501::login::ImapLoginOptions;
use io_imap::rfc3501::select::ImapMailboxSelectOptions;
use io_imap::types::core::VecN;
use io_imap::types::extensions::enable::CapabilityEnable;
use io_imap::types::fetch::MessageDataItem;
use io_imap::types::fetch::MessageDataItemName;
use io_imap::types::flag::FlagFetch;
use io_imap::types::mailbox::Mailbox;
use io_imap::types::sequence::SequenceSet;
use mail_parser::MimeHeaders;
use native_tls::TlsConnector;
use native_tls::TlsStream;
use rusqlite::Connection;
use rusqlite::types::ToSqlOutput;
use time::Date;
use time::OffsetDateTime;
use time::Time;
use time::UtcOffset;
use time::format_description::well_known;

use crate::model;
use crate::model::Message;

enum AddrType {
    From,
    To,
    Sender,
    Cc,
    Bcc,
}

impl rusqlite::ToSql for AddrType {
    fn to_sql(&self) -> rusqlite::Result<rusqlite::types::ToSqlOutput<'_>> {
        Ok(match self {
            AddrType::From => ToSqlOutput::from(1),
            AddrType::To => ToSqlOutput::from(2),
            AddrType::Sender => ToSqlOutput::from(3),
            AddrType::Cc => ToSqlOutput::from(4),
            AddrType::Bcc => ToSqlOutput::from(5),
        })
    }
}

#[derive(Debug, Default)]
pub(crate) struct Address {
    pub(crate) email: Box<str>,
    pub(crate) name: Option<Box<str>>,
}

impl Display for Address {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.name {
            Some(n) => write!(f, "{n} <{}>", self.email),
            None => write!(f, "{}", self.email),
        }
    }
}

impl From<&Address> for Box<str> {
    fn from(a: &Address) -> Self {
        Box::from(format!("{a}"))
    }
}

impl From<&Address> for String {
    fn from(a: &Address) -> Self {
        format!("{a}")
    }
}

impl From<&imap_proto::types::Address<'_>> for Address {
    fn from(value: &imap_proto::Address<'_>) -> Self {
        Self {
            email: Box::from(format!(
                "{}@{}",
                parse_str(value.mailbox.unwrap_or_default()),
                parse_str(value.host.unwrap_or_default())
            )),
            name: value.name.map(parse_str),
        }
    }
}

impl From<&mail_parser::Addr<'_>> for Address {
    fn from(a: &mail_parser::Addr<'_>) -> Self {
        let email = match &a.address {
            Some(v) => Box::from(v.as_ref()),
            None => Box::from(""),
        };
        let name = match &a.name {
            Some(v) => Some(Box::from(v.as_ref())),
            None => None,
        };
        Self { email, name }
    }
}

impl Address {
    fn multiple(addr: &mail_parser::Address) -> Vec<Self> {
        match addr {
            mail_parser::Address::List(addrs) => addrs.iter().map(Address::from).collect(),
            mail_parser::Address::Group(groups) => groups
                .iter()
                .map(|group| {
                    group
                        .addresses
                        .iter()
                        .map(Address::from)
                        .collect::<Vec<Address>>()
                })
                .flatten()
                .collect(),
        }
    }
}

#[derive(Debug)]
pub(crate) struct Email {
    pub(crate) uid: u32,
    pub(crate) message_id: Option<Box<str>>,
    pub(crate) timestamp: Option<OffsetDateTime>,
    pub(crate) internal_timestamp: OffsetDateTime,
    pub(crate) from: Vec<Address>,
    pub(crate) to: Vec<Address>,
    pub(crate) sender: Vec<Address>,
    pub(crate) cc: Vec<Address>,
    pub(crate) bcc: Vec<Address>,
    pub(crate) subject: Option<Box<str>>,
    pub(crate) in_reply_to: Option<Box<str>>,
    pub(crate) seen: bool,
    pub(crate) body: Option<Box<str>>,
    pub(crate) attachements: Vec<(Box<str>, Vec<u8>)>,
}

impl Default for Email {
    fn default() -> Self {
        Self {
            internal_timestamp: OffsetDateTime::now_utc(),
            from: Default::default(),
            to: Default::default(),
            sender: Default::default(),
            cc: Default::default(),
            bcc: Default::default(),
            subject: Default::default(),
            in_reply_to: Default::default(),
            seen: Default::default(),
            body: Default::default(),
            attachements: Default::default(),
            uid: 0,
            message_id: None,
            timestamp: None,
        }
    }
}

impl Email {
    pub(crate) fn load_from_db(
        mailbox: &model::Mailbox,
        uid: u32,
        db: &Connection,
    ) -> Result<Self> {
        let email = db
            .query_one(
                "SELECT
                     message_id, timestamp, internal_timestamp, subject, in_reply_to, seen, body
                 FROM emails
                 WHERE account=?1 AND mailbox=?2 AND uid=?3",
                (&mailbox.account, &mailbox.mailbox, uid),
                |r| {
                    Ok(Email {
                        uid,
                        message_id: r.get(0)?,
                        timestamp: match r.get(1)? {
                            Some(v) => OffsetDateTime::from_unix_timestamp(v).ok(),
                            None => None,
                        },
                        internal_timestamp: OffsetDateTime::from_unix_timestamp(r.get(2)?).unwrap(),
                        from: Vec::with_capacity(1),
                        to: Vec::with_capacity(1),
                        sender: Vec::with_capacity(1),
                        cc: Vec::new(),
                        bcc: Vec::new(),
                        subject: r.get(3)?,
                        in_reply_to: r.get(4)?,
                        seen: r.get(5)?,
                        body: r.get(6)?,
                        attachements: Vec::new(),
                    })
                },
            )
            .context("loading email metadata")?;
        Ok(email)
    }

    pub(crate) fn save_to_db(&self, mailbox: &model::Mailbox, db: &mut Connection) -> Result<()> {
        let tx = db.transaction().context("starting transaction")?;
        tx.execute(
            "INSERT INTO emails (
                account, mailbox, uid, message_id, timestamp, internal_timestamp,
                subject, in_reply_to, seen, body
            ) VALUES (
                ?1, ?2, ?3, ?4, 
                ?5, ?6, ?7, ?8, ?9, ?10

            )",
            (
                &mailbox.account,
                &mailbox.mailbox,
                self.uid,
                &self.message_id,
                self.timestamp.map(|v| v.unix_timestamp()),
                self.internal_timestamp.unix_timestamp(),
                &self.subject,
                &self.in_reply_to,
                self.seen,
                &self.body,
            ),
        )?;
        for addr in &self.from {
            tx.execute("INSERT INTO email_addresses (account, mailbox, uid, type, name, email) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                    (&mailbox.account, &mailbox.mailbox, self.uid, AddrType::From, &addr.name, &addr.email )).context("inserting email address")?;
        }
        for addr in &self.to {
            tx.execute("INSERT INTO email_addresses (account, mailbox, uid, type, name, email) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                    (&mailbox.account, &mailbox.mailbox, self.uid, AddrType::To, &addr.name, &addr.email )).context("inserting email address")?;
        }
        for addr in &self.sender {
            tx.execute("INSERT INTO email_addresses (account, mailbox, uid, type, name, email) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                    (&mailbox.account, &mailbox.mailbox, self.uid, AddrType::Sender, &addr.name, &addr.email )).context("inserting email address")?;
        }
        for addr in &self.cc {
            tx.execute("INSERT INTO email_addresses (account, mailbox, uid, type, name, email) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                    (&mailbox.account, &mailbox.mailbox, self.uid, AddrType::Cc, &addr.name, &addr.email )).context("inserting email address")?;
        }
        for addr in &self.bcc {
            tx.execute("INSERT INTO email_addresses (account, mailbox, uid, type, name, email) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                    (&mailbox.account, &mailbox.mailbox, self.uid, AddrType::Bcc, &addr.name, &addr.email )).context("inserting email address")?;
        }

        for (name, data) in &self.attachements {
            tx.execute(
                "INSERT INTO email_attachements(account, mailbox, uid, name, content) VALUES(?1, ?2, ?3, ?4, ?5)",
                (&mailbox.account, &mailbox.mailbox, self.uid, name, data),
            )?;
        }
        tx.commit().context("committing transaction")?;
        Ok(())
    }
}

fn parse_str(s: &[u8]) -> Box<str> {
    // TODO: sometimes base64 comes with malformed padding, so we might need to manually restore it
    let decoder = rfc2047_decoder::Decoder::new()
        .too_long_encoded_word_strategy(rfc2047_decoder::RecoverStrategy::Decode);
    match decoder.decode(s) {
        Ok(s) => s.into_boxed_str(),
        Err(e) => {
            eprintln!("{:#} {}", e, String::from_utf8_lossy(s));
            Box::from(String::from_utf8_lossy(s))
        }
    }
}

fn parse_date(ts: &[u8]) -> Option<OffsetDateTime> {
    let Ok(ts) = str::from_utf8(ts) else {
        return None;
    };
    if let Ok(ts) = OffsetDateTime::parse(ts, &well_known::Rfc2822) {
        return Some(ts);
    }

    let f = time::macros::format_description!(
        version = 3,
        "[weekday repr:short], [day] [month repr:short case_sensitive:false] [year repr:full] [hour repr:24 padding:zero]:[minute padding:zero]:[second padding:zero] [offset_hour sign:mandatory]"
    );
    if let Ok(ts) = OffsetDateTime::parse(ts, &f) {
        return Some(ts);
    }

    let f = time::macros::format_description!(
        version = 3,
        "[day] [month repr:short case_sensitive:false] [year repr:full] [hour repr:24 padding:zero]:[minute padding:zero]:[second padding:zero] [offset_hour sign:mandatory]"
    );
    if let Ok(ts) = OffsetDateTime::parse(ts, &f) {
        return Some(ts);
    }
    None
}

pub fn connect_to_accounts(
    db: &Connection,
) -> Result<HashMap<Box<str>, imap::Session<TlsStream<TcpStream>>>> {
    let connector = TlsConnector::new().context("initializing TLS")?;
    let mut stmt =
        db.prepare("SELECT name, host, port, login, password, starttls FROM accounts")?;
    let mut rows = stmt.query([]).context("querying the database")?;
    let mut connections = HashMap::with_capacity(2);
    while let Some(r) = rows.next()? {
        let account: Box<str> = r.get(0)?;
        let host: Box<str> = r.get(1)?;
        let port: u16 = r.get(2)?;
        let user: Box<str> = r.get(3)?;
        let password: Box<str> = r.get(4)?;
        let starttls: bool = r.get(5)?;
        let client = if starttls {
            imap::connect_starttls((host.as_ref(), port), &host, &connector)
                .context("connecting to server")?
        } else {
            imap::connect_starttls((host.as_ref(), port), &host, &connector)
                .context("connecting to server")?
        };
        let mut session = client
            .login(user, password)
            .map_err(|e| e.0)
            .context(format!("authenticating with {account}"))?;
        session.run_command_and_check_ok("ENABLE UTF8=ACCEPT")?;
        connections.insert(account, session);
    }
    Ok(connections)
}

/*
pub fn index_email(
    account: &str,
    mailbox: &str,
    f: &imap::types::Fetch,
    db: &mut Connection,
) -> Result<()> {
    let envelope = f.envelope().context("no envelope")?;
    let internal_timestamp = match f.internal_date() {
        Some(d) => OffsetDateTime::from_unix_timestamp(d.timestamp())?,
        None => OffsetDateTime::now_utc(),
    };
    let timestamp = match envelope.date {
        Some(ts) => parse_date(ts),
        None => None,
    };
    let m = Email {
        uid: f.uid.context("no uid")?,
        message_id: envelope.message_id.map(parse_str),
        timestamp,
        internal_timestamp,
        from: envelope
            .from
            .as_ref()
            .map(|v| v.iter().map(|v| Address::from(v)).collect())
            .unwrap_or(Vec::new()),
        to: envelope
            .to
            .as_ref()
            .map(|v| v.iter().map(|v| Address::from(v)).collect())
            .unwrap_or(Vec::new()),
        sender: envelope
            .sender
            .as_ref()
            .map(|v| v.iter().map(|v| Address::from(v)).collect())
            .unwrap_or(Vec::new()),
        bcc: envelope
            .bcc
            .as_ref()
            .map(|v| v.iter().map(|v| Address::from(v)).collect())
            .unwrap_or(Vec::new()),
        cc: envelope
            .cc
            .as_ref()
            .map(|v| v.iter().map(|v| Address::from(v)).collect())
            .unwrap_or(Vec::new()),
        subject: envelope.subject.map(parse_str),
        in_reply_to: envelope.in_reply_to.map(parse_str),
        seen: f
            .flags()
            .iter()
            .any(|f| matches!(f, imap::types::Flag::Seen)),
        body: None,
        attachements: Vec::new(),
    };

    m.save_to_db(account, mailbox, db)
        .context("saving message to database")?;
    Ok(())
}
*/

pub fn reindex_mailbox(
    mailbox: &model::Mailbox,
    url: url::Url,
    user: &str,
    password: &str,
    starttls: bool,
    notifications: mpsc::Sender<Message>,
) -> Result<()> {
    let mut opts = ImapClientStdConnectOptions::default();
    opts.session.starttls = starttls;
    let (mut client, _) =
        ImapClientStd::connect(&url, opts).context("connecting to IMAP server")?;

    client
        .login(user, password, ImapLoginOptions::default())
        .context("authenticating with IMAP server")?;

    client
        .enable(VecN::from([CapabilityEnable::try_from("QRESYNC").unwrap()]))
        .context("requesting QRESYNC cap")?;
    let mbox = mailbox.mailbox.clone().into_string();
    let mbox = Mailbox::try_from(mbox).context("getting mailbox")?;
    let mb_metadata = client
        .select(mbox, ImapMailboxSelectOptions::default())
        .context("selecting mailbox")?;
    notifications.send(Message::MailboxMetadata {
        mailbox: mailbox.clone(),
        uid_validity: mb_metadata
            .uid_validity
            .context("getting UIDVALIDITY")?
            .into(),
        highest_mod_seq: mb_metadata
            .highest_mod_seq
            .context("getting HIGHEST_MOD_SEQ")?,
    })?;

    let fetches = client
        .fetch(
            "1:*".try_into().unwrap(),
            (vec![MessageDataItemName::Flags]).into(),
            ImapMessageFetchOptions {
                uid: true,
                ..Default::default()
            },
        )
        .context("fetching list of mails in mailbox")?;

    let mut uids: Vec<u32> = Vec::with_capacity(fetches.len());
    for (_, items) in fetches {
        uids.push(
            items
                .into_iter()
                .filter_map(|v| match v {
                    MessageDataItem::Uid(uid) => Some(uid.into()),
                    _ => None,
                })
                .next()
                .expect("no uid?"),
        );
    }

    let mut count = 0;
    let sz = uids.len();
    // fetch email bodies, 10 emails per request
    for chunk in uids.chunks(10) {
        count += chunk.len();
        let range: String = chunk
            .iter()
            .map(|v| v.to_string())
            .collect::<Vec<_>>()
            .join(",");
        let fetches = client
            .fetch(
                range.as_str().try_into().unwrap(),
                vec![
                    MessageDataItemName::Flags,
                    MessageDataItemName::InternalDate,
                    MessageDataItemName::BodyExt {
                        section: None,
                        partial: None,
                        peek: true,
                    },
                ]
                .into(),
                ImapMessageFetchOptions {
                    uid: true,
                    ..Default::default()
                },
            )
            .context("fetching emails")?;

        let mut emails = Vec::with_capacity(10);
        for (_, items) in fetches {
            // process each fetch and create email struct from body and metadata
            let mut items = items.into_iter();
            let (
                Some(MessageDataItem::Uid(uid)),
                Some(MessageDataItem::Flags(flags)),
                Some(MessageDataItem::InternalDate(idate)),
                Some(MessageDataItem::BodyExt {
                    section: _,
                    origin: _,
                    data: body,
                }),
            ) = (items.next(), items.next(), items.next(), items.next())
            else {
                return Err(anyhow!("unexpected fetch result"));
            };

            let body = body.into_option().context("no email body")?;
            let msg = mail_parser::MessageParser::default()
                .parse(&body)
                .context("parsing mail")?;
            let timestamp = match msg.date() {
                Some(d) => parse_datetime(d),
                None => None,
            };
            let internal_timestamp =
                OffsetDateTime::from_unix_timestamp(idate.as_ref().timestamp())
                    .context("invalid internal timestamp")?;
            let seen = flags
                .iter()
                .any(|v| matches!(v, FlagFetch::Flag(io_imap::types::flag::Flag::Seen)));
            emails.push(Email {
                uid: uid.into(),
                message_id: msg.message_id().map(Box::from),
                timestamp,
                internal_timestamp,
                from: msg.from().map(Address::multiple).unwrap_or_default(),
                to: msg.to().map(Address::multiple).unwrap_or_default(),
                sender: msg.sender().map(Address::multiple).unwrap_or_default(),
                cc: msg.cc().map(Address::multiple).unwrap_or_default(),
                bcc: msg.bcc().map(Address::multiple).unwrap_or_default(),
                subject: msg.subject().map(Box::from),
                in_reply_to: if let mail_parser::HeaderValue::Text(v) = msg.in_reply_to() {
                    Some(Box::from(v.as_ref()))
                } else {
                    None
                },
                seen,
                body: msg.body_text(0).map(|v| Box::from(v.as_ref())),
                attachements: msg
                    .attachments()
                    .enumerate()
                    .map(|(idx, a)| {
                        (
                            a.attachment_name()
                                .map(Box::from)
                                .unwrap_or_else(|| format!("attachement-{idx}").into_boxed_str()),
                            a.contents().to_vec(),
                        )
                    })
                    .collect(),
            });
        }
        let _ = notifications.send(Message::Batch(vec![
            Message::ReindexStatus(crate::model::ReindexStatus {
                mailbox: mailbox.clone(),
                current: count,
                total: sz,
                done: false,
            }),
            Message::NewEmails {
                mailbox: mailbox.clone(),
                emails,
            },
        ]));
    }

    Ok(())
}

fn parse_datetime(ts: &mail_parser::DateTime) -> Option<OffsetDateTime> {
    Some(OffsetDateTime::new_in_offset(
        Date::from_calendar_date(
            ts.year as i32,
            time::Month::try_from(ts.month).ok()?,
            ts.day,
        )
        .ok()?,
        Time::from_hms(ts.hour, ts.minute, ts.second).ok()?,
        UtcOffset::from_hms(
            ts.tz_hour as i8 * if ts.tz_before_gmt { -1 } else { 1 },
            ts.tz_minute as i8,
            0,
        )
        .ok()?,
    ))
}

pub(crate) fn get_mailboxes(
    account: &str,
    update: bool,
    session: &mut imap::Session<TlsStream<TcpStream>>,
    db: &mut Connection,
) -> Result<Vec<String>> {
    let mut mailboxes = Vec::with_capacity(3);
    if !update {
        let mut stmt = db.prepare(
            "SELECT name FROM mailboxes WHERE account=?1 AND hidden = 0 ORDER BY lower(name) ASC",
        )?;
        let mut rows = stmt.query([account])?;
        while let Some(row) = rows.next()? {
            mailboxes.push(row.get(0)?);
        }
        return Ok(mailboxes);
    }
    let list = session.list(None, Some("*")).context("listing mailboxes")?;
    let tx = db.transaction().context("starting transaction")?;
    {
        let mut stmt = tx.prepare(
            "
        INSERT into mailboxes (name, account, uid_validity)
        VALUES (?1, ?2, ?3)
        ON CONFLICT (account, name) DO UPDATE SET uid_validity=excluded.uid_validity",
        )?;
        for v in &list {
            let mailbox = utf7_imap::decode_utf7_imap(String::from(v.name()));
            let mbox = session.select(v.name()).context("opening mailbox")?;
            stmt.execute((&mailbox, account, mbox.uid_validity))?;
            println!("{account} {mailbox}");
            mailboxes.push(mailbox);
        }
    }
    tx.commit().context("committing transaction")?;
    Ok(mailboxes)
}

mod test {
    #[test]
    fn test_parser() {
        let s = "=?UTF-8?B?TWFudGFzIE1pa8WheXM=?=";
        println!("{:?}", rfc2047_decoder::decode(s.as_bytes()));
    }
}
