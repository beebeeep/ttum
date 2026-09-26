use std::collections::HashMap;
use std::fmt::Display;
use std::net::TcpStream;

use anyhow::Context;
use anyhow::Result;
use native_tls::TlsConnector;
use native_tls::TlsStream;
use rusqlite::Connection;
use rusqlite::types::ToSqlOutput;
use time::Date;
use time::OffsetDateTime;
use time::Time;
use time::UtcOffset;
use time::format_description::well_known;

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

struct Email {
    uid: u32,
    message_id: Option<Box<str>>,
    timestamp: Option<OffsetDateTime>,
    internal_timestamp: OffsetDateTime,
    from: Vec<Address>,
    to: Vec<Address>,
    sender: Vec<Address>,
    cc: Vec<Address>,
    bcc: Vec<Address>,
    subject: Option<Box<str>>,
    in_reply_to: Option<Box<str>>,
    seen: bool,
    body: Option<Box<str>>,
}

impl Email {
    fn save_to_db(&self, mailbox: &str, db: &mut Connection) -> Result<()> {
        let tx = db.transaction().context("starting transaction")?;
        tx.execute(
            "INSERT INTO emails (
                mailbox, uid, message_id, timestamp, internal_timestamp,
                subject, in_reply_to, seen, body
            ) VALUES (
                ?1, ?2, ?3, ?4,
                ?5, ?6, ?7, ?8, ?9

            )",
            (
                mailbox,
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
            tx.execute("INSERT INTO email_addresses (mailbox, uid, type, name, email) VALUES (?1, ?2, ?3, ?4, ?5)",
                    (mailbox, self.uid, AddrType::From, &addr.name, &addr.email )).context("inserting email address")?;
        }
        for addr in &self.to {
            tx.execute("INSERT INTO email_addresses (mailbox, uid, type, name, email) VALUES (?1, ?2, ?3, ?4, ?5)",
                    (mailbox, self.uid, AddrType::To, &addr.name, &addr.email )).context("inserting email address")?;
        }
        for addr in &self.sender {
            tx.execute("INSERT INTO email_addresses (mailbox, uid, type, name, email) VALUES (?1, ?2, ?3, ?4, ?5)",
                    (mailbox, self.uid, AddrType::Sender, &addr.name, &addr.email )).context("inserting email address")?;
        }
        for addr in &self.cc {
            tx.execute("INSERT INTO email_addresses (mailbox, uid, type, name, email) VALUES (?1, ?2, ?3, ?4, ?5)",
                    (mailbox, self.uid, AddrType::Cc, &addr.name, &addr.email )).context("inserting email address")?;
        }
        for addr in &self.bcc {
            tx.execute("INSERT INTO email_addresses (mailbox, uid, type, name, email) VALUES (?1, ?2, ?3, ?4, ?5)",
                    (mailbox, self.uid, AddrType::Bcc, &addr.name, &addr.email )).context("inserting email address")?;
        }

        tx.commit().context("committing transaction")?;
        Ok(())
    }
}

pub(crate) struct Envelope {
    pub(crate) subject: Box<str>,
    pub(crate) timestamp: OffsetDateTime,
    pub(crate) addresses: Vec<Address>, // from address for inboxes, to address for outboxes
    pub(crate) seen: bool,
    pub(crate) id: Option<Box<str>>,
    pub(crate) in_reply_to: Option<Box<str>>,
    pub(crate) thread_root: Option<Box<str>>,
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
        eprintln!("connecting to {host}:{port}, starttls: {starttls}");
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

pub fn index_email(mailbox: &str, f: &imap::types::Fetch, db: &mut Connection) -> Result<()> {
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
    };

    m.save_to_db(mailbox, db)
        .context("saving message to database")?;
    Ok(())
}

pub fn reindex_mailbox(
    account: &str,
    mailbox: &str,
    session: &mut imap::Session<TlsStream<TcpStream>>,
    db: &mut Connection,
) -> Result<()> {
    db.execute("DELETE FROM emails WHERE mailbox=?1", [mailbox])
        .context("cleaning up db")?;
    db.execute("DELETE FROM email_attachements WHERE mailbox=?1", [mailbox])
        .context("cleaning up db")?;

    let mbox = session.select(mailbox).context("opening mailbox")?;
    if let Some(v) = mbox.uid_validity {
        db.execute(
            "INSERT into mailboxes (name, account, uid_validity)
             VALUES (?1, ?2, ?3)
             ON CONFLICT (name) DO UPDATE SET uid_validity=excluded.uid_validity",
            (mailbox, account, v),
        )
        .context("updating uid_validity")?;
    }
    let max_uid = mbox.uid_next.context("no next uid?")?;

    for uid in 1..max_uid {
        fetch_email(mailbox, uid, session, db)?;
        if uid % 10 == 0 {
            println!("{uid} of {max_uid} fetched");
        }
    }
    // let mut range = (1, 50)xx;
    // let max_uid = 100;
    // loop {
    //     let fetch = session
    //         .uid_fetch(
    //             format!("{}:{}", range.0, range.1),
    //             "(FLAGS INTERNALDATE ENVELOPE)",
    //         )
    //         .context("fetching messages")?;
    //     for f in fetch.iter() {
    //         index_email(mailbox, f, db)?;
    //     }
    //     range = (range.1 + 1, range.1 + 51);
    //     if range.0 >= max_uid {
    //         break;
    //     }
    //     println!("{range:?}");
    // }

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

fn fetch_email(
    mailbox: &str,
    uid: u32,
    session: &mut imap::Session<TlsStream<TcpStream>>,
    db: &mut Connection,
) -> Result<()> {
    let fetch = session
        .uid_fetch(format!("{uid}"), "(FLAGS INTERNALDATE BODY[])")
        .context("fetching the message")?;
    if fetch.is_empty() {
        return Ok(());
    }
    let body = fetch[0].body().unwrap();
    let msg = mail_parser::MessageParser::default()
        .parse(body)
        .context("parsing mail")?;
    let internal_timestamp = match fetch[0].internal_date() {
        Some(d) => OffsetDateTime::from_unix_timestamp(d.timestamp())?,
        None => OffsetDateTime::now_utc(),
    };
    let timestamp = match msg.date() {
        Some(d) => parse_datetime(d),
        None => None,
    };
    let email = Email {
        uid: fetch[0].uid.context("no uid")?,
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
        seen: fetch[0]
            .flags()
            .iter()
            .any(|f| matches!(f, imap::types::Flag::Seen)),
        body: msg.body_text(0).map(|v| Box::from(v.as_ref())),
    };

    email
        .save_to_db(mailbox, db)
        .context("saving message to database")?;
    Ok(())
}

mod test {
    #[test]
    fn test_parser() {
        let s = "=?utf-8?B?0J3QldCe0JPQoNCQ0J3QmNCn0JXQndCd0KvQmSDQmNCd0KLQldCg0J3QldCi?= =?utf-8?B?INCYINCR0JXQodCf0JvQkNCi0J3Qq9CZINCg0J7Qo9Ci0JXQoCE=?=";
        println!("{:?}", rfc2047_decoder::decode(s.as_bytes()));
    }
}
