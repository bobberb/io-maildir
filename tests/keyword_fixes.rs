//! Regression tests for two keyword-handling defects:
//!
//! - BUG 1: `MaildirFlags::from(&MaildirPath)` dropped dovecot `a..z`
//!   slot letters, so flag operations (add/set/remove) rewrote the
//!   filename without them and silently stripped custom keywords.
//! - BUG 2: a keyword containing the active header separator (`,` for
//!   X-Keywords, ` ` for X-Label) corrupted the round-trip by splitting
//!   into several bogus keywords on read-back.

use std::path::Path;

use io_maildir::{
    client::MaildirClient,
    flag::{KeywordHeader, MaildirFlag, MaildirFlags},
    maildir::{Maildir, MaildirSubdir},
    path::MaildirPath,
};
use tempfile::tempdir;

fn eml(tag: &str) -> Vec<u8> {
    [
        "From: io-maildir test <test@example.org>",
        "To: io-maildir test <test@example.org>",
        &format!("Subject: keyword fix test {tag}"),
        "Date: Thu, 01 Jan 2026 00:00:00 +0000",
        "MIME-Version: 1.0",
        "Content-Type: text/plain; charset=utf-8",
        "",
        &format!("body {tag}"),
    ]
    .join("\r\n")
    .into_bytes()
}

/// Reads back the keywords a message carries via the dovecot-slot
/// mechanism, resolving the filename's slot letters through the
/// per-folder table.
fn dovecot_keywords(client: &MaildirClient, maildir: &Maildir, id: &str) -> Vec<String> {
    let (path, _, _) = client.locate(maildir.clone(), id).expect("locate message");
    let table = client
        .load_dovecot_keywords(maildir)
        .expect("load dovecot table");
    MaildirFlags::with_dovecot(&path, &table)
        .iter()
        .filter_map(|f| f.as_keyword().map(str::to_string))
        .collect()
}

fn setup(client: &MaildirClient, root: &MaildirPath) -> Maildir {
    client
        .create_maildir(root.join("inbox"))
        .expect("create inbox");
    client.load_maildir(root.join("inbox")).expect("load inbox")
}

/// BUG 1, CRITICAL repro: a keyworded message must keep its keyword
/// after a standard-flag change. Before the fix, `add_flags(\Seen)`
/// located the message, read its flags WITHOUT the dovecot slot letter,
/// and rewrote the filename without it — stripping `NonJunk`.
#[test]
fn add_flag_preserves_dovecot_keyword() {
    let _ = env_logger::try_init();
    let dir = tempdir().unwrap();
    let root = MaildirPath::new(dir.path().to_string_lossy().into_owned());

    let mut client = MaildirClient::new(root.clone());
    client.dovecot_keywords = true;
    let inbox = setup(&client, &root);

    let (id, _) = client
        .store(
            inbox.clone(),
            MaildirSubdir::Cur,
            MaildirFlags::from_iter([MaildirFlag::keyword("NonJunk")]),
            eml("critical"),
        )
        .expect("store keyworded message");

    // Slot letter is present in the filename right after store.
    assert_eq!(
        dovecot_keywords(&client, &inbox, &id),
        vec!["NonJunk".to_string()],
        "keyword should be encoded via a dovecot slot after store",
    );

    // The regression: adding a standard flag must NOT drop the slot.
    client
        .add_flags(
            inbox.clone(),
            &id,
            MaildirFlags::from_iter([MaildirFlag::Seen]),
        )
        .expect("add Seen");

    let (path, _, _) = client.locate(inbox.clone(), &id).expect("locate after add");
    let named = MaildirFlags::from(&path);
    assert!(
        named.contains(&MaildirFlag::Seen),
        "Seen must be present after add_flags",
    );
    assert_eq!(
        dovecot_keywords(&client, &inbox, &id),
        vec!["NonJunk".to_string()],
        "NonJunk MUST survive add_flags(\\Seen)",
    );
}

/// BUG 1, multi-keyword removal: removing one keyword from a message
/// with two must leave the OTHER keyword's slot letter intact.
#[test]
fn remove_one_keyword_leaves_the_other() {
    let _ = env_logger::try_init();
    let dir = tempdir().unwrap();
    let root = MaildirPath::new(dir.path().to_string_lossy().into_owned());

    let mut client = MaildirClient::new(root.clone());
    client.dovecot_keywords = true;
    let inbox = setup(&client, &root);

    let (id, _) = client
        .store(
            inbox.clone(),
            MaildirSubdir::Cur,
            MaildirFlags::from_iter([
                MaildirFlag::keyword("NonJunk"),
                MaildirFlag::keyword("Work"),
            ]),
            eml("multi"),
        )
        .expect("store two-keyword message");

    let mut before = dovecot_keywords(&client, &inbox, &id);
    before.sort();
    assert_eq!(before, vec!["NonJunk".to_string(), "Work".to_string()]);

    client
        .remove_flags(
            inbox.clone(),
            &id,
            MaildirFlags::from_iter([MaildirFlag::keyword("Work")]),
        )
        .expect("remove Work keyword");

    assert_eq!(
        dovecot_keywords(&client, &inbox, &id),
        vec!["NonJunk".to_string()],
        "removing Work must leave NonJunk's slot letter intact",
    );
}

/// A normal keyword still round-trips through the dovecot mechanism
/// after the fix (no regression on the happy path).
#[test]
fn normal_keyword_round_trips_via_dovecot() {
    let _ = env_logger::try_init();
    let dir = tempdir().unwrap();
    let root = MaildirPath::new(dir.path().to_string_lossy().into_owned());

    let mut client = MaildirClient::new(root.clone());
    client.dovecot_keywords = true;
    let inbox = setup(&client, &root);

    let (id, _) = client
        .store(
            inbox.clone(),
            MaildirSubdir::Cur,
            MaildirFlags::from_iter([MaildirFlag::keyword("Important")]),
            eml("normal"),
        )
        .expect("store");

    assert_eq!(
        dovecot_keywords(&client, &inbox, &id),
        vec!["Important".to_string()],
    );
}

/// A normal keyword still round-trips through the header mechanism.
#[test]
fn normal_keyword_round_trips_via_header() {
    let _ = env_logger::try_init();
    let dir = tempdir().unwrap();
    let root = MaildirPath::new(dir.path().to_string_lossy().into_owned());

    let mut client = MaildirClient::new(root.clone());
    client.keywords_header = Some(KeywordHeader::XKeywords);
    let inbox = setup(&client, &root);

    let (id, _) = client
        .store(
            inbox.clone(),
            MaildirSubdir::Cur,
            MaildirFlags::from_iter([MaildirFlag::keyword("Important")]),
            eml("hdr"),
        )
        .expect("store");

    let (path, _, _) = client.locate(inbox.clone(), &id).expect("locate");
    let entry = io_maildir::entry::MaildirEntry::from_path(path);
    let msg = client.read_entry(&entry).expect("read");
    let kws = io_maildir::headers::extract_keywords_header(msg.contents(), KeywordHeader::XKeywords);
    assert_eq!(kws, vec!["Important".to_string()]);
}

/// BUG 2: a keyword containing the active separator must be dropped,
/// not corrupted into split fragments. With X-Keywords the separator is
/// `,`, so `Foo,Bar` would split into `["Bar", "Foo"]` on read-back.
#[test]
fn keyword_with_separator_is_dropped_not_corrupted() {
    let _ = env_logger::try_init();
    let dir = tempdir().unwrap();
    let root = MaildirPath::new(dir.path().to_string_lossy().into_owned());

    let mut client = MaildirClient::new(root.clone());
    client.keywords_header = Some(KeywordHeader::XKeywords);
    let inbox = setup(&client, &root);

    let (id, _) = client
        .store(
            inbox.clone(),
            MaildirSubdir::Cur,
            MaildirFlags::from_iter([MaildirFlag::keyword("Foo,Bar")]),
            eml("sep"),
        )
        .expect("store");

    let (path, _, _) = client.locate(inbox.clone(), &id).expect("locate");
    let entry = io_maildir::entry::MaildirEntry::from_path(path);
    let msg = client.read_entry(&entry).expect("read");
    let kws = io_maildir::headers::extract_keywords_header(msg.contents(), KeywordHeader::XKeywords);

    assert!(
        !kws.iter().any(|k| k == "Foo"),
        "no corrupted `Foo` fragment should appear",
    );
    assert!(
        !kws.iter().any(|k| k == "Bar"),
        "no corrupted `Bar` fragment should appear",
    );
    assert!(
        kws.is_empty(),
        "the corruptible keyword should be dropped entirely, got {kws:?}",
    );

    // Sanity: the message still exists on disk.
    let (final_path, _, _) = client.locate(inbox.clone(), &id).expect("still located");
    assert!(Path::new(final_path.as_str()).is_file());
}
