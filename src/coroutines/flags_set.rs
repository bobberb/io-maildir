//! I/O-free coroutine to set (replace) flags on a Maildir message.

use core::mem;

use alloc::string::{String, ToString};

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
    #[error("invalid Maildir flags set reply {0:?} for state {1:?}")]
    Invalid(Option<MaildirReply>, State),

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
    type Yield = MaildirYield;
    type Return = Result<(), MaildirFlagsSetError>;

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
                })) => match subdir {
                    MaildirSubdir::New | MaildirSubdir::Tmp => {
                        trace!("message is in /new or /tmp, flags are a no-op");
                        MaildirCoroutineState::Complete(Ok(()))
                    }
                    MaildirSubdir::Cur => {
                        let new_path = rename_with_flags(&path, &self.id, &self.flags);

                        trace!("rename {path} -> {new_path}");

                        let pairs = vec![(path, new_path)];
                        self.state = State::Renamed;
                        MaildirCoroutineState::Yielded(MaildirYield::WantsRename(pairs))
                    }
                },
                MaildirCoroutineState::Yielded(y) => {
                    self.state = State::Locate(c);
                    MaildirCoroutineState::Yielded(y)
                }
                MaildirCoroutineState::Complete(Err(err)) => {
                    MaildirCoroutineState::Complete(Err(err.into()))
                }
            },
            (State::Renamed, Some(MaildirReply::Rename)) => MaildirCoroutineState::Complete(Ok(())),
            (state, arg) => {
                let err = MaildirFlagsSetError::Invalid(arg, state);
                MaildirCoroutineState::Complete(Err(err))
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
