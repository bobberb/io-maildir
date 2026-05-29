//! I/O-free coroutine to list Maildirs inside a root directory.

use core::mem;

use alloc::collections::{BTreeMap, BTreeSet};

use log::trace;
use thiserror::Error;

use crate::{
    coroutine::*,
    maildir::{CUR, Maildir, NEW, TMP},
    path::MaildirPath,
};

/// Errors that can occur during the coroutine progression.
#[derive(Clone, Debug, Error)]
pub enum MaildirListError {
    #[error("invalid Maildir list reply {0:?} for state {1:?}")]
    Invalid(Option<MaildirReply>, State),
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

/// I/O-free coroutine to list all valid Maildirs inside a root
/// directory.
///
/// By default, entries starting with `.` are skipped. Set
/// [`Self::include_dotted`] to include them (required by Maildir++
/// where folders are stored as dotted siblings like `.Work.Foo`). A
/// child is reported as a Maildir when it contains all three of
/// `cur`, `new` and `tmp` as subdirectories.
#[derive(Debug)]
pub struct MaildirList {
    state: State,
    include_dotted: bool,
    include_root: bool,
}

impl MaildirList {
    /// Creates a new coroutine that will list Maildirs inside `root`.
    pub fn new(root: impl Into<MaildirPath>) -> Self {
        Self {
            state: State::Start(root.into()),
            include_dotted: false,
            include_root: false,
        }
    }

    /// Configures the coroutine to surface dotted (`.`-prefixed)
    /// folders during enumeration. Required for Maildir++ where
    /// folders live as siblings of the root with a leading dot.
    pub fn include_dotted(mut self, include: bool) -> Self {
        self.include_dotted = include;
        self
    }

    /// Configures the coroutine to also probe the root directory
    /// itself for `cur`/`new`/`tmp`. Required for Maildir++ where
    /// the root is the inbox; not reported by default to preserve
    /// historical "children only" semantics.
    pub fn include_root(mut self, include: bool) -> Self {
        self.include_root = include;
        self
    }
}

impl MaildirCoroutine for MaildirList {
    type Yield = MaildirYield;
    type Return = Result<BTreeSet<Maildir>, MaildirListError>;

    fn resume(
        &mut self,
        arg: Option<MaildirReply>,
    ) -> MaildirCoroutineState<Self::Yield, Self::Return> {
        match (mem::take(&mut self.state), arg) {
            (State::Start(root), None) => {
                trace!("wants read of {root}");

                let paths = BTreeSet::from_iter([root.clone()]);
                self.state = State::Start(root);
                MaildirCoroutineState::Yielded(MaildirYield::WantsDirRead(paths))
            }
            (State::Start(_), Some(MaildirReply::DirRead(entries))) => {
                let mut markers = BTreeMap::new();

                for (dir, names) in entries {
                    if self.include_root {
                        markers.insert(dir.join(CUR), dir.clone());
                        markers.insert(dir.join(NEW), dir.clone());
                        markers.insert(dir.join(TMP), dir.clone());
                    }

                    for path in names {
                        let Some(name) = path.file_name() else {
                            continue;
                        };

                        if !self.include_dotted && name.starts_with('.') {
                            continue;
                        }

                        markers.insert(path.join(CUR), path.clone());
                        markers.insert(path.join(NEW), path.clone());
                        markers.insert(path.join(TMP), path.clone());
                    }
                }

                if markers.is_empty() {
                    trace!("no candidate maildirs");
                    return MaildirCoroutineState::Complete(Ok(BTreeSet::new()));
                }

                let probes: BTreeSet<MaildirPath> = markers.keys().cloned().collect();
                trace!("wants dir-exists check for {} probes", probes.len());

                self.state = State::CheckingSubdirs { markers };
                MaildirCoroutineState::Yielded(MaildirYield::WantsDirExists(probes))
            }
            (State::CheckingSubdirs { markers }, Some(MaildirReply::DirExists(probes))) => {
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
                MaildirCoroutineState::Complete(Ok(found))
            }
            (state, arg) => {
                let err = MaildirListError::Invalid(arg, state);
                MaildirCoroutineState::Complete(Err(err))
            }
        }
    }
}
