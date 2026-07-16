//! I/O-free coroutine fetching a Maildir entry by its ID.
//!
//! # Example
//!
//! ```rust,no_run
//! use io_maildir::{client::MaildirClient, entry::get::MaildirEntryGet};
//!
//! let client = MaildirClient::new("/path/to/root");
//! let maildir = client.load_maildir("inbox").unwrap();
//!
//! let coroutine = MaildirEntryGet::new(maildir, "1700000000.1.M0P1.host");
//! let entry = client.run(coroutine).unwrap();
//!
//! println!("{} bytes at {}", entry.contents().len(), entry.path());
//! ```

use core::{fmt, mem};

use alloc::{collections::BTreeSet, string::ToString};

use log::{debug, trace};
use thiserror::Error;

use crate::{
    coroutine::*, entry::MaildirFullEntry, entry::locate::*, maildir::Maildir, maildir_try,
    path::MaildirFsPath,
};

/// Failure causes during a [`MaildirEntryGet`] step.
#[derive(Clone, Debug, Error)]
pub enum MaildirEntryGetError {
    /// A reply arrived that does not match the awaited step.
    #[error("Maildir message get failed: unexpected arg {0:?}")]
    UnexpectedArg(Option<MaildirReply>),
    /// The inner locate step failed.
    #[error(transparent)]
    Locate(#[from] MaildirEntryLocateError),
}

/// Locates a Maildir entry by ID and reads its contents.
#[derive(Debug)]
pub struct MaildirEntryGet {
    state: State,
}

impl MaildirEntryGet {
    /// Builds a coroutine fetching the Maildir entry with the given ID.
    pub fn new(maildir: Maildir, id: impl ToString) -> Self {
        Self {
            state: State::Locate(MaildirEntryLocate::new(maildir, id)),
        }
    }
}

impl MaildirCoroutine for MaildirEntryGet {
    type Yield = MaildirYield;
    type Return = Result<MaildirFullEntry, MaildirEntryGetError>;

    fn resume(
        &mut self,
        arg: Option<MaildirReply>,
    ) -> MaildirCoroutineState<Self::Yield, Self::Return> {
        match (&mut self.state, arg) {
            (State::Locate(c), arg) => {
                let out = maildir_try!(c, arg);
                let paths = BTreeSet::from_iter([out.path.clone()]);
                self.state = State::AwaitRead { path: out.path };
                MaildirCoroutineState::Yielded(MaildirYield::WantsFileRead(paths))
            }
            (State::AwaitRead { path }, Some(MaildirReply::FileRead(map))) => {
                let path = mem::take(path);
                let contents = map.into_values().next().unwrap_or_default();
                debug!("got entry");
                trace!("path: {path}");
                MaildirCoroutineState::Complete(Ok(MaildirFullEntry::from((path, contents))))
            }
            (_, arg) => {
                let err = MaildirEntryGetError::UnexpectedArg(arg);
                MaildirCoroutineState::Complete(Err(err))
            }
        }
    }
}

#[derive(Debug)]
enum State {
    Locate(MaildirEntryLocate),
    AwaitRead { path: MaildirFsPath },
}

impl fmt::Display for State {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Locate(_) => f.write_str("locate message"),
            Self::AwaitRead { .. } => f.write_str("await read reply"),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::entry::get::*;

    fn maildir() -> Maildir {
        Maildir::from_path("root")
    }

    #[test]
    fn unexpected_reply_returns_error() {
        let mut cor = MaildirEntryGet::new(maildir(), "abc");
        expect_wants_file_exists(&mut cor);

        let err = expect_complete_err(&mut cor, Some(MaildirReply::DirCreate));
        assert!(matches!(err, MaildirEntryGetError::Locate(_)));
    }

    fn expect_wants_file_exists(cor: &mut MaildirEntryGet) {
        match cor.resume(None) {
            MaildirCoroutineState::Yielded(MaildirYield::WantsFileExists(_)) => {}
            state => panic!("expected WantsFileExists, got {state:?}"),
        }
    }

    fn expect_complete_err(
        cor: &mut MaildirEntryGet,
        arg: Option<MaildirReply>,
    ) -> MaildirEntryGetError {
        match cor.resume(arg) {
            MaildirCoroutineState::Complete(Err(err)) => err,
            state => panic!("expected Complete(Err), got {state:?}"),
        }
    }
}
