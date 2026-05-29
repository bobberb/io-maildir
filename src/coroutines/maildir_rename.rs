//! I/O-free coroutine to rename a Maildir.

use alloc::{string::ToString, vec::Vec};

use log::trace;
use thiserror::Error;

use crate::{coroutine::*, path::MaildirPath};

/// Errors that can occur during the coroutine progression.
#[derive(Clone, Debug, Error)]
pub enum MaildirRenameError {
    #[error("invalid Maildir rename arg: {0:?}")]
    Invalid(Option<MaildirRenameArg>),
}

/// Argument fed back into [`MaildirRename`].
#[derive(Clone, Debug)]
pub enum MaildirRenameArg {
    /// Response to [`MaildirCoroutineState::WantsRename`].
    Rename,
}

/// I/O-free coroutine to rename a Maildir directory.
#[derive(Debug)]
pub struct MaildirRename {
    wants_rename: Option<Vec<(MaildirPath, MaildirPath)>>,
}

impl MaildirRename {
    /// Creates a new coroutine that will rename the Maildir at
    /// `path` to `name` (keeping the same parent directory).
    pub fn new(path: impl Into<MaildirPath>, name: impl ToString) -> Self {
        let from = path.into();
        let to = from.with_file_name(&name.to_string());

        Self {
            wants_rename: Some(vec![(from, to)]),
        }
    }
}

impl MaildirCoroutine for MaildirRename {
    type Arg = MaildirRenameArg;
    type Output = ();
    type Error = MaildirRenameError;

    fn resume(
        &mut self,
        arg: Option<Self::Arg>,
    ) -> MaildirCoroutineState<Self::Output, Self::Error> {
        match (self.wants_rename.take(), arg) {
            (Some(pairs), None) => {
                trace!("wants rename of {} path(s)", pairs.len());
                MaildirCoroutineState::WantsRename(pairs)
            }
            (None, Some(MaildirRenameArg::Rename)) => {
                trace!("maildir renamed");
                MaildirCoroutineState::Done(())
            }
            (_, arg) => {
                let err = MaildirRenameError::Invalid(arg);
                MaildirCoroutineState::Err(err)
            }
        }
    }
}
