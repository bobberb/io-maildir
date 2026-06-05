//! Example: store an entry in a Maildir synchronously.
//!
//! Run with:
//!
//! ```sh
//! cargo run --example std_store_entry
//! ```

use io_maildir::{
    client::MaildirClient, flag::types::MaildirFlags, maildir::types::MaildirSubdir, path::FsPath,
};
use tempfile::tempdir;

fn main() {
    env_logger::init();

    let tmp = tempdir().unwrap();
    let root = FsPath::new(tmp.path().to_string_lossy().into_owned());

    let client = MaildirClient::new(root);

    client.create_maildir("inbox").unwrap();
    let maildir = client.load_maildir("inbox").unwrap();

    let contents = b"From: alice@example.com\r\nTo: bob@example.com\r\nSubject: Hello\r\n\r\nHello, world!\r\n".to_vec();

    let (id, path) = client
        .store(
            maildir,
            MaildirSubdir::New,
            MaildirFlags::default(),
            contents,
        )
        .unwrap();

    println!("Stored entry:");
    println!("  ID:   {id}");
    println!("  Path: {path}");
}
