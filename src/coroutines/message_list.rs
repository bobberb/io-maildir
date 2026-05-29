//! I/O-free coroutine to list message entries in a Maildir.
//!
//! Scans both `/new` and `/cur`, confirms each candidate is a regular
//! file, and returns lightweight [`MaildirEntry`]s. Contents are not
//! read; pair with [`MaildirClient::read_entry`] / `read_entries` /
//! `read_entries_par` to load bodies.
//!
//! [`MaildirClient::read_entry`]: crate::client::MaildirClient::read_entry

use core::mem;

use alloc::collections::BTreeSet;

use log::trace;
use thiserror::Error;

use crate::{coroutine::*, entry::MaildirEntry, maildir::Maildir, path::MaildirPath};

/// Errors that can occur during the coroutine progression.
#[derive(Clone, Debug, Error)]
pub enum MaildirMessagesListError {
    #[error("invalid Maildir messages list reply {0:?} for state {1:?}")]
    Invalid(Option<MaildirReply>, State),
}

/// Internal progression state of [`MaildirMessagesList`].
#[derive(Clone, Debug, Default)]
pub enum State {
    Start(Maildir),
    Reading,
    Checking {
        candidates: BTreeSet<MaildirPath>,
    },
    #[default]
    Invalid,
}

/// I/O-free coroutine that returns every confirmed message entry in
/// a Maildir without reading any body.
#[derive(Debug)]
pub struct MaildirMessagesList {
    state: State,
}

impl MaildirMessagesList {
    /// Creates a new coroutine that will list every entry in
    /// `maildir`.
    pub fn new(maildir: Maildir) -> Self {
        Self {
            state: State::Start(maildir),
        }
    }
}

impl MaildirCoroutine for MaildirMessagesList {
    type Yield = MaildirYield;
    type Return = Result<BTreeSet<MaildirEntry>, MaildirMessagesListError>;

    fn resume(
        &mut self,
        arg: Option<MaildirReply>,
    ) -> MaildirCoroutineState<Self::Yield, Self::Return> {
        match (mem::take(&mut self.state), arg) {
            (State::Start(maildir), None) => {
                trace!("wants read of /new and /cur");

                let paths = BTreeSet::from_iter([maildir.new(), maildir.cur()]);
                self.state = State::Reading;
                MaildirCoroutineState::Yielded(MaildirYield::WantsDirRead(paths))
            }
            (State::Reading, Some(MaildirReply::DirRead(entries))) => {
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
                    trace!("no candidate entries");
                    return MaildirCoroutineState::Complete(Ok(BTreeSet::new()));
                }

                let probes = candidates.clone();
                trace!("wants file-exists for {} candidates", probes.len());

                self.state = State::Checking { candidates };
                MaildirCoroutineState::Yielded(MaildirYield::WantsFileExists(probes))
            }
            (State::Checking { candidates }, Some(MaildirReply::FileExists(probes))) => {
                let confirmed: BTreeSet<MaildirEntry> = candidates
                    .into_iter()
                    .filter(|p| probes.get(p).copied().unwrap_or(false))
                    .map(MaildirEntry::from_path)
                    .collect();

                MaildirCoroutineState::Complete(Ok(confirmed))
            }
            (state, arg) => {
                let err = MaildirMessagesListError::Invalid(arg, state);
                MaildirCoroutineState::Complete(Err(err))
            }
        }
    }
}
