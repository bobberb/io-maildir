//! I/O-free coroutine to get a Maildir message by its ID.

use core::mem;

use alloc::{collections::BTreeSet, string::ToString};

use log::trace;
use thiserror::Error;

use crate::{
    coroutine::*, coroutines::message_locate::*, maildir::Maildir, message::MaildirMessage,
    path::MaildirPath,
};

/// Errors that can occur during the coroutine progression.
#[derive(Clone, Debug, Error)]
pub enum MaildirMessageGetError {
    #[error("invalid Maildir message get reply {0:?} for state {1:?}")]
    Invalid(Option<MaildirReply>, State),

    #[error(transparent)]
    Locate(#[from] MaildirMessageLocateError),
}

/// Internal progression state of [`MaildirMessageGet`].
#[derive(Clone, Debug, Default)]
pub enum State {
    Locate(MaildirMessageLocate),
    Read(MaildirPath),
    #[default]
    Invalid,
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
}

impl MaildirCoroutine for MaildirMessageGet {
    type Yield = MaildirYield;
    type Return = Result<MaildirMessage, MaildirMessageGetError>;

    fn resume(
        &mut self,
        arg: Option<MaildirReply>,
    ) -> MaildirCoroutineState<Self::Yield, Self::Return> {
        match (mem::take(&mut self.state), arg) {
            (State::Locate(mut c), arg) => match c.resume(arg) {
                MaildirCoroutineState::Complete(Ok(MaildirMessageLocateOutput {
                    path, ..
                })) => {
                    trace!("located message at {path}");

                    let paths = BTreeSet::from_iter([path.clone()]);
                    self.state = State::Read(path);
                    MaildirCoroutineState::Yielded(MaildirYield::WantsFileRead(paths))
                }
                MaildirCoroutineState::Yielded(y) => {
                    self.state = State::Locate(c);
                    MaildirCoroutineState::Yielded(y)
                }
                MaildirCoroutineState::Complete(Err(err)) => {
                    MaildirCoroutineState::Complete(Err(err.into()))
                }
            },
            (State::Read(path), Some(MaildirReply::FileRead(map))) => {
                trace!("read message contents at {path}");

                let contents = map.into_values().next().unwrap_or_default();
                MaildirCoroutineState::Complete(Ok(MaildirMessage::from((path, contents))))
            }
            (state, arg) => {
                let err = MaildirMessageGetError::Invalid(arg, state);
                MaildirCoroutineState::Complete(Err(err))
            }
        }
    }
}
