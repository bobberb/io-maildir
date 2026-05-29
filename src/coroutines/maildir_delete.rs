//! I/O-free coroutine to delete a Maildir.

use alloc::collections::BTreeSet;

use log::trace;
use thiserror::Error;

use crate::{coroutine::*, path::MaildirPath};

/// Errors that can occur during the coroutine progression.
#[derive(Clone, Debug, Error)]
pub enum MaildirDeleteError {
    #[error("invalid Maildir delete reply: {0:?}")]
    Invalid(Option<MaildirReply>),
}

/// I/O-free coroutine to delete a Maildir and all its contents.
#[derive(Debug)]
pub struct MaildirDelete {
    wants_dir_remove: Option<BTreeSet<MaildirPath>>,
}

impl MaildirDelete {
    /// Creates a new coroutine that will recursively remove the
    /// Maildir at `path`.
    pub fn new(path: impl Into<MaildirPath>) -> Self {
        let paths = BTreeSet::from_iter([path.into()]);
        Self {
            wants_dir_remove: Some(paths),
        }
    }
}

impl MaildirCoroutine for MaildirDelete {
    type Yield = MaildirYield;
    type Return = Result<(), MaildirDeleteError>;

    fn resume(
        &mut self,
        arg: Option<MaildirReply>,
    ) -> MaildirCoroutineState<Self::Yield, Self::Return> {
        match (self.wants_dir_remove.take(), arg) {
            (Some(paths), None) => {
                trace!("wants remove of {} directories", paths.len());
                MaildirCoroutineState::Yielded(MaildirYield::WantsDirRemove(paths))
            }
            (None, Some(MaildirReply::DirRemove)) => {
                trace!("maildir removed");
                MaildirCoroutineState::Complete(Ok(()))
            }
            (_, arg) => {
                let err = MaildirDeleteError::Invalid(arg);
                MaildirCoroutineState::Complete(Err(err))
            }
        }
    }
}
