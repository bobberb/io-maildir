//! I/O-free coroutine to list messages in a Maildir.

use core::mem;

use alloc::{
    collections::{BTreeMap, BTreeSet},
    vec::Vec,
};

use log::trace;
use thiserror::Error;

use crate::{maildir::Maildir, message::Message, path::MaildirPath};

/// Errors that can occur during the coroutine progression.
#[derive(Clone, Debug, Error)]
pub enum MaildirMessagesListError {
    #[error("invalid Maildir messages list arg {0:?} for state {1:?}")]
    Invalid(Option<MaildirMessagesListArg>, State),
}

/// Result returned by [`MaildirMessagesList::resume`].
#[derive(Clone, Debug)]
pub enum MaildirMessagesListResult {
    /// The coroutine has successfully terminated its progression.
    Ok(BTreeSet<Message>),

    /// The caller must read the entries of the given directories and
    /// feed back [`MaildirMessagesListArg::DirRead`].
    WantsDirRead(BTreeSet<MaildirPath>),

    /// The caller must check whether the given paths exist as regular
    /// files and feed back [`MaildirMessagesListArg::FileExists`].
    WantsFileExists(BTreeSet<MaildirPath>),

    /// The caller must read the contents of the given files and feed
    /// back [`MaildirMessagesListArg::FileRead`].
    WantsFileRead(BTreeSet<MaildirPath>),

    /// The coroutine encountered an error.
    Err(MaildirMessagesListError),
}

/// Internal progression state of [`MaildirMessagesList`].
#[derive(Clone, Debug, Default)]
pub enum State {
    Start(Maildir),
    Reading,
    Checking {
        candidates: BTreeSet<MaildirPath>,
    },
    ReadingFiles,
    #[default]
    Invalid,
}

/// Argument fed back to [`MaildirMessagesList::resume`].
#[derive(Clone, Debug)]
pub enum MaildirMessagesListArg {
    /// Response to [`MaildirMessagesListResult::WantsDirRead`].
    DirRead(BTreeMap<MaildirPath, BTreeSet<MaildirPath>>),

    /// Response to [`MaildirMessagesListResult::WantsFileExists`].
    FileExists(BTreeMap<MaildirPath, bool>),

    /// Response to [`MaildirMessagesListResult::WantsFileRead`].
    FileRead(BTreeMap<MaildirPath, Vec<u8>>),
}

/// I/O-free coroutine to list all messages in a Maildir.
///
/// Scans both `/new` and `/cur` in a single batched directory read,
/// confirms each candidate is a regular file, then reads them.
#[derive(Debug)]
pub struct MaildirMessagesList {
    state: State,
}

impl MaildirMessagesList {
    /// Creates a new coroutine that will list all messages in
    /// `maildir`.
    pub fn new(maildir: Maildir) -> Self {
        Self {
            state: State::Start(maildir),
        }
    }

    /// Makes the listing progress.
    pub fn resume(
        &mut self,
        arg: Option<impl Into<MaildirMessagesListArg>>,
    ) -> MaildirMessagesListResult {
        match (mem::take(&mut self.state), arg.map(Into::into)) {
            (State::Start(maildir), None) => {
                trace!("wants read of /new and /cur");

                let paths = BTreeSet::from_iter([maildir.new(), maildir.cur()]);
                self.state = State::Reading;
                MaildirMessagesListResult::WantsDirRead(paths)
            }
            (State::Reading, Some(MaildirMessagesListArg::DirRead(entries))) => {
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
                    trace!("no candidate messages");
                    return MaildirMessagesListResult::Ok(BTreeSet::new());
                }

                let probes = candidates.clone();
                trace!("wants file-exists for {} candidates", probes.len());

                self.state = State::Checking { candidates };
                MaildirMessagesListResult::WantsFileExists(probes)
            }
            (State::Checking { candidates }, Some(MaildirMessagesListArg::FileExists(probes))) => {
                let confirmed: BTreeSet<MaildirPath> = candidates
                    .into_iter()
                    .filter(|p| probes.get(p).copied().unwrap_or(false))
                    .collect();

                if confirmed.is_empty() {
                    trace!("no confirmed messages");
                    return MaildirMessagesListResult::Ok(BTreeSet::new());
                }

                trace!("wants read of {} files", confirmed.len());
                self.state = State::ReadingFiles;
                MaildirMessagesListResult::WantsFileRead(confirmed)
            }
            (State::ReadingFiles, Some(MaildirMessagesListArg::FileRead(contents))) => {
                let messages = contents
                    .into_iter()
                    .map(|(path, contents)| Message::from((path, contents)))
                    .collect();

                MaildirMessagesListResult::Ok(messages)
            }
            (state, arg) => {
                let err = MaildirMessagesListError::Invalid(arg, state);
                MaildirMessagesListResult::Err(err)
            }
        }
    }
}
