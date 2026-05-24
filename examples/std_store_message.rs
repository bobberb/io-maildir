//! Example: store a message in a Maildir synchronously.
//!
//! Run with:
//!
//! ```sh
//! cargo run --example std_store_message
//! ```

use io_maildir::{
    client::MaildirClient, flag::MaildirFlags, maildir::MaildirSubdir, path::MaildirPath,
};
use tempfile::tempdir;

fn main() {
    let _ = env_logger::try_init();

    let tmp = tempdir().unwrap();
    let root = MaildirPath::new(tmp.path().join("inbox").to_string_lossy().into_owned());

    let client = MaildirClient::new(root.clone());

    client.create_maildir(root.clone()).unwrap();
    let maildir = client.load_maildir(root).unwrap();

    let contents = b"From: alice@example.com\r\nTo: bob@example.com\r\nSubject: Hello\r\n\r\nHello, world!\r\n".to_vec();

    let (id, path) = client
        .store(
            maildir,
            MaildirSubdir::New,
            MaildirFlags::default(),
            contents,
        )
        .unwrap();

    println!("Stored message:");
    println!("  ID:   {id}");
    println!("  Path: {path}");
}
