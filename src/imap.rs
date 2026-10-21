use crate::{
    mail::{Account, Address, Email, Mailbox, MailboxWithMetadata},
    model::Message,
};
use anyhow::{Context, Result, anyhow};
use io_imap::{
    client::{
        ImapClient, ImapClientStd, ImapClientStdConnectOptions, ImapMailboxWatchStreamOptions,
    },
    rfc3501::{
        fetch::ImapMessageFetchOptions, login::ImapLoginOptions, select::ImapMailboxSelectOptions,
    },
    types::{
        command::SelectParameter,
        core::VecN,
        extensions::enable::CapabilityEnable,
        fetch::{MessageDataItem, MessageDataItemName},
        flag::FlagFetch,
        mailbox::Mailbox as ImapMailbox,
    },
};
use mail_parser::MimeHeaders;
use std::{collections::BTreeMap, num::NonZero, sync::mpsc, thread, time::Duration};
use time::{Date, OffsetDateTime, Time, UtcOffset};
use tracing::{debug, error};

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

/// loop that periodically updates specified mailboxes using provided client.
pub(crate) fn mailboxes_update_loop(
    mut client: ImapClientStd,
    mut mailboxes: Vec<MailboxWithMetadata>,
    period: Duration,
    notifications: mpsc::Sender<Message>,
) {
    loop {
        for mailbox in mailboxes.iter_mut() {
            if let Err(e) = update_mailbox(&mut client, mailbox, &notifications) {
                error!("updating {mailbox}: {e:#}");
                continue;
            }
            let _ = notifications.send(Message::MailboxMetadata(mailbox.clone()));
        }
        thread::sleep(period);
    }
}

/// fetches new and vanished emails from the mailbox. Sends messages with email changes to provided channel, updates mailbox' metadata in place.
fn update_mailbox(
    client: &mut ImapClientStd,
    mailbox: &mut MailboxWithMetadata,
    notifications: &mpsc::Sender<Message>,
) -> Result<()> {
    debug!("getting updates for {mailbox}");

    let mbox = mailbox.inner.mailbox.clone().into_string();
    let mbox = ImapMailbox::try_from(mbox).context("getting mailbox")?;

    // select mailbox and do QRESYNC
    let mb_metadata = client
        .select(
            mbox,
            ImapMailboxSelectOptions {
                parameters: vec![SelectParameter::QResync {
                    uid_validity: NonZero::try_from(mailbox.uid_validity)?,
                    mod_sequence_value: NonZero::try_from(mailbox.highest_mod_seq)?,
                    known_uids: None,
                    seq_match_data: None,
                }],
            },
        )
        .context("selecting mailbox")?;

    // emails removed from mailbox since highest_mod_seq
    if !mb_metadata.vanished_earlier.is_empty() {
        notifications.send(Message::VanishedEmails {
            mailbox: mailbox.inner.clone(),
            uids: mb_metadata
                .vanished_earlier
                .iter()
                .inspect(|e| debug!("update_mailbox: email {e} vanished from {mailbox}"))
                .map(|v| v.get())
                .collect(),
        })?;
    }

    // emails added to mailbox since highest_mod_seq
    let mut uids = Vec::with_capacity(mb_metadata.changed.len());
    for fetch in &mb_metadata.changed {
        for item in fetch.items.as_ref() {
            if let MessageDataItem::Uid(uid) = item {
                debug!("update_mailbox: new email {uid} for {mailbox}");
                uids.push(uid.get())
            };
        }
    }
    for chunk in uids.chunks(10) {
        let emails = fetch_emails(client, chunk).context("fetching new emails")?;
        let _ = notifications.send(Message::NewEmails {
            mailbox: mailbox.inner.clone(),
            emails,
        });
    }

    mailbox.uid_validity = mb_metadata
        .uid_validity
        .context("getting UIDVALIDITY")?
        .into();
    mailbox.highest_mod_seq = mb_metadata
        .highest_mod_seq
        .context("getting HIGHEST_MOD_SEQ")?;

    Ok(())
}

/// starts IDLE and watch for changes in mailbox
pub fn watch_mailbox(
    account: &Account,
    mailbox: &Mailbox,
    notifications: mpsc::Sender<Message>,
) -> Result<()> {
    let mbox = if mailbox.mailbox.eq_ignore_ascii_case("inbox") {
        io_imap::types::mailbox::Mailbox::Inbox
    } else {
        io_imap::types::mailbox::Mailbox::try_from(mailbox.mailbox.clone().into_string())?
    };

    let watch = account
        .get_client()
        .context("initializing IDLE connection")?
        .watch_mailbox(
            mbox.clone(),
            &[],
            ImapMailboxWatchStreamOptions {
                idle_timeout: Some(Duration::from_secs(10)),
                ..Default::default()
            },
        )
        .context("starting IDLE")?;

    let mut client = account.get_client().context("getting client for watcher")?;
    let _ = client
        .select(mbox, ImapMailboxSelectOptions::default())
        .context("selecting mailbox")?;
    for event in watch {
        let event = event?;
        match event {
            io_imap::watch::ImapMailboxWatchEvent::EnvelopeAdded { uid, items: _ } => {
                debug!("watch_mailbox: new email {uid} for {mailbox}");
                let emails =
                    fetch_emails(&mut client, &[uid.get()]).context("fetching new email")?;
                notifications.send(Message::NewEmails {
                    mailbox: mailbox.clone(),
                    emails: emails,
                })?;
            }
            io_imap::watch::ImapMailboxWatchEvent::EnvelopeRemoved { uid } => {
                debug!("watch_mailbox: email {uid} vanished from {mailbox}");
                notifications.send(Message::VanishedEmails {
                    mailbox: mailbox.clone(),
                    uids: vec![uid.get()],
                })?;
            }
            _ => {}
        }
    }

    Ok(())
}

fn fetch_emails(client: &mut ImapClientStd, uids: &[u32]) -> Result<Vec<Email>> {
    let range: String = uids
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

    let mut emails = Vec::with_capacity(fetches.len());
    for (_, items) in fetches {
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
        let internal_timestamp = OffsetDateTime::from_unix_timestamp(idate.as_ref().timestamp())
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

    Ok(emails)
}

pub fn reindex_mailbox(
    account: &Account,
    mailbox: &Mailbox,
    notifications: mpsc::Sender<Message>,
) -> Result<()> {
    let mut client = account.get_client()?;
    client
        .enable(VecN::from([CapabilityEnable::try_from("QRESYNC").unwrap()]))
        .context("requesting QRESYNC cap")?;

    let mbox = mailbox.mailbox.clone().into_string();
    let mbox = ImapMailbox::try_from(mbox).context("getting mailbox")?;
    let mb_metadata = client
        .select(mbox, ImapMailboxSelectOptions::default())
        .context("selecting mailbox")?;
    notifications.send(Message::MailboxMetadata(MailboxWithMetadata {
        inner: mailbox.clone(),
        uid_validity: mb_metadata
            .uid_validity
            .context("getting UIDVALIDITY")?
            .into(),
        highest_mod_seq: mb_metadata
            .highest_mod_seq
            .context("getting HIGHEST_MOD_SEQ")?,
    }))?;

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

    // fetch email bodies, 10 emails per request
    let mut count = 0;
    let total = uids.len();
    for chunk in uids.chunks(10) {
        count += chunk.len();
        let emails = fetch_emails(&mut client, chunk)?;
        let _ = notifications.send(Message::Batch(vec![
            Message::ReindexStatus(crate::model::ReindexStatus {
                mailbox: mailbox.clone(),
                current: count,
                total: total,
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

pub(crate) fn get_account_mailboxes(account: &Account) -> Result<Vec<MailboxWithMetadata>> {
    let mut client = account.get_client()?;
    client
        .enable(VecN::from([CapabilityEnable::try_from("QRESYNC").unwrap()]))
        .context("requesting QRESYNC cap")?;

    let mailboxes = client.list("".try_into()?, "*".try_into()?)?;
    let mut result = Vec::new();
    for (mailbox, _, _) in mailboxes {
        let mbox_name: Box<str> = match &mailbox {
            ImapMailbox::Inbox => Box::from("Inbox"),
            ImapMailbox::Other(d) => Box::from(String::from_utf8_lossy(d.inner().as_ref())),
        };
        let mb_metadata = client
            .select(mailbox, ImapMailboxSelectOptions::default())
            .context("selecting mailbox")?;
        result.push(MailboxWithMetadata {
            inner: Mailbox {
                account: account.name.clone(),
                mailbox: mbox_name,
            },
            uid_validity: mb_metadata
                .uid_validity
                .context("expect uid_validity")?
                .into(),
            highest_mod_seq: mb_metadata
                .highest_mod_seq
                .context("expect highest_mod_seq")?,
        });
    }
    Ok(result)
}
