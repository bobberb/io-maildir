//! I/O-free coroutine to create a Maildir.

use alloc::collections::BTreeSet;

use log::trace;
use thiserror::Error;

use crate::{
    coroutine::*,
    maildir::{CUR, NEW, TMP},
    path::MaildirPath,
};

/// Errors that can occur during the coroutine progression.
#[derive(Clone, Debug, Error)]
pub enum MaildirCreateError {
    #[error("invalid Maildir create reply: {0:?}")]
    Invalid(Option<MaildirReply>),
}

/// I/O-free coroutine to create a Maildir with its `cur`, `new` and
/// `tmp` subdirectories.
///
/// All four directories are created in a single I/O request. The
/// [`BTreeSet`] guarantees lexicographic order, so `root` is always
/// created before its subdirectories.
#[derive(Debug)]
pub struct MaildirCreate {
    wants_dir_create: Option<BTreeSet<MaildirPath>>,
}

impl MaildirCreate {
    /// Creates a new coroutine that will initialise a Maildir rooted
    /// at `root`.
    pub fn new(root: impl Into<MaildirPath>) -> Self {
        let root = root.into();
        let cur = root.join(CUR);
        let new = root.join(NEW);
        let tmp = root.join(TMP);

        let paths = BTreeSet::from_iter([root, cur, new, tmp]);

        Self {
            wants_dir_create: Some(paths),
        }
    }
}

impl MaildirCoroutine for MaildirCreate {
    type Yield = MaildirYield;
    type Return = Result<(), MaildirCreateError>;

    fn resume(
        &mut self,
        arg: Option<MaildirReply>,
    ) -> MaildirCoroutineState<Self::Yield, Self::Return> {
        match (self.wants_dir_create.take(), arg) {
            (Some(paths), None) => {
                trace!("wants create of {} directories", paths.len());
                MaildirCoroutineState::Yielded(MaildirYield::WantsDirCreate(paths))
            }
            (None, Some(MaildirReply::DirCreate)) => {
                trace!("maildir created");
                MaildirCoroutineState::Complete(Ok(()))
            }
            (_, arg) => {
                let err = MaildirCreateError::Invalid(arg);
                MaildirCoroutineState::Complete(Err(err))
            }
        }
    }
}
