//! I/O-free coroutine to copy a Maildir message.

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
pub enum MaildirMessageCopyError {
    #[error("invalid Maildir message copy reply {0:?} for state {1:?}")]
    Invalid(Option<MaildirReply>, State),

    #[error(transparent)]
    Locate(#[from] MaildirMessageLocateError),
}

/// Internal progression state of [`MaildirMessageCopy`].
#[derive(Clone, Debug, Default)]
pub enum State {
    Locate(MaildirMessageLocate),
    Copied,
    #[default]
    Invalid,
}

/// I/O-free coroutine to copy a Maildir message to another Maildir.
#[derive(Debug)]
pub struct MaildirMessageCopy {
    id: String,
    target: Maildir,
    target_subdir: Option<MaildirSubdir>,
    state: State,
}

impl MaildirMessageCopy {
    /// Creates a new coroutine that will copy message `id` from
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

impl MaildirCoroutine for MaildirMessageCopy {
    type Yield = MaildirYield;
    type Return = Result<(), MaildirMessageCopyError>;

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
                    self.state = State::Copied;
                    MaildirCoroutineState::Yielded(MaildirYield::WantsCopy(pairs))
                }
                MaildirCoroutineState::Yielded(y) => {
                    self.state = State::Locate(c);
                    MaildirCoroutineState::Yielded(y)
                }
                MaildirCoroutineState::Complete(Err(err)) => {
                    MaildirCoroutineState::Complete(Err(err.into()))
                }
            },
            (State::Copied, Some(MaildirReply::Copy)) => {
                trace!("copied source to target");
                MaildirCoroutineState::Complete(Ok(()))
            }
            (state, arg) => {
                let err = MaildirMessageCopyError::Invalid(arg, state);
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
