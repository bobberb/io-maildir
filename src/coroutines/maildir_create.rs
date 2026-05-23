//! I/O-free coroutine to create a Maildir.

use alloc::collections::BTreeSet;

use log::trace;
use thiserror::Error;

use crate::{
    maildir::{CUR, NEW, TMP},
    path::MaildirPath,
};

/// Errors that can occur during the coroutine progression.
#[derive(Clone, Debug, Error)]
pub enum MaildirCreateError {
    #[error("invalid Maildir create arg: {0:?}")]
    Invalid(Option<MaildirCreateArg>),
}

/// Result returned by [`MaildirCreate::resume`].
#[derive(Clone, Debug)]
pub enum MaildirCreateResult {
    /// The coroutine has successfully terminated its progression.
    Ok,

    /// The caller must create the given directories and feed back
    /// [`MaildirCreateArg::DirCreate`].
    WantsDirCreate(BTreeSet<MaildirPath>),

    /// The coroutine encountered an error.
    Err(MaildirCreateError),
}

/// Argument fed back to [`MaildirCreate::resume`].
#[derive(Clone, Debug)]
pub enum MaildirCreateArg {
    /// Response to [`MaildirCreateResult::WantsDirCreate`].
    DirCreate,
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

    /// Makes the Maildir creation progress.
    pub fn resume(&mut self, arg: Option<impl Into<MaildirCreateArg>>) -> MaildirCreateResult {
        match (self.wants_dir_create.take(), arg.map(Into::into)) {
            (Some(paths), None) => {
                trace!("wants create of {} directories", paths.len());
                MaildirCreateResult::WantsDirCreate(paths)
            }
            (None, Some(MaildirCreateArg::DirCreate)) => {
                trace!("maildir created");
                MaildirCreateResult::Ok
            }
            (_, arg) => {
                let err = MaildirCreateError::Invalid(arg);
                MaildirCreateResult::Err(err)
            }
        }
    }
}
