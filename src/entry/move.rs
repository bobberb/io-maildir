//! I/O-free coroutine moving a Maildir entry to another Maildir.

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

/// Failure causes during a [`MaildirEntryMove`] step.
#[derive(Clone, Debug, Error)]
pub enum MaildirEntryMoveError {
    #[error("Maildir message move failed: unexpected arg {0:?}")]
    UnexpectedArg(Option<MaildirReply>),

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
        trace!("entry move: {}", self.state);

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
        let mut cor = MaildirEntryMove::new("abc", source(), target(), None);
        let _ = expect_wants_file_exists(&mut cor);

        let err = expect_complete_err(&mut cor, Some(MaildirReply::DirCreate));
        assert!(matches!(err, MaildirEntryMoveError::Locate(_)));
    }

    // --- utils

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
