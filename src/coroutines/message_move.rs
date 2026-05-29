//! I/O-free coroutine to move a Maildir message.

use core::mem;

use alloc::string::{String, ToString};

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
    #[error("invalid Maildir message move reply {0:?} for state {1:?}")]
    Invalid(Option<MaildirReply>, State),

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
    type Yield = MaildirYield;
    type Return = Result<(), MaildirMessageMoveError>;

    fn resume(
        &mut self,
        arg: Option<MaildirReply>,
    ) -> MaildirCoroutineState<Self::Yield, Self::Return> {
        match (mem::take(&mut self.state), arg) {
            (State::Locate(mut c), arg) => match c.resume(arg) {
                MaildirCoroutineState::Complete(Ok(MaildirMessageLocateOutput {
                    path,
                    subdir,
                    ..
                })) => {
                    trace!("located source at {path}");

                    let target_subdir = self.target_subdir.clone().unwrap_or(subdir);
                    let target = build_target_path(&self.target, &target_subdir, &self.id);

                    let pairs = vec![(path, target)];
                    self.state = State::Renamed;
                    MaildirCoroutineState::Yielded(MaildirYield::WantsRename(pairs))
                }
                MaildirCoroutineState::Yielded(y) => {
                    self.state = State::Locate(c);
                    MaildirCoroutineState::Yielded(y)
                }
                MaildirCoroutineState::Complete(Err(err)) => {
                    MaildirCoroutineState::Complete(Err(err.into()))
                }
            },
            (State::Renamed, Some(MaildirReply::Rename)) => {
                trace!("renamed source to target");
                MaildirCoroutineState::Complete(Ok(()))
            }
            (state, arg) => {
                let err = MaildirMessageMoveError::Invalid(arg, state);
                MaildirCoroutineState::Complete(Err(err))
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
