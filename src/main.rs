use anyhow::{Context, Result};
use rusqlite::Connection;
use ttum::app::App;

fn init_db(file: &str) -> Result<Connection> {
    let conn = Connection::open(file)?;
    conn.pragma_update(None, "foreign_keys", "ON")
        .context("enabling FKs")?;
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

fn main() -> Result<()> {
    let db = init_db("ttum.db").context("initializing database")?;
    let app = App::load(db)?;
    Ok(())
}
