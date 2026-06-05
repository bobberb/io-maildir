//! I/O-free coroutine renaming a Maildir directory.
//!
//! # Example
//!
//! ```rust,no_run
//! use io_maildir::{
//!     client::MaildirClient,
//!     maildir::rename::MaildirRename,
//!     path::MaildirPath,
//! };
//!
//! let client = MaildirClient::new("/path/to/root");
//!
//! let coroutine = MaildirRename::new(
//!     &client.store,
//!     MaildirPath::from("inbox"),
//!     MaildirPath::from("archive"),
//! );
//! client.run(coroutine).unwrap();
//! ```

use core::{fmt, mem};

use alloc::vec::Vec;

use log::trace;
use thiserror::Error;

use crate::{
    coroutine::*,
    path::{FsPath, MaildirPath},
    store::MaildirStore,
};

/// Failure causes during a [`MaildirRename`] step.
#[derive(Clone, Debug, Error)]
pub enum MaildirRenameError {
    #[error("Maildir rename failed: unexpected arg {0:?}")]
    UnexpectedArg(Option<MaildirReply>),
}

/// Renames a Maildir from `from` to `to`, both logical mailbox paths
/// resolved through the store. Crossing parents is fine in either
/// layout: fs moves the directory tree, Maildir++ just renames the
/// flat dotted entry.
#[derive(Debug)]
pub struct MaildirRename {
    state: State,
}

impl MaildirRename {
    pub fn new(store: &MaildirStore, from: MaildirPath, to: MaildirPath) -> Self {
        let from = store.resolve(&from);
        let to = store.resolve(&to);

        Self {
            state: State::Start {
                pairs: vec![(from, to)],
            },
        }
    }
}

impl MaildirCoroutine for MaildirRename {
    type Yield = MaildirYield;
    type Return = Result<(), MaildirRenameError>;

    fn resume(
        &mut self,
        arg: Option<MaildirReply>,
    ) -> MaildirCoroutineState<Self::Yield, Self::Return> {
        trace!("maildir rename: {}", self.state);

        match (&mut self.state, arg) {
            (State::Start { pairs }, None) => {
                let pairs = mem::take(pairs);
                self.state = State::AwaitRename;
                MaildirCoroutineState::Yielded(MaildirYield::WantsRename(pairs))
            }
            (State::AwaitRename, Some(MaildirReply::Rename)) => {
                MaildirCoroutineState::Complete(Ok(()))
            }
            (_, arg) => {
                let err = MaildirRenameError::UnexpectedArg(arg);
                MaildirCoroutineState::Complete(Err(err))
            }
        }
    }
}

#[derive(Debug)]
enum State {
    Start { pairs: Vec<(FsPath, FsPath)> },
    AwaitRename,
}

impl fmt::Display for State {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Start { .. } => f.write_str("start"),
            Self::AwaitRename => f.write_str("await rename reply"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fs_store() -> MaildirStore {
        MaildirStore {
            root: FsPath::from("root"),
            maildirpp: false,
        }
    }

    fn maildirpp_store() -> MaildirStore {
        MaildirStore {
            root: FsPath::from("root"),
            maildirpp: true,
        }
    }

    #[test]
    fn fs_resolves_both_sides() {
        let mut cor = MaildirRename::new(
            &fs_store(),
            MaildirPath::from("Old"),
            MaildirPath::from("New"),
        );

        let pairs = expect_wants_rename(&mut cor);
        assert_eq!(pairs.len(), 1);
        assert_eq!(pairs[0].0, FsPath::from("root/Old"));
        assert_eq!(pairs[0].1, FsPath::from("root/New"));

        expect_complete_ok(&mut cor, Some(MaildirReply::Rename));
    }

    #[test]
    fn maildirpp_flattens_both_sides() {
        let mut cor = MaildirRename::new(
            &maildirpp_store(),
            MaildirPath::from("Foo/Bar"),
            MaildirPath::from("Baz/Qux"),
        );

        let pairs = expect_wants_rename(&mut cor);
        assert_eq!(pairs[0].0, FsPath::from("root/.Foo.Bar"));
        assert_eq!(pairs[0].1, FsPath::from("root/.Baz.Qux"));
    }

    #[test]
    fn unexpected_reply_returns_error() {
        let mut cor = MaildirRename::new(
            &fs_store(),
            MaildirPath::from("Old"),
            MaildirPath::from("New"),
        );
        let _ = expect_wants_rename(&mut cor);

        let err = expect_complete_err(&mut cor, Some(MaildirReply::Copy));
        assert!(matches!(err, MaildirRenameError::UnexpectedArg(_)));
    }

    // --- utils

    fn expect_wants_rename(cor: &mut MaildirRename) -> Vec<(FsPath, FsPath)> {
        match cor.resume(None) {
            MaildirCoroutineState::Yielded(MaildirYield::WantsRename(pairs)) => pairs,
            state => panic!("expected WantsRename, got {state:?}"),
        }
    }

    fn expect_complete_ok(cor: &mut MaildirRename, arg: Option<MaildirReply>) {
        match cor.resume(arg) {
            MaildirCoroutineState::Complete(Ok(())) => {}
            state => panic!("expected Complete(Ok), got {state:?}"),
        }
    }

    fn expect_complete_err(
        cor: &mut MaildirRename,
        arg: Option<MaildirReply>,
    ) -> MaildirRenameError {
        match cor.resume(arg) {
            MaildirCoroutineState::Complete(Err(err)) => err,
            state => panic!("expected Complete(Err), got {state:?}"),
        }
    }
}
