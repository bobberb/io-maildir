//! I/O-free coroutine to remove flags from a Maildir message.

use core::mem;

use alloc::{
    collections::{BTreeMap, BTreeSet},
    string::{String, ToString},
};

use log::trace;
use thiserror::Error;

use crate::{
    coroutine::*,
    coroutines::message_locate::*,
    flag::MaildirFlags,
    maildir::{Maildir, MaildirSubdir},
    message::INFORMATIONAL_SUFFIX_SEPARATOR,
    path::MaildirPath,
};

/// Errors that can occur during the coroutine progression.
#[derive(Clone, Debug, Error)]
pub enum MaildirFlagsRemoveError {
    #[error("invalid Maildir flags remove arg {0:?} for state {1:?}")]
    Invalid(Option<MaildirFlagsRemoveArg>, State),

    #[error(transparent)]
    Locate(#[from] MaildirMessageLocateError),
}

/// Internal progression state of [`MaildirFlagsRemove`].
#[derive(Clone, Debug, Default)]
pub enum State {
    Locate(MaildirMessageLocate),
    Renamed,
    #[default]
    Invalid,
}

/// Argument fed back into [`MaildirFlagsRemove`].
#[derive(Clone, Debug)]
pub enum MaildirFlagsRemoveArg {
    /// Forwarded to the inner locate coroutine.
    FileExists(BTreeMap<MaildirPath, bool>),

    /// Forwarded to the inner locate coroutine.
    DirRead(BTreeMap<MaildirPath, BTreeSet<MaildirPath>>),

    /// Response to [`MaildirCoroutineState::WantsRename`].
    Rename,
}

/// I/O-free coroutine to remove flags from a Maildir message.
///
/// Only messages in `/cur` carry flags; messages in `/new` or `/tmp`
/// are left unchanged.
#[derive(Debug)]
pub struct MaildirFlagsRemove {
    state: State,
    id: String,
    flags: MaildirFlags,
}

impl MaildirFlagsRemove {
    /// Creates a new coroutine that will remove `flags` from message
    /// `id` in `maildir`.
    pub fn new(maildir: Maildir, id: impl ToString, flags: MaildirFlags) -> Self {
        let id = id.to_string();
        Self {
            state: State::Locate(MaildirMessageLocate::new(maildir, &id)),
            id,
            flags,
        }
    }
}

impl MaildirCoroutine for MaildirFlagsRemove {
    type Arg = MaildirFlagsRemoveArg;
    type Output = ();
    type Error = MaildirFlagsRemoveError;

    fn resume(
        &mut self,
        arg: Option<Self::Arg>,
    ) -> MaildirCoroutineState<Self::Output, Self::Error> {
        match (mem::take(&mut self.state), arg) {
            (State::Locate(mut c), arg) => {
                let locate_arg = match arg {
                    None => None,
                    Some(MaildirFlagsRemoveArg::FileExists(probes)) => {
                        Some(MaildirMessageLocateArg::FileExists(probes))
                    }
                    Some(MaildirFlagsRemoveArg::DirRead(entries)) => {
                        Some(MaildirMessageLocateArg::DirRead(entries))
                    }
                    Some(other) => {
                        let state = State::Locate(c);
                        let err = MaildirFlagsRemoveError::Invalid(Some(other), state);
                        return MaildirCoroutineState::Err(err);
                    }
                };

                match c.resume(locate_arg) {
                    MaildirCoroutineState::Done(MaildirMessageLocateOk {
                        path,
                        subdir,
                        flags: mut existing,
                    }) => match subdir {
                        MaildirSubdir::New | MaildirSubdir::Tmp => {
                            trace!("message is in /new or /tmp, flags are a no-op");
                            MaildirCoroutineState::Done(())
                        }
                        MaildirSubdir::Cur => {
                            existing.difference(&self.flags);
                            let new_path = rename_with_flags(&path, &self.id, &existing);

                            trace!("rename {path} -> {new_path}");

                            let pairs = vec![(path, new_path)];
                            self.state = State::Renamed;
                            MaildirCoroutineState::WantsRename(pairs)
                        }
                    },
                    MaildirCoroutineState::WantsFileExists(probes) => {
                        self.state = State::Locate(c);
                        MaildirCoroutineState::WantsFileExists(probes)
                    }
                    MaildirCoroutineState::WantsDirRead(paths) => {
                        self.state = State::Locate(c);
                        MaildirCoroutineState::WantsDirRead(paths)
                    }
                    MaildirCoroutineState::Err(err) => MaildirCoroutineState::Err(err.into()),
                    other => unreachable!("MaildirMessageLocate yielded {other:?}"),
                }
            }
            (State::Renamed, Some(MaildirFlagsRemoveArg::Rename)) => {
                MaildirCoroutineState::Done(())
            }
            (state, arg) => {
                let err = MaildirFlagsRemoveError::Invalid(arg, state);
                MaildirCoroutineState::Err(err)
            }
        }
    }
}

fn rename_with_flags(path: &MaildirPath, id: &str, flags: &MaildirFlags) -> MaildirPath {
    let mut name = String::from(id);
    name.push(INFORMATIONAL_SUFFIX_SEPARATOR);
    name.push_str("2,");
    name.push_str(&flags.to_string());
    path.with_file_name(&name)
}
