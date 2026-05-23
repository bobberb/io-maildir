//! I/O-free coroutine to add flags to a Maildir message.

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
    flag::Flags,
    maildir::{Maildir, MaildirSubdir},
    message::INFORMATIONAL_SUFFIX_SEPARATOR,
    path::MaildirPath,
};

/// Errors that can occur during the coroutine progression.
#[derive(Clone, Debug, Error)]
pub enum MaildirFlagsAddError {
    #[error("invalid Maildir flags add arg {0:?} for state {1:?}")]
    Invalid(Option<MaildirFlagsAddArg>, State),

    #[error(transparent)]
    Locate(#[from] MaildirMessageLocateError),
}

/// Result returned by [`MaildirFlagsAdd::resume`].
#[derive(Clone, Debug)]
pub enum MaildirFlagsAddResult {
    /// The coroutine has successfully terminated its progression.
    Ok,

    /// Forwarded from the inner locate coroutine.
    WantsFileExists(BTreeSet<MaildirPath>),

    /// Forwarded from the inner locate coroutine.
    WantsDirRead(BTreeSet<MaildirPath>),

    /// The caller must rename each `(from, to)` pair and feed back
    /// [`MaildirFlagsAddArg::Rename`].
    WantsRename(Vec<(MaildirPath, MaildirPath)>),

    /// The coroutine encountered an error.
    Err(MaildirFlagsAddError),
}

/// Internal progression state of [`MaildirFlagsAdd`].
#[derive(Clone, Debug, Default)]
pub enum State {
    Locate(MaildirMessageLocate),
    Renamed,
    #[default]
    Invalid,
}

/// Argument fed back to [`MaildirFlagsAdd::resume`].
#[derive(Clone, Debug)]
pub enum MaildirFlagsAddArg {
    /// Forwarded to the inner locate coroutine.
    FileExists(BTreeMap<MaildirPath, bool>),

    /// Forwarded to the inner locate coroutine.
    DirRead(BTreeMap<MaildirPath, BTreeSet<MaildirPath>>),

    /// Response to [`MaildirFlagsAddResult::WantsRename`].
    Rename,
}

/// I/O-free coroutine to add flags to a Maildir message.
///
/// Only messages in `/cur` carry flags; messages in `/new` or `/tmp`
/// are left unchanged.
#[derive(Debug)]
pub struct MaildirFlagsAdd {
    state: State,
    id: String,
    flags: Flags,
}

impl MaildirFlagsAdd {
    /// Creates a new coroutine that will add `flags` to message `id`
    /// in `maildir`.
    pub fn new(maildir: Maildir, id: impl ToString, flags: Flags) -> Self {
        let id = id.to_string();
        Self {
            state: State::Locate(MaildirMessageLocate::new(maildir, &id)),
            id,
            flags,
        }
    }

    /// Makes the flags add progress.
    pub fn resume(&mut self, arg: Option<impl Into<MaildirFlagsAddArg>>) -> MaildirFlagsAddResult {
        match (mem::take(&mut self.state), arg.map(Into::into)) {
            (State::Locate(mut c), arg) => {
                let locate_arg = match arg {
                    None => None,
                    Some(MaildirFlagsAddArg::FileExists(probes)) => {
                        Some(MaildirMessageLocateArg::FileExists(probes))
                    }
                    Some(MaildirFlagsAddArg::DirRead(entries)) => {
                        Some(MaildirMessageLocateArg::DirRead(entries))
                    }
                    Some(other) => {
                        let state = State::Locate(c);
                        let err = MaildirFlagsAddError::Invalid(Some(other), state);
                        return MaildirFlagsAddResult::Err(err);
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
                            MaildirFlagsAddResult::Ok
                        }
                        MaildirSubdir::Cur => {
                            existing.extend(self.flags.clone());
                            let new_path = rename_with_flags(&path, &self.id, &existing);

                            trace!("rename {path} -> {new_path}");

                            let pairs = vec![(path, new_path)];
                            self.state = State::Renamed;
                            MaildirFlagsAddResult::WantsRename(pairs)
                        }
                    },
                    MaildirMessageLocateResult::WantsFileExists(probes) => {
                        self.state = State::Locate(c);
                        MaildirFlagsAddResult::WantsFileExists(probes)
                    }
                    MaildirMessageLocateResult::WantsDirRead(paths) => {
                        self.state = State::Locate(c);
                        MaildirFlagsAddResult::WantsDirRead(paths)
                    }
                    MaildirMessageLocateResult::Err(err) => MaildirFlagsAddResult::Err(err.into()),
                }
            }
            (State::Renamed, Some(MaildirFlagsAddArg::Rename)) => MaildirFlagsAddResult::Ok,
            (state, arg) => {
                let err = MaildirFlagsAddError::Invalid(arg, state);
                MaildirFlagsAddResult::Err(err)
            }
        }
    }
}

fn rename_with_flags(path: &MaildirPath, id: &str, flags: &Flags) -> MaildirPath {
    let mut name = String::from(id);
    name.push(INFORMATIONAL_SUFFIX_SEPARATOR);
    name.push_str("2,");
    name.push_str(&flags.to_string());
    path.with_file_name(&name)
}
