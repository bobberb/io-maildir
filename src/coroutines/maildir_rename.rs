//! I/O-free coroutine to rename a Maildir.

use alloc::{string::ToString, vec::Vec};

use log::trace;
use thiserror::Error;

use crate::path::MaildirPath;

/// Errors that can occur during the coroutine progression.
#[derive(Clone, Debug, Error)]
pub enum MaildirRenameError {
    #[error("invalid Maildir rename arg: {0:?}")]
    Invalid(Option<MaildirRenameArg>),
}

/// Result returned by [`MaildirRename::resume`].
#[derive(Clone, Debug)]
pub enum MaildirRenameResult {
    /// The coroutine has successfully terminated its progression.
    Ok,

    /// The caller must rename each `(from, to)` pair and feed back
    /// [`MaildirRenameArg::Rename`].
    WantsRename(Vec<(MaildirPath, MaildirPath)>),

    /// The coroutine encountered an error.
    Err(MaildirRenameError),
}

/// Argument fed back to [`MaildirRename::resume`].
#[derive(Clone, Debug)]
pub enum MaildirRenameArg {
    /// Response to [`MaildirRenameResult::WantsRename`].
    Rename,
}

/// I/O-free coroutine to rename a Maildir directory.
#[derive(Debug)]
pub struct MaildirRename {
    wants_rename: Option<Vec<(MaildirPath, MaildirPath)>>,
}

impl MaildirRename {
    /// Creates a new coroutine that will rename the Maildir at `path`
    /// to `name` (keeping the same parent directory).
    pub fn new(path: impl Into<MaildirPath>, name: impl ToString) -> Self {
        let from = path.into();
        let to = from.with_file_name(&name.to_string());

        Self {
            wants_rename: Some(vec![(from, to)]),
        }
    }

    /// Makes the Maildir rename progress.
    pub fn resume(&mut self, arg: Option<impl Into<MaildirRenameArg>>) -> MaildirRenameResult {
        match (self.wants_rename.take(), arg.map(Into::into)) {
            (Some(pairs), None) => {
                trace!("wants rename of {} path(s)", pairs.len());
                MaildirRenameResult::WantsRename(pairs)
            }
            (None, Some(MaildirRenameArg::Rename)) => {
                trace!("maildir renamed");
                MaildirRenameResult::Ok
            }
            (_, arg) => {
                let err = MaildirRenameError::Invalid(arg);
                MaildirRenameResult::Err(err)
            }
        }
    }
}
