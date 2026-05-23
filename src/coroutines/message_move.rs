//! I/O-free coroutine to move a Maildir message.

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
    maildir::{Maildir, MaildirSubdir},
    message::INFORMATIONAL_SUFFIX_SEPARATOR,
    path::MaildirPath,
};

/// Errors that can occur during the coroutine progression.
#[derive(Clone, Debug, Error)]
pub enum MaildirMessageMoveError {
    #[error("invalid Maildir message move arg {0:?} for state {1:?}")]
    Invalid(Option<MaildirMessageMoveArg>, State),

    #[error(transparent)]
    Locate(#[from] MaildirMessageLocateError),
}

/// Result returned by [`MaildirMessageMove::resume`].
#[derive(Clone, Debug)]
pub enum MaildirMessageMoveResult {
    /// The coroutine has successfully terminated its progression.
    Ok,

    /// Forwarded from the inner locate coroutine.
    WantsFileExists(BTreeSet<MaildirPath>),

    /// Forwarded from the inner locate coroutine.
    WantsDirRead(BTreeSet<MaildirPath>),

    /// The caller must rename each `(from, to)` pair and feed back
    /// [`MaildirMessageMoveArg::Rename`].
    WantsRename(Vec<(MaildirPath, MaildirPath)>),

    /// The coroutine encountered an error.
    Err(MaildirMessageMoveError),
}

/// Internal progression state of [`MaildirMessageMove`].
#[derive(Clone, Debug, Default)]
pub enum State {
    Locate(MaildirMessageLocate),
    Renamed,
    #[default]
    Invalid,
}

/// Argument fed back to [`MaildirMessageMove::resume`].
#[derive(Clone, Debug)]
pub enum MaildirMessageMoveArg {
    /// Forwarded to the inner locate coroutine.
    FileExists(BTreeMap<MaildirPath, bool>),

    /// Forwarded to the inner locate coroutine.
    DirRead(BTreeMap<MaildirPath, BTreeSet<MaildirPath>>),

    /// Response to [`MaildirMessageMoveResult::WantsRename`].
    Rename,
}

/// I/O-free coroutine to move a Maildir message to another Maildir.
#[derive(Debug)]
pub struct MaildirMessageMove {
    id: String,
    target: Maildir,
    target_subdir: Option<MaildirSubdir>,
    state: State,
}

impl MaildirMessageMove {
    /// Creates a new coroutine that will move message `id` from
    /// `source` into `target`.
    ///
    /// If `target_subdir` is `None`, the message is placed into the
    /// same subdir as in the source Maildir.
    pub fn new(
        id: impl ToString,
        source: Maildir,
        target: Maildir,
        target_subdir: Option<MaildirSubdir>,
    ) -> Self {
        let id = id.to_string();
        Self {
            state: State::Locate(MaildirMessageLocate::new(source, &id)),
            id,
            target,
            target_subdir,
        }
    }

    /// Makes the message move progress.
    pub fn resume(
        &mut self,
        arg: Option<impl Into<MaildirMessageMoveArg>>,
    ) -> MaildirMessageMoveResult {
        match (mem::take(&mut self.state), arg.map(Into::into)) {
            (State::Locate(mut c), arg) => {
                let locate_arg = match arg {
                    None => None,
                    Some(MaildirMessageMoveArg::FileExists(probes)) => {
                        Some(MaildirMessageLocateArg::FileExists(probes))
                    }
                    Some(MaildirMessageMoveArg::DirRead(entries)) => {
                        Some(MaildirMessageLocateArg::DirRead(entries))
                    }
                    Some(other) => {
                        let state = State::Locate(c);
                        let err = MaildirMessageMoveError::Invalid(Some(other), state);
                        return MaildirMessageMoveResult::Err(err);
                    }
                };

                match c.resume(locate_arg) {
                    MaildirMessageLocateResult::Ok { path, subdir, .. } => {
                        trace!("located source at {path}");

                        let target_subdir = self.target_subdir.clone().unwrap_or(subdir);
                        let target = build_target_path(&self.target, &target_subdir, &self.id);

                        let pairs = vec![(path, target)];
                        self.state = State::Renamed;
                        MaildirMessageMoveResult::WantsRename(pairs)
                    }
                    MaildirMessageLocateResult::WantsFileExists(probes) => {
                        self.state = State::Locate(c);
                        MaildirMessageMoveResult::WantsFileExists(probes)
                    }
                    MaildirMessageLocateResult::WantsDirRead(paths) => {
                        self.state = State::Locate(c);
                        MaildirMessageMoveResult::WantsDirRead(paths)
                    }
                    MaildirMessageLocateResult::Err(err) => {
                        MaildirMessageMoveResult::Err(err.into())
                    }
                }
            }
            (State::Renamed, Some(MaildirMessageMoveArg::Rename)) => {
                trace!("renamed source to target");
                MaildirMessageMoveResult::Ok
            }
            (state, arg) => {
                let err = MaildirMessageMoveError::Invalid(arg, state);
                MaildirMessageMoveResult::Err(err)
            }
        }
    }
}

fn build_target_path(target: &Maildir, subdir: &MaildirSubdir, id: &str) -> MaildirPath {
    match subdir {
        MaildirSubdir::Cur => {
            let name = format!("{id}{INFORMATIONAL_SUFFIX_SEPARATOR}2,");
            target.cur().join(&name)
        }
        MaildirSubdir::New => target.new().join(id),
        MaildirSubdir::Tmp => target.tmp().join(id),
    }
}
