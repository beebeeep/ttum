use crate::{
    mail::{Account, Address, Email, Mailbox},
    model::Message,
};
use anyhow::{Context, Result, anyhow};
use io_imap::{
    client::{ImapClient, ImapClientStd, ImapClientStdConnectOptions},
    rfc3501::{
        fetch::ImapMessageFetchOptions, login::ImapLoginOptions, select::ImapMailboxSelectOptions,
    },
    types::{
        core::VecN,
        extensions::enable::CapabilityEnable,
        fetch::{MessageDataItem, MessageDataItemName},
        flag::FlagFetch,
        mailbox::Mailbox as ImapMailbox,
    },
};
use mail_parser::MimeHeaders;
use std::sync::mpsc;
use time::{Date, OffsetDateTime, Time, UtcOffset};

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

pub fn reindex_mailbox(
    account: &Account,
    mailbox: &Mailbox,
    notifications: mpsc::Sender<Message>,
) -> Result<()> {
    let mut opts = ImapClientStdConnectOptions::default();
    opts.session.starttls = account.starttls;
    let (mut client, _) =
        ImapClientStd::connect(&account.url, opts).context("connecting to IMAP server")?;

    client
        .login(
            &account.user,
            &account.password,
            ImapLoginOptions::default(),
        )
        .context("authenticating with IMAP server")?;

    client
        .enable(VecN::from([CapabilityEnable::try_from("QRESYNC").unwrap()]))
        .context("requesting QRESYNC cap")?;
    let mbox = mailbox.mailbox.clone().into_string();
    let mbox = ImapMailbox::try_from(mbox).context("getting mailbox")?;
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

pub(crate) fn get_account_mailboxes(account: &Account) -> Result<Vec<Mailbox>> {
    todo!()
}
