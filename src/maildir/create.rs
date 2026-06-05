//! I/O-free coroutine creating a Maildir with its cur/new/tmp subdirs.
//!
//! # Example
//!
//! ```rust,no_run
//! use io_maildir::{
//!     client::MaildirClient,
//!     maildir::create::MaildirCreate,
//!     path::MaildirPath,
//! };
//!
//! let client = MaildirClient::new("/path/to/root");
//!
//! let coroutine = MaildirCreate::new(&client.store, MaildirPath::from("inbox"));
//! client.run(coroutine).unwrap();
//! ```

use core::{fmt, mem};

use alloc::collections::BTreeSet;

use log::trace;
use thiserror::Error;

use crate::{
    coroutine::*,
    maildir::types::{CUR, NEW, TMP},
    path::{FsPath, MaildirPath},
    store::MaildirStore,
};

/// Failure causes during a [`MaildirCreate`] step.
#[derive(Clone, Debug, Error)]
pub enum MaildirCreateError {
    #[error("Maildir create failed: unexpected arg {0:?}")]
    UnexpectedArg(Option<MaildirReply>),
}

/// Creates a Maildir with its cur, new and tmp subdirectories.
#[derive(Debug)]
pub struct MaildirCreate {
    state: State,
}

impl MaildirCreate {
    pub fn new(store: &MaildirStore, name: MaildirPath) -> Self {
        let root = store.resolve(&name);
        let cur = root.join(CUR);
        let new = root.join(NEW);
        let tmp = root.join(TMP);

        // NOTE: BTreeSet's lexicographic order guarantees `root` is
        // created before its subdirectories.
        let paths = BTreeSet::from_iter([root, cur, new, tmp]);

        Self {
            state: State::Start { paths },
        }
    }
}

impl MaildirCoroutine for MaildirCreate {
    type Yield = MaildirYield;
    type Return = Result<(), MaildirCreateError>;

    fn resume(
        &mut self,
        arg: Option<MaildirReply>,
    ) -> MaildirCoroutineState<Self::Yield, Self::Return> {
        trace!("maildir create: {}", self.state);

        match (&mut self.state, arg) {
            (State::Start { paths }, None) => {
                let paths = mem::take(paths);
                self.state = State::AwaitCreate;
                MaildirCoroutineState::Yielded(MaildirYield::WantsDirCreate(paths))
            }
            (State::AwaitCreate, Some(MaildirReply::DirCreate)) => {
                MaildirCoroutineState::Complete(Ok(()))
            }
            (_, arg) => {
                let err = MaildirCreateError::UnexpectedArg(arg);
                MaildirCoroutineState::Complete(Err(err))
            }
        }
    }
}

#[derive(Debug)]
enum State {
    Start { paths: BTreeSet<FsPath> },
    AwaitCreate,
}

impl fmt::Display for State {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Start { .. } => f.write_str("start"),
            Self::AwaitCreate => f.write_str("await create reply"),
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
    fn fs_creates_nested_subdirs() {
        let mut cor = MaildirCreate::new(&fs_store(), MaildirPath::from("inbox"));

        let paths = expect_wants_dir_create(&mut cor);
        assert_eq!(paths.len(), 4);
        assert!(paths.contains(&FsPath::from("root/inbox")));
        assert!(paths.contains(&FsPath::from("root/inbox/cur")));
        assert!(paths.contains(&FsPath::from("root/inbox/new")));
        assert!(paths.contains(&FsPath::from("root/inbox/tmp")));

        expect_complete_ok(&mut cor, Some(MaildirReply::DirCreate));
    }

    #[test]
    fn maildirpp_flattens_logical_hierarchy_with_dots() {
        let mut cor = MaildirCreate::new(&maildirpp_store(), MaildirPath::from("Foo/Bar"));

        let paths = expect_wants_dir_create(&mut cor);
        assert!(paths.contains(&FsPath::from("root/.Foo.Bar")));
        assert!(paths.contains(&FsPath::from("root/.Foo.Bar/cur")));
        assert!(paths.contains(&FsPath::from("root/.Foo.Bar/new")));
        assert!(paths.contains(&FsPath::from("root/.Foo.Bar/tmp")));
    }

    #[test]
    fn empty_name_targets_store_root() {
        let mut cor = MaildirCreate::new(&fs_store(), MaildirPath::default());

        let paths = expect_wants_dir_create(&mut cor);
        assert!(paths.contains(&FsPath::from("root")));
        assert!(paths.contains(&FsPath::from("root/cur")));
    }

    #[test]
    fn unexpected_reply_returns_error() {
        let mut cor = MaildirCreate::new(&fs_store(), MaildirPath::from("inbox"));
        let _ = expect_wants_dir_create(&mut cor);

        let err = expect_complete_err(&mut cor, Some(MaildirReply::FileCreate));
        assert!(matches!(err, MaildirCreateError::UnexpectedArg(_)));
    }

    // --- utils

    fn expect_wants_dir_create(cor: &mut MaildirCreate) -> BTreeSet<FsPath> {
        match cor.resume(None) {
            MaildirCoroutineState::Yielded(MaildirYield::WantsDirCreate(paths)) => paths,
            state => panic!("expected WantsDirCreate, got {state:?}"),
        }
    }

    fn expect_complete_ok(cor: &mut MaildirCreate, arg: Option<MaildirReply>) {
        match cor.resume(arg) {
            MaildirCoroutineState::Complete(Ok(())) => {}
            state => panic!("expected Complete(Ok), got {state:?}"),
        }
    }

    fn expect_complete_err(
        cor: &mut MaildirCreate,
        arg: Option<MaildirReply>,
    ) -> MaildirCreateError {
        match cor.resume(arg) {
            MaildirCoroutineState::Complete(Err(err)) => err,
            state => panic!("expected Complete(Err), got {state:?}"),
        }
    }
}
