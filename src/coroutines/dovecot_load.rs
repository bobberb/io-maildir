//! I/O-free coroutine that reads the `dovecot-keywords` file sitting
//! at the root of a Maildir.
//!
//! Returns an empty table when the file is absent so callers can run
//! the resolution path unconditionally.

use alloc::{
    collections::{BTreeMap, BTreeSet},
    string::String,
    vec::Vec,
};

use log::trace;
use thiserror::Error;

use crate::{headers::parse_dovecot_keywords, maildir::Maildir, path::MaildirPath};

const FILENAME: &str = "dovecot-keywords";

/// Errors that can occur during coroutine progression.
#[derive(Clone, Debug, Error)]
pub enum DovecotLoadError {
    #[error("invalid dovecot load arg: {0:?}")]
    Invalid(Option<DovecotLoadArg>),
}

/// Result returned by [`DovecotLoad::resume`].
#[derive(Debug)]
pub enum DovecotLoadResult {
    Ok(BTreeMap<char, String>),
    WantsFileExists(BTreeSet<MaildirPath>),
    WantsFileRead(BTreeSet<MaildirPath>),
    Err(DovecotLoadError),
}

/// Argument fed back to [`DovecotLoad::resume`].
#[derive(Clone, Debug)]
pub enum DovecotLoadArg {
    FileExists(BTreeMap<MaildirPath, bool>),
    FileRead(BTreeMap<MaildirPath, Vec<u8>>),
}

#[derive(Debug, Default)]
enum State {
    #[default]
    Probe,
    Read,
    Done,
}

/// I/O-free coroutine loading a `dovecot-keywords` table for the
/// supplied Maildir.
#[derive(Debug)]
pub struct DovecotLoad {
    path: MaildirPath,
    state: State,
}

impl DovecotLoad {
    pub fn new(maildir: &Maildir) -> Self {
        Self {
            path: maildir.path().join(FILENAME),
            state: State::Probe,
        }
    }

    pub fn resume(&mut self, arg: Option<DovecotLoadArg>) -> DovecotLoadResult {
        match (&self.state, arg) {
            (State::Probe, None) => {
                let mut paths = BTreeSet::new();
                paths.insert(self.path.clone());
                trace!("wants dovecot-keywords probe at {}", self.path);
                DovecotLoadResult::WantsFileExists(paths)
            }
            (State::Probe, Some(DovecotLoadArg::FileExists(map))) => {
                let exists = map.get(&self.path).copied().unwrap_or(false);
                if !exists {
                    self.state = State::Done;
                    trace!("no dovecot-keywords at {}", self.path);
                    return DovecotLoadResult::Ok(BTreeMap::new());
                }

                let mut paths = BTreeSet::new();
                paths.insert(self.path.clone());
                self.state = State::Read;
                DovecotLoadResult::WantsFileRead(paths)
            }
            (State::Read, Some(DovecotLoadArg::FileRead(mut map))) => {
                let bytes = map.remove(&self.path).unwrap_or_default();
                let text = core::str::from_utf8(&bytes).unwrap_or("");
                let table = parse_dovecot_keywords(text);
                self.state = State::Done;
                trace!("loaded {} dovecot-keywords entries", table.len());
                DovecotLoadResult::Ok(table)
            }
            (_, arg) => DovecotLoadResult::Err(DovecotLoadError::Invalid(arg)),
        }
    }
}
