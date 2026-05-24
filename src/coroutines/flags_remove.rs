//! I/O-free coroutine to remove flags from a Maildir message.

use core::mem;

use alloc::{
    collections::{BTreeMap, BTreeSet},
    string::{String, ToString},
    vec::Vec,
};

use log::trace;
use thiserror::Error;

use crate::{
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

/// Result returned by [`MaildirFlagsRemove::resume`].
#[derive(Clone, Debug)]
pub enum MaildirFlagsRemoveResult {
    /// The coroutine has successfully terminated its progression.
    Ok,

    /// Forwarded from the inner locate coroutine.
    WantsFileExists(BTreeSet<MaildirPath>),

    /// Forwarded from the inner locate coroutine.
    WantsDirRead(BTreeSet<MaildirPath>),

    /// The caller must rename each `(from, to)` pair and feed back
    /// [`MaildirFlagsRemoveArg::Rename`].
    WantsRename(Vec<(MaildirPath, MaildirPath)>),

    /// The coroutine encountered an error.
    Err(MaildirFlagsRemoveError),
}

/// Internal progression state of [`MaildirFlagsRemove`].
#[derive(Clone, Debug, Default)]
pub enum State {
    Locate(MaildirMessageLocate),
    Renamed,
    #[default]
    Invalid,
}

/// Argument fed back to [`MaildirFlagsRemove::resume`].
#[derive(Clone, Debug)]
pub enum MaildirFlagsRemoveArg {
    /// Forwarded to the inner locate coroutine.
    FileExists(BTreeMap<MaildirPath, bool>),

    /// Forwarded to the inner locate coroutine.
    DirRead(BTreeMap<MaildirPath, BTreeSet<MaildirPath>>),

    /// Response to [`MaildirFlagsRemoveResult::WantsRename`].
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

    /// Makes the flags remove progress.
    pub fn resume(
        &mut self,
        arg: Option<impl Into<MaildirFlagsRemoveArg>>,
    ) -> MaildirFlagsRemoveResult {
        match (mem::take(&mut self.state), arg.map(Into::into)) {
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
                        return MaildirFlagsRemoveResult::Err(err);
                    }
                };

                match c.resume(locate_arg) {
                    MaildirMessageLocateResult::Ok {
                        path,
                        subdir,
                        flags: mut existing,
                    } => match subdir {
                        MaildirSubdir::New | MaildirSubdir::Tmp => {
                            trace!("message is in /new or /tmp, flags are a no-op");
                            MaildirFlagsRemoveResult::Ok
                        }
                        MaildirSubdir::Cur => {
                            existing.difference(&self.flags);
                            let new_path = rename_with_flags(&path, &self.id, &existing);

                            trace!("rename {path} -> {new_path}");

                            let pairs = vec![(path, new_path)];
                            self.state = State::Renamed;
                            MaildirFlagsRemoveResult::WantsRename(pairs)
                        }
                    },
                    MaildirMessageLocateResult::WantsFileExists(probes) => {
                        self.state = State::Locate(c);
                        MaildirFlagsRemoveResult::WantsFileExists(probes)
                    }
                    MaildirMessageLocateResult::WantsDirRead(paths) => {
                        self.state = State::Locate(c);
                        MaildirFlagsRemoveResult::WantsDirRead(paths)
                    }
                    MaildirMessageLocateResult::Err(err) => {
                        MaildirFlagsRemoveResult::Err(err.into())
                    }
                }
            }
            (State::Renamed, Some(MaildirFlagsRemoveArg::Rename)) => MaildirFlagsRemoveResult::Ok,
            (state, arg) => {
                let err = MaildirFlagsRemoveError::Invalid(arg, state);
                MaildirFlagsRemoveResult::Err(err)
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
