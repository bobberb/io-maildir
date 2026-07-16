//! I/O-free coroutine deleting a Maildir and all its contents.
//!
//! # Example
//!
//! ```rust,no_run
//! use io_maildir::{
//!     client::MaildirClient,
//!     maildir::delete::MaildirDelete,
//!     path::MaildirPath,
//! };
//!
//! let client = MaildirClient::new("/path/to/root");
//!
//! let coroutine = MaildirDelete::new(&client.store, MaildirPath::from("inbox"));
//! client.run(coroutine).unwrap();
//! ```

use core::{fmt, mem};

use alloc::collections::BTreeSet;

use log::debug;
use thiserror::Error;

use crate::{
    coroutine::*,
    path::{MaildirFsPath, MaildirPath},
    store::MaildirStore,
};

/// Failure causes during a [`MaildirDelete`] step.
#[derive(Clone, Debug, Error)]
pub enum MaildirDeleteError {
    /// A reply arrived that does not match the awaited step.
    #[error("Maildir delete failed: unexpected arg {0:?}")]
    UnexpectedArg(Option<MaildirReply>),
}

/// Recursively removes a Maildir directory.
#[derive(Debug)]
pub struct MaildirDelete {
    state: State,
}

impl MaildirDelete {
    /// Builds a coroutine recursively removing the Maildir resolved
    /// from `name` under `store`.
    pub fn new(store: &MaildirStore, name: MaildirPath) -> Self {
        let paths = BTreeSet::from_iter([store.resolve(&name)]);
        Self {
            state: State::Start { paths },
        }
    }
}

impl MaildirCoroutine for MaildirDelete {
    type Yield = MaildirYield;
    type Return = Result<(), MaildirDeleteError>;

    fn resume(
        &mut self,
        arg: Option<MaildirReply>,
    ) -> MaildirCoroutineState<Self::Yield, Self::Return> {
        match (&mut self.state, arg) {
            (State::Start { paths }, None) => {
                let paths = mem::take(paths);
                self.state = State::AwaitRemove;
                MaildirCoroutineState::Yielded(MaildirYield::WantsDirRemove(paths))
            }
            (State::AwaitRemove, Some(MaildirReply::DirRemove)) => {
                debug!("deleted maildir");
                MaildirCoroutineState::Complete(Ok(()))
            }
            (_, arg) => {
                let err = MaildirDeleteError::UnexpectedArg(arg);
                MaildirCoroutineState::Complete(Err(err))
            }
        }
    }
}

#[derive(Debug)]
enum State {
    Start { paths: BTreeSet<MaildirFsPath> },
    AwaitRemove,
}

impl fmt::Display for State {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Start { .. } => f.write_str("start"),
            Self::AwaitRemove => f.write_str("await remove reply"),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::maildir::delete::*;

    fn fs_store() -> MaildirStore {
        MaildirStore {
            root: MaildirFsPath::from("root"),
            maildirpp: false,
        }
    }

    fn maildirpp_store() -> MaildirStore {
        MaildirStore {
            root: MaildirFsPath::from("root"),
            maildirpp: true,
        }
    }

    #[test]
    fn fs_removes_resolved_path() {
        let mut cor = MaildirDelete::new(&fs_store(), MaildirPath::from("inbox"));

        let paths = expect_wants_dir_remove(&mut cor);
        assert_eq!(paths.len(), 1);
        assert!(paths.contains(&MaildirFsPath::from("root/inbox")));

        expect_complete_ok(&mut cor, Some(MaildirReply::DirRemove));
    }

    #[test]
    fn maildirpp_removes_dotted_flat_path() {
        let mut cor = MaildirDelete::new(&maildirpp_store(), MaildirPath::from("Foo/Bar"));

        let paths = expect_wants_dir_remove(&mut cor);
        assert!(paths.contains(&MaildirFsPath::from("root/.Foo.Bar")));
    }

    #[test]
    fn unexpected_reply_returns_error() {
        let mut cor = MaildirDelete::new(&fs_store(), MaildirPath::from("inbox"));
        let _ = expect_wants_dir_remove(&mut cor);

        let err = expect_complete_err(&mut cor, Some(MaildirReply::DirCreate));
        assert!(matches!(err, MaildirDeleteError::UnexpectedArg(_)));
    }

    fn expect_wants_dir_remove(cor: &mut MaildirDelete) -> BTreeSet<MaildirFsPath> {
        match cor.resume(None) {
            MaildirCoroutineState::Yielded(MaildirYield::WantsDirRemove(paths)) => paths,
            state => panic!("expected WantsDirRemove, got {state:?}"),
        }
    }

    fn expect_complete_ok(cor: &mut MaildirDelete, arg: Option<MaildirReply>) {
        match cor.resume(arg) {
            MaildirCoroutineState::Complete(Ok(())) => {}
            state => panic!("expected Complete(Ok), got {state:?}"),
        }
    }

    fn expect_complete_err(
        cor: &mut MaildirDelete,
        arg: Option<MaildirReply>,
    ) -> MaildirDeleteError {
        match cor.resume(arg) {
            MaildirCoroutineState::Complete(Err(err)) => err,
            state => panic!("expected Complete(Err), got {state:?}"),
        }
    }
}
