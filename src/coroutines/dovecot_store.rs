//! I/O-free coroutine that writes a `dovecot-keywords` table at the
//! root of a Maildir.

use alloc::{collections::BTreeMap, string::String, vec::Vec};

use log::trace;
use thiserror::Error;

use crate::{headers::serialize_dovecot_keywords, maildir::Maildir, path::MaildirPath};

const FILENAME: &str = "dovecot-keywords";

#[derive(Clone, Debug, Error)]
pub enum DovecotStoreError {
    #[error("invalid dovecot store arg: {0:?}")]
    Invalid(Option<DovecotStoreArg>),
}

#[derive(Debug)]
pub enum DovecotStoreResult {
    Ok,
    WantsFileCreate(BTreeMap<MaildirPath, Vec<u8>>),
    Err(DovecotStoreError),
}

#[derive(Clone, Debug)]
pub enum DovecotStoreArg {
    FileCreate,
}

#[derive(Debug, Default)]
enum State {
    #[default]
    Pending,
    Awaiting,
    Done,
}

/// I/O-free coroutine writing a `dovecot-keywords` file at the root
/// of a Maildir.
#[derive(Debug)]
pub struct DovecotStore {
    path: MaildirPath,
    payload: Option<Vec<u8>>,
    state: State,
}

impl DovecotStore {
    pub fn new(maildir: &Maildir, table: &BTreeMap<char, String>) -> Self {
        let payload = serialize_dovecot_keywords(table).into_bytes();
        Self {
            path: maildir.path().join(FILENAME),
            payload: Some(payload),
            state: State::Pending,
        }
    }

    pub fn resume(&mut self, arg: Option<DovecotStoreArg>) -> DovecotStoreResult {
        match (&self.state, arg, self.payload.take()) {
            (State::Pending, None, Some(payload)) => {
                trace!(
                    "wants dovecot-keywords write at {} ({} bytes)",
                    self.path,
                    payload.len()
                );
                let mut map = BTreeMap::new();
                map.insert(self.path.clone(), payload);
                self.state = State::Awaiting;
                DovecotStoreResult::WantsFileCreate(map)
            }
            (State::Awaiting, Some(DovecotStoreArg::FileCreate), _) => {
                self.state = State::Done;
                DovecotStoreResult::Ok
            }
            (_, arg, _) => DovecotStoreResult::Err(DovecotStoreError::Invalid(arg)),
        }
    }
}
