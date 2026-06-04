//! I/O-free coroutine writing the `dovecot-keywords` slot table at
//! the root of a Maildir.

use core::{fmt, mem};

use alloc::{collections::BTreeMap, string::String, vec::Vec};

use log::trace;
use thiserror::Error;

use crate::{
    coroutine::*, dovecot::types::serialize_dovecot_keywords, maildir::types::Maildir, path::FsPath,
};

const FILENAME: &str = "dovecot-keywords";

/// Failure causes during a [`DovecotStore`] step.
#[derive(Clone, Debug, Error)]
pub enum DovecotStoreError {
    #[error("Maildir dovecot store failed: unexpected arg {0:?}")]
    UnexpectedArg(Option<MaildirReply>),
}

/// Persists the `dovecot-keywords` slot table at the root of a Maildir.
#[derive(Debug)]
pub struct DovecotStore {
    state: State,
}

impl DovecotStore {
    pub fn new(maildir: &Maildir, table: &BTreeMap<char, String>) -> Self {
        let path = maildir.path().join(FILENAME);
        let payload = serialize_dovecot_keywords(table).into_bytes();
        Self {
            state: State::Start { path, payload },
        }
    }
}

impl MaildirCoroutine for DovecotStore {
    type Yield = MaildirYield;
    type Return = Result<(), DovecotStoreError>;

    fn resume(
        &mut self,
        arg: Option<MaildirReply>,
    ) -> MaildirCoroutineState<Self::Yield, Self::Return> {
        trace!("dovecot store: {}", self.state);

        match (&mut self.state, arg) {
            (State::Start { path, payload }, None) => {
                let path = mem::take(path);
                let payload = mem::take(payload);
                let files = BTreeMap::from_iter([(path, payload)]);
                self.state = State::AwaitWrite;
                MaildirCoroutineState::Yielded(MaildirYield::WantsFileCreate(files))
            }
            (State::AwaitWrite, Some(MaildirReply::FileCreate)) => {
                MaildirCoroutineState::Complete(Ok(()))
            }
            (_, arg) => {
                let err = DovecotStoreError::UnexpectedArg(arg);
                MaildirCoroutineState::Complete(Err(err))
            }
        }
    }
}

#[derive(Debug)]
enum State {
    Start { path: FsPath, payload: Vec<u8> },
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
    use super::*;

    fn maildir() -> Maildir {
        Maildir::from_path("root")
    }

    fn keywords_path() -> FsPath {
        FsPath::from("root/dovecot-keywords")
    }

    #[test]
    fn test() {
        let m = maildir();
        let table = BTreeMap::from_iter([('a', String::from("Project"))]);
        let mut cor = DovecotStore::new(&m, &table);

        let files = expect_wants_file_create(&mut cor);
        assert!(files.contains_key(&keywords_path()));

        expect_complete_ok(&mut cor, Some(MaildirReply::FileCreate));
    }

    #[test]
    fn unexpected_reply_returns_error() {
        let m = maildir();
        let mut cor = DovecotStore::new(&m, &BTreeMap::new());
        let _ = expect_wants_file_create(&mut cor);

        let err = expect_complete_err(&mut cor, Some(MaildirReply::DirCreate));
        assert!(matches!(err, DovecotStoreError::UnexpectedArg(_)));
    }

    // --- utils

    fn expect_wants_file_create(cor: &mut DovecotStore) -> BTreeMap<FsPath, Vec<u8>> {
        match cor.resume(None) {
            MaildirCoroutineState::Yielded(MaildirYield::WantsFileCreate(files)) => files,
            state => panic!("expected WantsFileCreate, got {state:?}"),
        }
    }

    fn expect_complete_ok(cor: &mut DovecotStore, arg: Option<MaildirReply>) {
        match cor.resume(arg) {
            MaildirCoroutineState::Complete(Ok(())) => {}
            state => panic!("expected Complete(Ok), got {state:?}"),
        }
    }

    fn expect_complete_err(cor: &mut DovecotStore, arg: Option<MaildirReply>) -> DovecotStoreError {
        match cor.resume(arg) {
            MaildirCoroutineState::Complete(Err(err)) => err,
            state => panic!("expected Complete(Err), got {state:?}"),
        }
    }
}
