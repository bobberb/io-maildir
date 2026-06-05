//! I/O-free coroutine copying a Maildir entry to another Maildir.
//!
//! # Example
//!
//! ```rust,no_run
//! use io_maildir::{client::MaildirClient, entry::copy::MaildirEntryCopy};
//!
//! let client = MaildirClient::new("/path/to/root");
//! let source = client.load_maildir("inbox").unwrap();
//! let target = client.load_maildir("archive").unwrap();
//!
//! let coroutine = MaildirEntryCopy::new("1700000000.1.M0P1.host", source, target, None);
//! client.run(coroutine).unwrap();
//! ```

use core::fmt;

use alloc::string::{String, ToString};

use log::trace;
use thiserror::Error;

use crate::{
    coroutine::*,
    entry::locate::*,
    entry::types::INFORMATIONAL_SUFFIX_SEPARATOR,
    maildir::types::{Maildir, MaildirSubdir},
    maildir_try,
    path::FsPath,
};

/// Failure causes during a [`MaildirEntryCopy`] step.
#[derive(Clone, Debug, Error)]
pub enum MaildirEntryCopyError {
    #[error("Maildir message copy failed: unexpected arg {0:?}")]
    UnexpectedArg(Option<MaildirReply>),

    #[error(transparent)]
    Locate(#[from] MaildirEntryLocateError),
}

/// Copies a Maildir entry into `target`; `None` target_subdir keeps
/// the source subdir.
#[derive(Debug)]
pub struct MaildirEntryCopy {
    id: String,
    target: Maildir,
    target_subdir: Option<MaildirSubdir>,
    state: State,
}

impl MaildirEntryCopy {
    pub fn new(
        id: impl ToString,
        source: Maildir,
        target: Maildir,
        target_subdir: Option<MaildirSubdir>,
    ) -> Self {
        let id = id.to_string();
        Self {
            state: State::Locate(MaildirEntryLocate::new(source, &id)),
            id,
            target,
            target_subdir,
        }
    }
}

impl MaildirCoroutine for MaildirEntryCopy {
    type Yield = MaildirYield;
    type Return = Result<(), MaildirEntryCopyError>;

    fn resume(
        &mut self,
        arg: Option<MaildirReply>,
    ) -> MaildirCoroutineState<Self::Yield, Self::Return> {
        trace!("entry copy: {}", self.state);

        match (&mut self.state, arg) {
            (State::Locate(c), arg) => {
                let out = maildir_try!(c, arg);

                let target_subdir = self.target_subdir.clone().unwrap_or(out.subdir);
                let target = build_target_path(&self.target, &target_subdir, &self.id);
                let pairs = vec![(out.path, target)];
                self.state = State::AwaitCopy;
                MaildirCoroutineState::Yielded(MaildirYield::WantsCopy(pairs))
            }
            (State::AwaitCopy, Some(MaildirReply::Copy)) => MaildirCoroutineState::Complete(Ok(())),
            (_, arg) => {
                let err = MaildirEntryCopyError::UnexpectedArg(arg);
                MaildirCoroutineState::Complete(Err(err))
            }
        }
    }
}

#[derive(Debug)]
enum State {
    Locate(MaildirEntryLocate),
    AwaitCopy,
}

impl fmt::Display for State {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Locate(_) => f.write_str("locate source"),
            Self::AwaitCopy => f.write_str("await copy reply"),
        }
    }
}

fn build_target_path(target: &Maildir, subdir: &MaildirSubdir, id: &str) -> FsPath {
    match subdir {
        MaildirSubdir::Cur => {
            let name = format!("{id}{INFORMATIONAL_SUFFIX_SEPARATOR}2,");
            target.cur().join(&name)
        }
        MaildirSubdir::New => target.new().join(id),
        MaildirSubdir::Tmp => target.tmp().join(id),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn source() -> Maildir {
        Maildir::from_path("root/src")
    }

    fn target() -> Maildir {
        Maildir::from_path("root/dst")
    }

    #[test]
    fn unexpected_reply_returns_error() {
        let mut cor = MaildirEntryCopy::new("abc", source(), target(), None);
        let _ = expect_wants_file_exists(&mut cor);

        let err = expect_complete_err(&mut cor, Some(MaildirReply::DirCreate));
        assert!(matches!(err, MaildirEntryCopyError::Locate(_)));
    }

    // --- utils

    fn expect_wants_file_exists(cor: &mut MaildirEntryCopy) {
        match cor.resume(None) {
            MaildirCoroutineState::Yielded(MaildirYield::WantsFileExists(_)) => {}
            state => panic!("expected WantsFileExists, got {state:?}"),
        }
    }

    fn expect_complete_err(
        cor: &mut MaildirEntryCopy,
        arg: Option<MaildirReply>,
    ) -> MaildirEntryCopyError {
        match cor.resume(arg) {
            MaildirCoroutineState::Complete(Err(err)) => err,
            state => panic!("expected Complete(Err), got {state:?}"),
        }
    }
}
