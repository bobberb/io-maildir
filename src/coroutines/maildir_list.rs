//! I/O-free coroutine to list Maildirs inside a root directory.

use core::mem;

use alloc::collections::{BTreeMap, BTreeSet};

use log::trace;
use thiserror::Error;

use crate::{
    maildir::{CUR, Maildir, NEW, TMP},
    path::MaildirPath,
};

/// Errors that can occur during the coroutine progression.
#[derive(Clone, Debug, Error)]
pub enum MaildirListError {
    #[error("invalid Maildir list arg {0:?} for state {1:?}")]
    Invalid(Option<MaildirListArg>, State),
}

/// Result returned by [`MaildirList::resume`].
#[derive(Clone, Debug)]
pub enum MaildirListResult {
    /// The coroutine has successfully terminated its progression.
    Ok(BTreeSet<Maildir>),

    /// The caller must read the entries of the given directories and
    /// feed back [`MaildirListArg::DirRead`].
    WantsDirRead(BTreeSet<MaildirPath>),

    /// The caller must check whether the given paths exist as
    /// directories and feed back [`MaildirListArg::DirExists`].
    WantsDirExists(BTreeSet<MaildirPath>),

    /// The coroutine encountered an error.
    Err(MaildirListError),
}

/// Internal progression state of [`MaildirList`].
#[derive(Clone, Debug, Default)]
pub enum State {
    Start(MaildirPath),
    /// Probing each scanned candidate for its `cur`/`new`/`tmp`
    /// subdirectories. `markers` maps each probed path to the
    /// candidate root it belongs to.
    CheckingSubdirs {
        markers: BTreeMap<MaildirPath, MaildirPath>,
    },
    #[default]
    Invalid,
}

/// Argument fed back to [`MaildirList::resume`].
#[derive(Clone, Debug)]
pub enum MaildirListArg {
    /// Response to [`MaildirListResult::WantsDirRead`].
    DirRead(BTreeMap<MaildirPath, BTreeSet<MaildirPath>>),

    /// Response to [`MaildirListResult::WantsDirExists`].
    DirExists(BTreeMap<MaildirPath, bool>),
}

/// I/O-free coroutine to list all valid Maildirs inside a root
/// directory.
///
/// Entries starting with `.` are skipped. A child is reported as a
/// Maildir when it contains all three of `cur`, `new` and `tmp` as
/// subdirectories.
#[derive(Debug)]
pub struct MaildirList {
    state: State,
}

impl MaildirList {
    /// Creates a new coroutine that will list Maildirs inside `root`.
    pub fn new(root: impl Into<MaildirPath>) -> Self {
        Self {
            state: State::Start(root.into()),
        }
    }

    /// Makes the listing progress.
    pub fn resume(&mut self, arg: Option<impl Into<MaildirListArg>>) -> MaildirListResult {
        match (mem::take(&mut self.state), arg.map(Into::into)) {
            (State::Start(root), None) => {
                trace!("wants read of {root}");

                let paths = BTreeSet::from_iter([root.clone()]);
                self.state = State::Start(root);
                MaildirListResult::WantsDirRead(paths)
            }
            (State::Start(_), Some(MaildirListArg::DirRead(entries))) => {
                let mut markers = BTreeMap::new();

                for (_dir, names) in entries {
                    for path in names {
                        let Some(name) = path.file_name() else {
                            continue;
                        };

                        if name.starts_with('.') {
                            continue;
                        }

                        markers.insert(path.join(CUR), path.clone());
                        markers.insert(path.join(NEW), path.clone());
                        markers.insert(path.join(TMP), path.clone());
                    }
                }

                if markers.is_empty() {
                    trace!("no candidate maildirs");
                    return MaildirListResult::Ok(BTreeSet::new());
                }

                let probes: BTreeSet<MaildirPath> = markers.keys().cloned().collect();
                trace!("wants dir-exists check for {} probes", probes.len());

                self.state = State::CheckingSubdirs { markers };
                MaildirListResult::WantsDirExists(probes)
            }
            (State::CheckingSubdirs { markers }, Some(MaildirListArg::DirExists(probes))) => {
                let mut hits: BTreeMap<MaildirPath, u8> = BTreeMap::new();

                for (probe, root) in markers {
                    if probes.get(&probe).copied().unwrap_or(false) {
                        *hits.entry(root).or_insert(0) += 1;
                    }
                }

                let found: BTreeSet<Maildir> = hits
                    .into_iter()
                    .filter(|(_, n)| *n == 3)
                    .map(|(root, _)| Maildir::from_path(root))
                    .collect();

                trace!("found {} maildirs", found.len());
                MaildirListResult::Ok(found)
            }
            (state, arg) => {
                let err = MaildirListError::Invalid(arg, state);
                MaildirListResult::Err(err)
            }
        }
    }
}
