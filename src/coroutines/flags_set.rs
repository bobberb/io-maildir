//! I/O-free coroutine to set (replace) flags on a Maildir message.

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
pub enum MaildirFlagsSetError {
    #[error("invalid Maildir flags set arg {0:?} for state {1:?}")]
    Invalid(Option<MaildirFlagsSetArg>, State),

    #[error(transparent)]
    Locate(#[from] MaildirMessageLocateError),
}

/// Internal progression state of [`MaildirFlagsSet`].
#[derive(Clone, Debug, Default)]
pub enum State {
    Locate(MaildirMessageLocate),
    Renamed,
    #[default]
    Invalid,
}

/// Argument fed back into [`MaildirFlagsSet`].
#[derive(Clone, Debug)]
pub enum MaildirFlagsSetArg {
    /// Forwarded to the inner locate coroutine.
    FileExists(BTreeMap<MaildirPath, bool>),

    /// Forwarded to the inner locate coroutine.
    DirRead(BTreeMap<MaildirPath, BTreeSet<MaildirPath>>),

    /// Response to [`MaildirCoroutineState::WantsRename`].
    Rename,
}

/// I/O-free coroutine to set (replace) the flags of a Maildir
/// message.
///
/// Only messages in `/cur` carry flags; messages in `/new` or `/tmp`
/// are left unchanged.
#[derive(Debug)]
pub struct MaildirFlagsSet {
    state: State,
    id: String,
    flags: MaildirFlags,
}

impl MaildirFlagsSet {
    /// Creates a new coroutine that will replace the flags of
    /// message `id` in `maildir` with `flags`.
    pub fn new(maildir: Maildir, id: impl ToString, flags: MaildirFlags) -> Self {
        let id = id.to_string();
        Self {
            state: State::Locate(MaildirMessageLocate::new(maildir, &id)),
            id,
            flags,
        }
    }
}

impl MaildirCoroutine for MaildirFlagsSet {
    type Arg = MaildirFlagsSetArg;
    type Output = ();
    type Error = MaildirFlagsSetError;

    fn resume(
        &mut self,
        arg: Option<Self::Arg>,
    ) -> MaildirCoroutineState<Self::Output, Self::Error> {
        match (mem::take(&mut self.state), arg) {
            (State::Locate(mut c), arg) => {
                let locate_arg = match arg {
                    None => None,
                    Some(MaildirFlagsSetArg::FileExists(probes)) => {
                        Some(MaildirMessageLocateArg::FileExists(probes))
                    }
                    Some(MaildirFlagsSetArg::DirRead(entries)) => {
                        Some(MaildirMessageLocateArg::DirRead(entries))
                    }
                    Some(other) => {
                        let state = State::Locate(c);
                        let err = MaildirFlagsSetError::Invalid(Some(other), state);
                        return MaildirCoroutineState::Err(err);
                    }
                };

                match c.resume(locate_arg) {
                    MaildirCoroutineState::Done(MaildirMessageLocateOk {
                        path, subdir, ..
                    }) => match subdir {
                        MaildirSubdir::New | MaildirSubdir::Tmp => {
                            trace!("message is in /new or /tmp, flags are a no-op");
                            MaildirCoroutineState::Done(())
                        }
                        MaildirSubdir::Cur => {
                            let new_path = rename_with_flags(&path, &self.id, &self.flags);

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
            (State::Renamed, Some(MaildirFlagsSetArg::Rename)) => MaildirCoroutineState::Done(()),
            (state, arg) => {
                let err = MaildirFlagsSetError::Invalid(arg, state);
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
