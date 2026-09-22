use std::net::TcpStream;

use anyhow::Context;
use anyhow::Result;
use imap::Session;
use native_tls::TlsConnector;
use native_tls::TlsStream;
use rusqlite::Connection;
use rusqlite::types::ToSqlOutput;
use time::OffsetDateTime;
use time::format_description::well_known;

const INTERNALDATE_FORMAT: time::format_description::FormatDescriptionV3 = time::macros::format_description!(
    version = 3,
    "[day padding:space]-[month repr:short case_sensitive:false]-[year repr:full] [hour repr:24 padding:zero]:[minute padding:zero]:[second padding:zero] [offset_hour sign:mandatory][offset_minute]"
);

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

struct Address {
    email: Box<str>,
    name: Option<Box<str>>,
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

fn init_db(file: &str) -> Result<Connection> {
    let conn = Connection::open(file)?;
    conn.execute(
        "CREATE TABLE IF NOT EXISTS accounts(name TEXT, host TEXT, port INTEGER, login TEXT, password TEXT, PRIMARY KEY(name))",
        (),
    )?;
    conn.execute(
        "CREATE TABLE IF NOT EXISTS mailboxes(name TEXT, account TEXT, uid_validity INTEGER, PRIMARY KEY(name))",
        (),
    )?;
    conn.execute(
        "CREATE TABLE IF NOT EXISTS emails(
            mailbox TEXT, uid INTEGER, message_id TEXT,
            timestamp INTEGER, internal_timestamp INTEGER,
            subject TEXT, in_reply_to TEXT,
            seen INTEGER,
            body TEXT,
            PRIMARY KEY (mailbox, uid)
        )
        ",
        (),
    )?;
    conn.execute(
        "CREATE TABLE IF NOT EXISTS email_addresses(
            id INTEGER PRIMARY KEY,
            mailbox TEXT, uid INTEGER,
            type INTEGER,                 -- to, from, sender, cc, bcc
            name TEXT, email TEXT,
            FOREIGN KEY (mailbox, uid) REFERENCES emails(mailbox, uid) ON DELETE CASCADE
        )",
        (),
    )?;
    conn.execute(
        "CREATE TABLE IF NOT EXISTS email_attachements(
            mailbox TEXT, uid INTEGER, name TEXT,
            attachement_type TEXT,
            content TEXT,
            PRIMARY KEY (mailbox, uid, name),
            FOREIGN KEY (mailbox, uid) REFERENCES emails(mailbox, uid) ON DELETE CASCADE
        )
        ",
        (),
    )?;
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_email_message_id ON emails(mailbox, uid, message_id)",
        (),
    )?;
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_email_ts ON emails(timestamp)",
        (),
    )?;
    Ok(conn)
}

fn connect_to_account(
    account: &str,
    db: &Connection,
) -> Result<imap::Session<TlsStream<TcpStream>>> {
    let connector = TlsConnector::new().context("initializing TLS")?;
    let (host, port, login, password): (Box<str>, u16, Box<str>, Box<str>) = db.query_row(
        "SELECT host, port, login, password FROM accounts WHERE name=?1",
        [account],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
    )?;
    let client = imap::connect_starttls((host.as_ref(), port), host.as_ref(), &connector)
        .context("connecting to server")?;
    client
        .login(login, password)
        .map_err(|e| e.0)
        .context("authenticating")
}

fn index_email(mailbox: &str, f: &imap::types::Fetch, db: &mut Connection) -> Result<()> {
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
        in_reply_to: envelope.subject.map(parse_str),
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

fn reindex_mailbox(
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

    let mut range = (1, 50);
    loop {
        let fetch = session
            .uid_fetch(
                format!("{}:{}", range.0, range.1),
                "(FLAGS INTERNALDATE ENVELOPE)",
            )
            .context("fetching messages")?;
        for f in fetch.iter() {
            index_email(mailbox, f, db)?;
        }
        range = (range.1 + 1, range.1 + 51);
        if range.0 >= max_uid {
            break;
        }
        println!("{range:?}");
    }
    Ok(())
}

fn main() -> Result<()> {
    let mut db = init_db("ttum.db").context("initializing database")?;
    let mut session = connect_to_account("miga.me.uk", &db).context("connecting to account")?;

    reindex_mailbox("miga.me.uk", "Archive", &mut session, &mut db)?;

    Ok(())
}

mod test {
    #[test]
    fn test_parser() {
        let s = "=?utf-8?B?0J3QldCe0JPQoNCQ0J3QmNCn0JXQndCd0KvQmSDQmNCd0KLQldCg0J3QldCi?= =?utf-8?B?INCYINCR0JXQodCf0JvQkNCi0J3Qq9CZINCg0J7Qo9Ci0JXQoCE=?=";
        println!("{:?}", rfc2047_decoder::decode(s.as_bytes()));
    }
}
