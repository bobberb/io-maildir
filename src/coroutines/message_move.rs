//! I/O-free coroutine to move a Maildir message.

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

/// Internal progression state of [`MaildirMessageMove`].
#[derive(Clone, Debug, Default)]
pub enum State {
    Locate(MaildirMessageLocate),
    Renamed,
    #[default]
    Invalid,
}

/// Argument fed back into [`MaildirMessageMove`].
#[derive(Clone, Debug)]
pub enum MaildirMessageMoveArg {
    /// Forwarded to the inner locate coroutine.
    FileExists(BTreeMap<MaildirPath, bool>),

    /// Forwarded to the inner locate coroutine.
    DirRead(BTreeMap<MaildirPath, BTreeSet<MaildirPath>>),

    /// Response to [`MaildirCoroutineState::WantsRename`].
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
}

impl MaildirCoroutine for MaildirMessageMove {
    type Arg = MaildirMessageMoveArg;
    type Output = ();
    type Error = MaildirMessageMoveError;

    fn resume(
        &mut self,
        arg: Option<Self::Arg>,
    ) -> MaildirCoroutineState<Self::Output, Self::Error> {
        match (mem::take(&mut self.state), arg) {
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
                        return MaildirCoroutineState::Err(err);
                    }
                };

                match c.resume(locate_arg) {
                    MaildirCoroutineState::Done(MaildirMessageLocateOk {
                        path, subdir, ..
                    }) => {
                        trace!("located source at {path}");

                        let target_subdir = self.target_subdir.clone().unwrap_or(subdir);
                        let target = build_target_path(&self.target, &target_subdir, &self.id);

                        let pairs = vec![(path, target)];
                        self.state = State::Renamed;
                        MaildirCoroutineState::WantsRename(pairs)
                    }
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
            (State::Renamed, Some(MaildirMessageMoveArg::Rename)) => {
                trace!("renamed source to target");
                MaildirCoroutineState::Done(())
            }
            (state, arg) => {
                let err = MaildirMessageMoveError::Invalid(arg, state);
                MaildirCoroutineState::Err(err)
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
