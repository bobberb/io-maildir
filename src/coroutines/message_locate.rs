//! I/O-free coroutine to locate a Maildir message by its ID.

use core::mem;

use alloc::{
    collections::BTreeSet,
    string::{String, ToString},
};

use log::trace;
use thiserror::Error;

use crate::{
    coroutine::*,
    flag::MaildirFlags,
    maildir::{Maildir, MaildirSubdir},
    path::MaildirPath,
};

/// Errors that can occur during the coroutine progression.
#[derive(Clone, Debug, Error)]
pub enum MaildirMessageLocateError {
    #[error("invalid Maildir locate reply {0:?} for state {1:?}")]
    Invalid(Option<MaildirReply>, State),

    /// No message with the given ID was found in the Maildir.
    #[error("message {0} not found in Maildir")]
    NotFound(String),
}

/// Successful output of [`MaildirMessageLocate`].
#[derive(Clone, Debug)]
pub struct MaildirMessageLocateOutput {
    pub path: MaildirPath,
    pub subdir: MaildirSubdir,
    pub flags: MaildirFlags,
}

/// Internal progression state of [`MaildirMessageLocate`].
#[derive(Clone, Debug, Default)]
pub enum State {
    /// Initial state: about to probe `/new/<id>` and `/tmp/<id>`.
    Start { maildir: Maildir, id: String },
    /// Probes issued; awaiting their results.
    CheckingNewTmp {
        maildir: Maildir,
        id: String,
        new_path: MaildirPath,
        tmp_path: MaildirPath,
    },
    /// `/new` and `/tmp` did not contain the id; scanning `/cur`.
    ReadingCur { id: String },
    #[default]
    Invalid,
}

/// I/O-free coroutine to locate a Maildir message file by its ID.
///
/// Probes `/new/<id>` and `/tmp/<id>` first; if neither exists,
/// scans `/cur` for an entry whose filename starts with the id.
#[derive(Clone, Debug)]
pub struct MaildirMessageLocate {
    state: State,
}

impl MaildirMessageLocate {
    /// Creates a new coroutine that will search for message `id`
    /// inside `maildir`.
    pub fn new(maildir: Maildir, id: impl ToString) -> Self {
        Self {
            state: State::Start {
                maildir,
                id: id.to_string(),
            },
        }
    }
}

impl MaildirCoroutine for MaildirMessageLocate {
    type Yield = MaildirYield;
    type Return = Result<MaildirMessageLocateOutput, MaildirMessageLocateError>;

    fn resume(
        &mut self,
        arg: Option<MaildirReply>,
    ) -> MaildirCoroutineState<Self::Yield, Self::Return> {
        match (mem::take(&mut self.state), arg) {
            (State::Start { maildir, id }, None) => {
                let new_path = maildir.new().join(&id);
                let tmp_path = maildir.tmp().join(&id);
                trace!("wants file-exists for {new_path}, {tmp_path}");

                let probes = BTreeSet::from_iter([new_path.clone(), tmp_path.clone()]);
                self.state = State::CheckingNewTmp {
                    maildir,
                    id,
                    new_path,
                    tmp_path,
                };
                MaildirCoroutineState::Yielded(MaildirYield::WantsFileExists(probes))
            }
            (
                State::CheckingNewTmp {
                    maildir,
                    id,
                    new_path,
                    tmp_path,
                },
                Some(MaildirReply::FileExists(probes)),
            ) => {
                if probes.get(&new_path).copied().unwrap_or(false) {
                    trace!("located {id} in /new");
                    return MaildirCoroutineState::Complete(Ok(MaildirMessageLocateOutput {
                        path: new_path,
                        subdir: MaildirSubdir::New,
                        flags: MaildirFlags::default(),
                    }));
                }

                if probes.get(&tmp_path).copied().unwrap_or(false) {
                    trace!("located {id} in /tmp");
                    return MaildirCoroutineState::Complete(Ok(MaildirMessageLocateOutput {
                        path: tmp_path,
                        subdir: MaildirSubdir::Tmp,
                        flags: MaildirFlags::default(),
                    }));
                }

                trace!("wants read of /cur for {id}");
                let paths = BTreeSet::from_iter([maildir.cur()]);
                self.state = State::ReadingCur { id };
                MaildirCoroutineState::Yielded(MaildirYield::WantsDirRead(paths))
            }
            (State::ReadingCur { id }, Some(MaildirReply::DirRead(entries))) => {
                let paths = entries.into_values().next().unwrap_or_default();

                for path in paths {
                    let Some(name) = path.file_name() else {
                        continue;
                    };

                    if !name.starts_with(&id) {
                        continue;
                    }

                    let flags = MaildirFlags::from(&path);
                    trace!("located {id} in /cur at {path}");
                    return MaildirCoroutineState::Complete(Ok(MaildirMessageLocateOutput {
                        path,
                        subdir: MaildirSubdir::Cur,
                        flags,
                    }));
                }

                MaildirCoroutineState::Complete(Err(MaildirMessageLocateError::NotFound(id)))
            }
            (state, arg) => {
                let err = MaildirMessageLocateError::Invalid(arg, state);
                MaildirCoroutineState::Complete(Err(err))
            }
        }
    }
}
