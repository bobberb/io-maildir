//! I/O-free coroutine to delete a Maildir.

use alloc::collections::BTreeSet;

use log::trace;
use thiserror::Error;

use crate::{coroutine::*, path::MaildirPath};

/// Errors that can occur during the coroutine progression.
#[derive(Clone, Debug, Error)]
pub enum MaildirDeleteError {
    #[error("invalid Maildir delete arg: {0:?}")]
    Invalid(Option<MaildirDeleteArg>),
}

/// Argument fed back into [`MaildirDelete`].
#[derive(Clone, Debug)]
pub enum MaildirDeleteArg {
    /// Response to [`MaildirCoroutineState::WantsDirRemove`].
    DirRemove,
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
    type Arg = MaildirDeleteArg;
    type Output = ();
    type Error = MaildirDeleteError;

    fn resume(
        &mut self,
        arg: Option<Self::Arg>,
    ) -> MaildirCoroutineState<Self::Output, Self::Error> {
        match (self.wants_dir_remove.take(), arg) {
            (Some(paths), None) => {
                trace!("wants remove of {} directories", paths.len());
                MaildirCoroutineState::WantsDirRemove(paths)
            }
            (None, Some(MaildirDeleteArg::DirRemove)) => {
                trace!("maildir removed");
                MaildirCoroutineState::Done(())
            }
            (_, arg) => {
                let err = MaildirDeleteError::Invalid(arg);
                MaildirCoroutineState::Err(err)
            }
        }
    }
}
