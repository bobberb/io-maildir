//! I/O-free coroutine moving a Maildir entry to another Maildir.
//!
//! # Example
//!
//! ```rust,no_run
//! use io_maildir::{client::MaildirClient, entry::r#move::MaildirEntryMove};
//!
//! let client = MaildirClient::new("/path/to/root");
//! let source = client.load_maildir("inbox").unwrap();
//! let target = client.load_maildir("archive").unwrap();
//!
//! let coroutine = MaildirEntryMove::new("1700000000.1.M0P1.host", source, target, None);
//! client.run(coroutine).unwrap();
//! ```

use core::fmt;

use alloc::string::{String, ToString};

use log::debug;
use thiserror::Error;

use crate::{
    coroutine::*,
    entry::INFORMATIONAL_SUFFIX_SEPARATOR,
    entry::locate::*,
    maildir::{Maildir, MaildirSubdir},
    maildir_try,
    path::MaildirFsPath,
};

/// Failure causes during a [`MaildirEntryMove`] step.
#[derive(Clone, Debug, Error)]
pub enum MaildirEntryMoveError {
    /// A reply arrived that does not match the awaited step.
    #[error("Maildir message move failed: unexpected arg {0:?}")]
    UnexpectedArg(Option<MaildirReply>),
    /// The inner locate step failed.
    #[error(transparent)]
    Locate(#[from] MaildirEntryLocateError),
}

/// Moves a Maildir entry into `target`; `None` target_subdir keeps
/// the source subdir.
#[derive(Debug)]
pub struct MaildirEntryMove {
    id: String,
    target: Maildir,
    target_subdir: Option<MaildirSubdir>,
    state: State,
}

impl MaildirEntryMove {
    /// Builds a coroutine moving the Maildir entry into the target
    /// Maildir.
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

impl MaildirCoroutine for MaildirEntryMove {
    type Yield = MaildirYield;
    type Return = Result<(), MaildirEntryMoveError>;

    fn resume(
        &mut self,
        arg: Option<MaildirReply>,
    ) -> MaildirCoroutineState<Self::Yield, Self::Return> {
        match (&mut self.state, arg) {
            (State::Locate(c), arg) => {
                let out = maildir_try!(c, arg);

                let target_subdir = self.target_subdir.clone().unwrap_or(out.subdir);
                let target = build_target_path(&self.target, &target_subdir, &self.id);
                let pairs = vec![(out.path, target)];
                self.state = State::AwaitRename;
                MaildirCoroutineState::Yielded(MaildirYield::WantsRename(pairs))
            }
            (State::AwaitRename, Some(MaildirReply::Rename)) => {
                debug!("moved entry");
                MaildirCoroutineState::Complete(Ok(()))
            }
            (_, arg) => {
                let err = MaildirEntryMoveError::UnexpectedArg(arg);
                MaildirCoroutineState::Complete(Err(err))
            }
        }
    }
}

#[derive(Debug)]
enum State {
    Locate(MaildirEntryLocate),
    AwaitRename,
}

impl fmt::Display for State {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Locate(_) => f.write_str("locate source"),
            Self::AwaitRename => f.write_str("await rename reply"),
        }
    }
}

fn build_target_path(target: &Maildir, subdir: &MaildirSubdir, id: &str) -> MaildirFsPath {
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
    use crate::entry::r#move::*;

    fn source() -> Maildir {
        Maildir::from_path("root/src")
    }

    fn target() -> Maildir {
        Maildir::from_path("root/dst")
    }

    #[test]
    fn unexpected_reply_returns_error() {
        let mut cor = MaildirEntryMove::new("abc", source(), target(), None);
        expect_wants_file_exists(&mut cor);

        let err = expect_complete_err(&mut cor, Some(MaildirReply::DirCreate));
        assert!(matches!(err, MaildirEntryMoveError::Locate(_)));
    }

    fn expect_wants_file_exists(cor: &mut MaildirEntryMove) {
        match cor.resume(None) {
            MaildirCoroutineState::Yielded(MaildirYield::WantsFileExists(_)) => {}
            state => panic!("expected WantsFileExists, got {state:?}"),
        }
    }

    fn expect_complete_err(
        cor: &mut MaildirEntryMove,
        arg: Option<MaildirReply>,
    ) -> MaildirEntryMoveError {
        match cor.resume(arg) {
            MaildirCoroutineState::Complete(Err(err)) => err,
            state => panic!("expected Complete(Err), got {state:?}"),
        }
    }
}
