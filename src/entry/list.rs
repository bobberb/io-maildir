//! I/O-free coroutine listing entries (no body) in a Maildir.
//!
//! # Example
//!
//! ```rust,no_run
//! use io_maildir::{client::MaildirClient, entry::list::MaildirEntryList};
//!
//! let client = MaildirClient::new("/path/to/root");
//! let maildir = client.load_maildir("inbox").unwrap();
//!
//! let coroutine = MaildirEntryList::new(maildir);
//! let entries = client.run(coroutine).unwrap();
//!
//! for entry in &entries {
//!     println!("{}", entry.path());
//! }
//! ```

use core::{fmt, mem};

use alloc::collections::BTreeSet;

use log::trace;
use thiserror::Error;

use crate::{coroutine::*, entry::types::MaildirEntry, maildir::types::Maildir, path::FsPath};

/// Failure causes during a [`MaildirEntryList`] step.
#[derive(Clone, Debug, Error)]
pub enum MaildirEntryListError {
    #[error("Maildir messages list failed: unexpected arg {0:?}")]
    UnexpectedArg(Option<MaildirReply>),
}

/// Lists every confirmed entry in a Maildir; bodies are not
/// read.
#[derive(Debug)]
pub struct MaildirEntryList {
    state: State,
}

impl MaildirEntryList {
    pub fn new(maildir: Maildir) -> Self {
        Self {
            state: State::Start { maildir },
        }
    }
}

impl MaildirCoroutine for MaildirEntryList {
    type Yield = MaildirYield;
    type Return = Result<BTreeSet<MaildirEntry>, MaildirEntryListError>;

    fn resume(
        &mut self,
        arg: Option<MaildirReply>,
    ) -> MaildirCoroutineState<Self::Yield, Self::Return> {
        trace!("entries list: {}", self.state);

        match (&mut self.state, arg) {
            (State::Start { maildir }, None) => {
                let paths = BTreeSet::from_iter([maildir.new(), maildir.cur()]);
                self.state = State::AwaitRead;
                MaildirCoroutineState::Yielded(MaildirYield::WantsDirRead(paths))
            }
            (State::AwaitRead, Some(MaildirReply::DirRead(entries))) => {
                let mut candidates = BTreeSet::new();

                for (_dir, names) in entries {
                    for path in names {
                        let Some(name) = path.file_name() else {
                            continue;
                        };

                        if name.starts_with('.') {
                            continue;
                        }

                        candidates.insert(path);
                    }
                }

                if candidates.is_empty() {
                    return MaildirCoroutineState::Complete(Ok(BTreeSet::new()));
                }

                let probes = candidates.clone();
                self.state = State::AwaitProbe { candidates };
                MaildirCoroutineState::Yielded(MaildirYield::WantsFileExists(probes))
            }
            (State::AwaitProbe { candidates }, Some(MaildirReply::FileExists(probes))) => {
                let confirmed: BTreeSet<MaildirEntry> = mem::take(candidates)
                    .into_iter()
                    .filter(|p| probes.get(p).copied().unwrap_or(false))
                    .map(MaildirEntry::from_path)
                    .collect();
                MaildirCoroutineState::Complete(Ok(confirmed))
            }
            (_, arg) => {
                let err = MaildirEntryListError::UnexpectedArg(arg);
                MaildirCoroutineState::Complete(Err(err))
            }
        }
    }
}

#[derive(Debug)]
enum State {
    Start { maildir: Maildir },
    AwaitRead,
    AwaitProbe { candidates: BTreeSet<FsPath> },
}

impl fmt::Display for State {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Start { .. } => f.write_str("start"),
            Self::AwaitRead => f.write_str("await read reply"),
            Self::AwaitProbe { .. } => f.write_str("await probe reply"),
        }
    }
}

#[cfg(test)]
mod tests {
    use alloc::collections::BTreeMap;

    use super::*;

    fn maildir() -> Maildir {
        Maildir::from_path("root")
    }

    #[test]
    fn empty_maildir_returns_empty() {
        let mut cor = MaildirEntryList::new(maildir());
        expect_wants_dir_read(&mut cor);

        let mut entries = BTreeMap::new();
        entries.insert(FsPath::from("root/new"), BTreeSet::new());
        entries.insert(FsPath::from("root/cur"), BTreeSet::new());
        let out = expect_complete_ok(&mut cor, Some(MaildirReply::DirRead(entries)));
        assert!(out.is_empty());
    }

    #[test]
    fn unexpected_reply_returns_error() {
        let mut cor = MaildirEntryList::new(maildir());
        expect_wants_dir_read(&mut cor);

        let err = expect_complete_err(&mut cor, Some(MaildirReply::DirCreate));
        assert!(matches!(err, MaildirEntryListError::UnexpectedArg(_)));
    }

    // --- utils

    fn expect_wants_dir_read(cor: &mut MaildirEntryList) {
        match cor.resume(None) {
            MaildirCoroutineState::Yielded(MaildirYield::WantsDirRead(_)) => {}
            state => panic!("expected WantsDirRead, got {state:?}"),
        }
    }

    fn expect_complete_ok(
        cor: &mut MaildirEntryList,
        arg: Option<MaildirReply>,
    ) -> BTreeSet<MaildirEntry> {
        match cor.resume(arg) {
            MaildirCoroutineState::Complete(Ok(out)) => out,
            state => panic!("expected Complete(Ok), got {state:?}"),
        }
    }

    fn expect_complete_err(
        cor: &mut MaildirEntryList,
        arg: Option<MaildirReply>,
    ) -> MaildirEntryListError {
        match cor.resume(arg) {
            MaildirCoroutineState::Complete(Err(err)) => err,
            state => panic!("expected Complete(Err), got {state:?}"),
        }
    }
}
