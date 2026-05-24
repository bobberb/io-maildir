//! I/O-free coroutine to get a Maildir message by its ID.

use core::mem;

use alloc::{
    collections::{BTreeMap, BTreeSet},
    string::ToString,
    vec::Vec,
};

use log::trace;
use thiserror::Error;

use crate::{
    coroutines::message_locate::*, maildir::Maildir, message::MaildirMessage, path::MaildirPath,
};

/// Errors that can occur during the coroutine progression.
#[derive(Clone, Debug, Error)]
pub enum MaildirMessageGetError {
    #[error("invalid Maildir message get arg {0:?} for state {1:?}")]
    Invalid(Option<MaildirMessageGetArg>, State),

    #[error(transparent)]
    Locate(#[from] MaildirMessageLocateError),
}

/// Result returned by [`MaildirMessageGet::resume`].
#[derive(Clone, Debug)]
pub enum MaildirMessageGetResult {
    /// The coroutine has successfully terminated its progression.
    Ok(MaildirMessage),

    /// Forwarded from the inner locate coroutine.
    WantsFileExists(BTreeSet<MaildirPath>),

    /// Forwarded from the inner locate coroutine.
    WantsDirRead(BTreeSet<MaildirPath>),

    /// The caller must read the contents of the given files and feed
    /// back [`MaildirMessageGetArg::FileRead`].
    WantsFileRead(BTreeSet<MaildirPath>),

    /// The coroutine encountered an error.
    Err(MaildirMessageGetError),
}

/// Internal progression state of [`MaildirMessageGet`].
#[derive(Clone, Debug, Default)]
pub enum State {
    Locate(MaildirMessageLocate),
    Read(MaildirPath),
    #[default]
    Invalid,
}

/// Argument fed back to [`MaildirMessageGet::resume`].
#[derive(Clone, Debug)]
pub enum MaildirMessageGetArg {
    /// Forwarded to the inner locate coroutine.
    FileExists(BTreeMap<MaildirPath, bool>),

    /// Forwarded to the inner locate coroutine.
    DirRead(BTreeMap<MaildirPath, BTreeSet<MaildirPath>>),

    /// Response to [`MaildirMessageGetResult::WantsFileRead`].
    FileRead(BTreeMap<MaildirPath, Vec<u8>>),
}

/// I/O-free coroutine to get a single Maildir message by its ID.
#[derive(Debug)]
pub struct MaildirMessageGet {
    state: State,
}

impl MaildirMessageGet {
    /// Creates a new coroutine that will retrieve message `id` from
    /// `maildir`.
    pub fn new(maildir: Maildir, id: impl ToString) -> Self {
        Self {
            state: State::Locate(MaildirMessageLocate::new(maildir, id)),
        }
    }

    /// Makes the message get progress.
    pub fn resume(
        &mut self,
        arg: Option<impl Into<MaildirMessageGetArg>>,
    ) -> MaildirMessageGetResult {
        match (mem::take(&mut self.state), arg.map(Into::into)) {
            (State::Locate(mut c), arg) => {
                let locate_arg = match arg {
                    None => None,
                    Some(MaildirMessageGetArg::FileExists(probes)) => {
                        Some(MaildirMessageLocateArg::FileExists(probes))
                    }
                    Some(MaildirMessageGetArg::DirRead(entries)) => {
                        Some(MaildirMessageLocateArg::DirRead(entries))
                    }
                    Some(other) => {
                        let state = State::Locate(c);
                        let err = MaildirMessageGetError::Invalid(Some(other), state);
                        return MaildirMessageGetResult::Err(err);
                    }
                };

                match c.resume(locate_arg) {
                    MaildirMessageLocateResult::Ok { path, .. } => {
                        trace!("located message at {path}");

                        let paths = BTreeSet::from_iter([path.clone()]);
                        self.state = State::Read(path);
                        MaildirMessageGetResult::WantsFileRead(paths)
                    }
                    MaildirMessageLocateResult::WantsFileExists(probes) => {
                        self.state = State::Locate(c);
                        MaildirMessageGetResult::WantsFileExists(probes)
                    }
                    MaildirMessageLocateResult::WantsDirRead(paths) => {
                        self.state = State::Locate(c);
                        MaildirMessageGetResult::WantsDirRead(paths)
                    }
                    MaildirMessageLocateResult::Err(err) => {
                        MaildirMessageGetResult::Err(err.into())
                    }
                }
            }
            (State::Read(path), Some(MaildirMessageGetArg::FileRead(map))) => {
                trace!("read message contents at {path}");

                let contents = map.into_values().next().unwrap_or_default();
                MaildirMessageGetResult::Ok(MaildirMessage::from((path, contents)))
            }
            (state, arg) => {
                let err = MaildirMessageGetError::Invalid(arg, state);
                MaildirMessageGetResult::Err(err)
            }
        }
    }
}
