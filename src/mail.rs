use std::collections::HashMap;
use std::fmt::Display;
use std::net::TcpStream;

use crate::imap::get_account_mailboxes;
use crate::model::Message;
use anyhow::Context;
use anyhow::Result;
use io_imap::client::ImapClient;
use io_imap::client::ImapClientStd;
use io_imap::client::ImapClientStdConnectOptions;
use io_imap::rfc3501::login::ImapLoginOptions;
use io_imap::types::core::VecN;
use io_imap::types::extensions::enable::CapabilityEnable;
use native_tls::TlsConnector;
use native_tls::TlsStream;
use rusqlite::Connection;
use rusqlite::types::ToSqlOutput;
use time::OffsetDateTime;

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
    pub(crate) fn multiple(addr: &mail_parser::Address) -> Vec<Self> {
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

#[derive(Debug, Default, Clone)]
pub(crate) struct Mailbox {
    pub(crate) account: Box<str>,
    pub(crate) mailbox: Box<str>,
}

impl Display for Mailbox {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}/{}", self.account, self.mailbox)
    }
}

#[derive(Debug, Clone)]
pub(crate) struct MailboxWithMetadata {
    pub(crate) inner: Mailbox,
    pub(crate) uid_validity: u32,
    pub(crate) highest_mod_seq: u64,
}

impl Display for MailboxWithMetadata {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.inner.fmt(f)
    }
}

impl MailboxWithMetadata {
    pub(crate) fn load_all(update: bool, db: &mut Connection) -> Result<Vec<Self>> {
        let mut mailboxes = Vec::with_capacity(3);
        if !update {
            let mut stmt = db.prepare(
            "SELECT account, name, uid_validity, highest_mod_seq FROM mailboxes WHERE hidden = 0 ORDER BY account, lower(name) ASC",
        )?;
            let mut rows = stmt.query([])?;
            while let Some(row) = rows.next()? {
                mailboxes.push(MailboxWithMetadata {
                    inner: Mailbox {
                        account: row.get(0)?,
                        mailbox: row.get(1)?,
                    },
                    uid_validity: row.get(2)?,
                    highest_mod_seq: row.get::<_, i64>(3)? as u64,
                });
            }
            return Ok(mailboxes);
        }

        let accounts = Account::load_all(db).context("loading accounts")?;
        let tx = db.transaction().context("starting transaction")?;
        {
            let mut stmt = tx.prepare(
                "
        INSERT into mailboxes (account, name, uid_validity, highest_mod_seq )
        VALUES (?1, ?2, ?3, ?4)
        ON CONFLICT (account, name) DO UPDATE SET
            uid_validity=excluded.uid_validity,
            highest_mod_seq=excluded.highest_mod_seq
        ",
            )?;
            for (acc_name, account) in &accounts {
                let mboxes = get_account_mailboxes(account).context("getting account mailboxes")?;
                for m in mboxes {
                    stmt.execute((
                        acc_name,
                        m.inner.mailbox.clone(),
                        m.uid_validity,
                        m.highest_mod_seq as i64,
                    ))?;
                    mailboxes.push(m);
                }
            }
        }
        tx.commit().context("committing transaction")?;
        Ok(mailboxes)
    }

    pub(crate) fn save_to_db(&self, db: &Connection) -> Result<()> {
        db.execute(
            "
            INSERT INTO mailboxes(account, name, uid_validity, highest_mod_seq)
            VALUES (?1, ?2, ?3, ?4)
            ON CONFLICT (account, name) DO UPDATE SET
                uid_validity=excluded.uid_validity,
                highest_mod_seq=excluded.highest_mod_seq
            ",
            (
                &self.inner.account,
                &self.inner.mailbox,
                self.uid_validity,
                self.highest_mod_seq as i64,
            ),
        )
        .context("upserting mailbox metadata")?;
        Ok(())
    }
}

pub(crate) struct Account {
    pub(crate) name: Box<str>,
    pub(crate) url: url::Url,
    pub(crate) user: Box<str>,
    pub(crate) password: Box<str>,
    pub(crate) starttls: bool,
}

impl Account {
    pub(crate) fn load(name: &str, db: &Connection) -> Result<Self> {
        let (host, port, user, password, starttls): (Box<str>, u16, Box<str>, Box<str>, bool) = db
            .query_one(
                "SELECT host, port, login, password, starttls FROM accounts WHERE name=?1",
                [name],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
            )?;

        Ok(Self {
            name: Box::from(name),
            url: url::Url::parse(&format!(
                "{}://{host}:{port}",
                if starttls { "imap" } else { "imaps" }
            ))?,
            user,
            password,
            starttls,
        })
    }

    pub(crate) fn get_mailboxes(&self, db: &Connection) -> Result<Vec<MailboxWithMetadata>> {
        let mut mailboxes = Vec::with_capacity(3);
        let mut stmt = db.prepare(
            "SELECT account, name, uid_validity, highest_mod_seq FROM mailboxes WHERE account = ?1 AND hidden = 0 ORDER BY lower(name) ASC",
        )?;
        let mut rows = stmt.query([&self.name])?;
        while let Some(row) = rows.next()? {
            mailboxes.push(MailboxWithMetadata {
                inner: Mailbox {
                    account: row.get(0)?,
                    mailbox: row.get(1)?,
                },
                uid_validity: row.get(2)?,
                highest_mod_seq: row.get::<_, i64>(3)? as u64,
            });
        }
        return Ok(mailboxes);
    }

    pub(crate) fn get_client(&self) -> Result<ImapClientStd> {
        let mut opts = ImapClientStdConnectOptions::default();
        opts.session.starttls = self.starttls;
        let (mut client, _) =
            ImapClientStd::connect(&self.url, opts).context("connecting to IMAP server")?;

        client
            .login(&self.user, &self.password, ImapLoginOptions::default())
            .context("authenticating with IMAP server")?;
        client
            .enable(VecN::from([CapabilityEnable::try_from("QRESYNC").unwrap()]))
            .context("requesting QRESYNC cap")?;
        Ok(client)
    }

    pub(crate) fn load_all(db: &Connection) -> Result<HashMap<Box<str>, Self>> {
        let mut stmt =
            db.prepare("SELECT name, host, port, login, password, starttls FROM accounts")?;
        let mut rows = stmt.query([])?;
        let mut result = HashMap::new();
        while let Some(r) = rows.next()? {
            let (name, host, port, user, password, starttls) = (
                r.get::<_, Box<str>>(0)?,
                r.get::<_, Box<str>>(1)?,
                r.get::<_, u16>(2)?,
                r.get(3)?,
                r.get(4)?,
                r.get(5)?,
            );
            result.insert(
                name.clone(),
                Self {
                    name: name,
                    url: url::Url::parse(&format!(
                        "{}://{host}:{port}",
                        if starttls { "imap" } else { "imaps" }
                    ))?,
                    user,
                    password,
                    starttls,
                },
            );
        }

        Ok(result)
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
    pub(crate) fn load_from_db(mailbox: &Mailbox, uid: u32, db: &Connection) -> Result<Self> {
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

    pub(crate) fn save_to_db(&self, mailbox: &Mailbox, db: &mut Connection) -> Result<()> {
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

    pub(crate) fn drop_uids(mailbox: &Mailbox, uids: &[u32], db: &mut Connection) -> Result<()> {
        let tx = db.transaction().context("starting transaction")?;
        {
            let mut stmt =
                tx.prepare("DELETE FROM emails WHERE account = ?1 AND mailbox = ?2 AND uid = ?3")?;
            for uid in uids {
                stmt.execute((&mailbox.account, &mailbox.mailbox, uid))?;
            }
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

mod test {
    #[test]
    fn test_parser() {
        let s = "=?UTF-8?B?TWFudGFzIE1pa8WheXM=?=";
        println!("{:?}", rfc2047_decoder::decode(s.as_bytes()));
    }
}
