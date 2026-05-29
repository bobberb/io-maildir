//! I/O-free coroutine that reads the `dovecot-keywords` file sitting
//! at the root of a Maildir.
//!
//! Returns an empty table when the file is absent so callers can run
//! the resolution path unconditionally.

use alloc::{
    collections::{BTreeMap, BTreeSet},
    string::String,
};

use log::trace;
use thiserror::Error;

use crate::{coroutine::*, headers::parse_dovecot_keywords, maildir::Maildir, path::MaildirPath};

const FILENAME: &str = "dovecot-keywords";

/// Errors that can occur during coroutine progression.
#[derive(Clone, Debug, Error)]
pub enum DovecotLoadError {
    #[error("invalid dovecot load reply: {0:?}")]
    Invalid(Option<MaildirReply>),
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
}

impl MaildirCoroutine for DovecotLoad {
    type Yield = MaildirYield;
    type Return = Result<BTreeMap<char, String>, DovecotLoadError>;

    fn resume(
        &mut self,
        arg: Option<MaildirReply>,
    ) -> MaildirCoroutineState<Self::Yield, Self::Return> {
        match (&self.state, arg) {
            (State::Probe, None) => {
                let mut paths = BTreeSet::new();
                paths.insert(self.path.clone());
                trace!("wants dovecot-keywords probe at {}", self.path);
                MaildirCoroutineState::Yielded(MaildirYield::WantsFileExists(paths))
            }
            (State::Probe, Some(MaildirReply::FileExists(map))) => {
                let exists = map.get(&self.path).copied().unwrap_or(false);
                if !exists {
                    self.state = State::Done;
                    trace!("no dovecot-keywords at {}", self.path);
                    return MaildirCoroutineState::Complete(Ok(BTreeMap::new()));
                }

                let mut paths = BTreeSet::new();
                paths.insert(self.path.clone());
                self.state = State::Read;
                MaildirCoroutineState::Yielded(MaildirYield::WantsFileRead(paths))
            }
            (State::Read, Some(MaildirReply::FileRead(mut map))) => {
                let bytes = map.remove(&self.path).unwrap_or_default();
                let text = core::str::from_utf8(&bytes).unwrap_or("");
                let table = parse_dovecot_keywords(text);
                self.state = State::Done;
                trace!("loaded {} dovecot-keywords entries", table.len());
                MaildirCoroutineState::Complete(Ok(table))
            }
            (_, arg) => MaildirCoroutineState::Complete(Err(DovecotLoadError::Invalid(arg))),
        }
    }
}
