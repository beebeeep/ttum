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
        "CREATE TABLE IF NOT EXISTS mailboxes(name TEXT, account TEXT, uid_validity INTEGER, PRIMARY KEY(account, name))",
        (),
    )?;
    conn.execute(
        "CREATE TABLE IF NOT EXISTS emails(
            account TEXT, mailbox TEXT, uid INTEGER, message_id TEXT,
            timestamp INTEGER, internal_timestamp INTEGER,
            subject TEXT, in_reply_to TEXT,
            seen INTEGER,
            body TEXT,
            PRIMARY KEY (account, mailbox, uid)
        )
        ",
        (),
    )?;
    conn.execute(
        "CREATE TABLE IF NOT EXISTS email_addresses(
            id INTEGER PRIMARY KEY,
            account TEXT, mailbox TEXT, uid INTEGER,
            type INTEGER,                 -- from, to, sender, cc, bcc
            name TEXT, email TEXT,
            FOREIGN KEY (account, mailbox, uid) REFERENCES emails(account, mailbox, uid) ON DELETE CASCADE
        )",
        (),
    )?;
    conn.execute(
        "CREATE TABLE IF NOT EXISTS email_attachements(
            id INTEGER PRIMARY KEY,
            account TEXT, mailbox TEXT, uid INTEGER, name TEXT,
            attachement_type TEXT,
            content BLOB,
            FOREIGN KEY (account, mailbox, uid) REFERENCES emails(account, mailbox, uid) ON DELETE CASCADE
        )
        ",
        (),
    )?;
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_email_message_id ON emails(account, mailbox, uid, message_id)",
        (),
    )?;
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_email_ts ON emails(account, mailbox, timestamp)",
        (),
    )?;
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_email_attachement ON email_attachements(account, mailbox, uid, name)",
        (),
    )?;
    Ok(conn)
}

fn main() -> Result<()> {
    let db = init_db("ttum.db").context("initializing database")?;
    let matches = clap::Command::new("ttum mail client")
        .arg(
            clap::Arg::new("update-mailboxes")
                .short('m')
                .action(clap::ArgAction::SetTrue),
        )
        .get_matches();
    let app = App::load(db, matches.get_flag("update-mailboxes"))?;
    let r = app.run();
    ratatui::restore();
    if let Err(e) = r {
        eprintln!("got error: {e:#}");
    }
    Ok(())
}
