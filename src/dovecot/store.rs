//! I/O-free coroutine writing the `dovecot-keywords` slot table at
//! the root of a Maildir.
//!
//! # Example
//!
//! ```rust,no_run
//! use std::collections::BTreeMap;
//!
//! use io_maildir::{client::MaildirClient, dovecot::store::MaildirDovecotStore};
//!
//! let client = MaildirClient::new("/path/to/root");
//! let maildir = client.load_maildir("inbox").unwrap();
//!
//! let mut table = BTreeMap::new();
//! table.insert('a', "Important".to_string());
//! table.insert('b', "Personal".to_string());
//!
//! let coroutine = MaildirDovecotStore::new(&maildir, &table);
//! client.run(coroutine).unwrap();
//! ```

use core::{fmt, mem};

use alloc::{collections::BTreeMap, string::String, vec::Vec};

use log::debug;
use thiserror::Error;

use crate::{
    coroutine::*, dovecot::utils::serialize_dovecot_keywords, maildir::Maildir, path::MaildirFsPath,
};

const FILENAME: &str = "dovecot-keywords";

/// Failure causes during a [`MaildirDovecotStore`] step.
#[derive(Clone, Debug, Error)]
pub enum MaildirDovecotStoreError {
    /// A reply arrived that does not match the awaited step.
    #[error("Maildir dovecot store failed: unexpected arg {0:?}")]
    UnexpectedArg(Option<MaildirReply>),
}

/// Persists the `dovecot-keywords` slot table at the root of a Maildir.
#[derive(Debug)]
pub struct MaildirDovecotStore {
    state: State,
}

impl MaildirDovecotStore {
    /// Builds a coroutine persisting the `dovecot-keywords` slot table
    /// at the root of a Maildir.
    pub fn new(maildir: &Maildir, table: &BTreeMap<char, String>) -> Self {
        let path = maildir.path().join(FILENAME);
        let payload = serialize_dovecot_keywords(table).into_bytes();
        Self {
            state: State::Start { path, payload },
        }
    }
}

impl MaildirCoroutine for MaildirDovecotStore {
    type Yield = MaildirYield;
    type Return = Result<(), MaildirDovecotStoreError>;

    fn resume(
        &mut self,
        arg: Option<MaildirReply>,
    ) -> MaildirCoroutineState<Self::Yield, Self::Return> {
        match (&mut self.state, arg) {
            (State::Start { path, payload }, None) => {
                let path = mem::take(path);
                let payload = mem::take(payload);
                let files = BTreeMap::from_iter([(path, payload)]);
                self.state = State::AwaitWrite;
                MaildirCoroutineState::Yielded(MaildirYield::WantsFileCreate(files))
            }
            (State::AwaitWrite, Some(MaildirReply::FileCreate)) => {
                debug!("stored dovecot keywords");
                MaildirCoroutineState::Complete(Ok(()))
            }
            (_, arg) => {
                let err = MaildirDovecotStoreError::UnexpectedArg(arg);
                MaildirCoroutineState::Complete(Err(err))
            }
        }
    }
}

#[derive(Debug)]
enum State {
    Start {
        path: MaildirFsPath,
        payload: Vec<u8>,
    },
    AwaitWrite,
}

impl fmt::Display for State {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Start { .. } => f.write_str("start"),
            Self::AwaitWrite => f.write_str("await write reply"),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::dovecot::store::*;

    fn maildir() -> Maildir {
        Maildir::from_path("root")
    }

    fn keywords_path() -> MaildirFsPath {
        MaildirFsPath::from("root/dovecot-keywords")
    }

    #[test]
    fn test() {
        let m = maildir();
        let table = BTreeMap::from_iter([('a', String::from("Project"))]);
        let mut cor = MaildirDovecotStore::new(&m, &table);

        let files = expect_wants_file_create(&mut cor);
        assert!(files.contains_key(&keywords_path()));

        expect_complete_ok(&mut cor, Some(MaildirReply::FileCreate));
    }

    #[test]
    fn unexpected_reply_returns_error() {
        let m = maildir();
        let mut cor = MaildirDovecotStore::new(&m, &BTreeMap::new());
        let _ = expect_wants_file_create(&mut cor);

        let err = expect_complete_err(&mut cor, Some(MaildirReply::DirCreate));
        assert!(matches!(err, MaildirDovecotStoreError::UnexpectedArg(_)));
    }

    fn expect_wants_file_create(cor: &mut MaildirDovecotStore) -> BTreeMap<MaildirFsPath, Vec<u8>> {
        match cor.resume(None) {
            MaildirCoroutineState::Yielded(MaildirYield::WantsFileCreate(files)) => files,
            state => panic!("expected WantsFileCreate, got {state:?}"),
        }
    }

    fn expect_complete_ok(cor: &mut MaildirDovecotStore, arg: Option<MaildirReply>) {
        match cor.resume(arg) {
            MaildirCoroutineState::Complete(Ok(())) => {}
            state => panic!("expected Complete(Ok), got {state:?}"),
        }
    }

    fn expect_complete_err(
        cor: &mut MaildirDovecotStore,
        arg: Option<MaildirReply>,
    ) -> MaildirDovecotStoreError {
        match cor.resume(arg) {
            MaildirCoroutineState::Complete(Err(err)) => err,
            state => panic!("expected Complete(Err), got {state:?}"),
        }
    }
}
