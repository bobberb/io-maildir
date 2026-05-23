//! I/O-free coroutine to copy a Maildir message.

use core::mem;

use alloc::{
    collections::{BTreeMap, BTreeSet},
    string::{String, ToString},
    vec::Vec,
};

use log::trace;
use thiserror::Error;

use crate::{
    coroutines::message_locate::*,
    maildir::{Maildir, MaildirSubdir},
    message::INFORMATIONAL_SUFFIX_SEPARATOR,
    path::MaildirPath,
};

/// Errors that can occur during the coroutine progression.
#[derive(Clone, Debug, Error)]
pub enum MaildirMessageCopyError {
    #[error("invalid Maildir message copy arg {0:?} for state {1:?}")]
    Invalid(Option<MaildirMessageCopyArg>, State),

    #[error(transparent)]
    Locate(#[from] MaildirMessageLocateError),
}

/// Result returned by [`MaildirMessageCopy::resume`].
#[derive(Clone, Debug)]
pub enum MaildirMessageCopyResult {
    /// The coroutine has successfully terminated its progression.
    Ok,

    /// Forwarded from the inner locate coroutine.
    WantsFileExists(BTreeSet<MaildirPath>),

    /// Forwarded from the inner locate coroutine.
    WantsDirRead(BTreeSet<MaildirPath>),

    /// The caller must copy each `(source, target)` pair and feed
    /// back [`MaildirMessageCopyArg::Copy`].
    WantsCopy(Vec<(MaildirPath, MaildirPath)>),

    /// The coroutine encountered an error.
    Err(MaildirMessageCopyError),
}

/// Internal progression state of [`MaildirMessageCopy`].
#[derive(Clone, Debug, Default)]
pub enum State {
    Locate(MaildirMessageLocate),
    Copied,
    #[default]
    Invalid,
}

/// Argument fed back to [`MaildirMessageCopy::resume`].
#[derive(Clone, Debug)]
pub enum MaildirMessageCopyArg {
    /// Forwarded to the inner locate coroutine.
    FileExists(BTreeMap<MaildirPath, bool>),

    /// Forwarded to the inner locate coroutine.
    DirRead(BTreeMap<MaildirPath, BTreeSet<MaildirPath>>),

    /// Response to [`MaildirMessageCopyResult::WantsCopy`].
    Copy,
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

    /// Makes the message copy progress.
    pub fn resume(
        &mut self,
        arg: Option<impl Into<MaildirMessageCopyArg>>,
    ) -> MaildirMessageCopyResult {
        match (mem::take(&mut self.state), arg.map(Into::into)) {
            (State::Locate(mut c), arg) => {
                let locate_arg = match arg {
                    None => None,
                    Some(MaildirMessageCopyArg::FileExists(probes)) => {
                        Some(MaildirMessageLocateArg::FileExists(probes))
                    }
                    Some(MaildirMessageCopyArg::DirRead(entries)) => {
                        Some(MaildirMessageLocateArg::DirRead(entries))
                    }
                    Some(other) => {
                        let state = State::Locate(c);
                        let err = MaildirMessageCopyError::Invalid(Some(other), state);
                        return MaildirMessageCopyResult::Err(err);
                    }
                };

                match c.resume(locate_arg) {
                    MaildirMessageLocateResult::Ok { path, subdir, .. } => {
                        trace!("located source at {path}");

                        let target_subdir = self.target_subdir.clone().unwrap_or(subdir);
                        let target = build_target_path(&self.target, &target_subdir, &self.id);

                        let pairs = vec![(path, target)];
                        self.state = State::Copied;
                        MaildirMessageCopyResult::WantsCopy(pairs)
                    }
                    MaildirMessageLocateResult::WantsFileExists(probes) => {
                        self.state = State::Locate(c);
                        MaildirMessageCopyResult::WantsFileExists(probes)
                    }
                    MaildirMessageLocateResult::WantsDirRead(paths) => {
                        self.state = State::Locate(c);
                        MaildirMessageCopyResult::WantsDirRead(paths)
                    }
                    MaildirMessageLocateResult::Err(err) => {
                        MaildirMessageCopyResult::Err(err.into())
                    }
                }
            }
            (State::Copied, Some(MaildirMessageCopyArg::Copy)) => {
                trace!("copied source to target");
                MaildirMessageCopyResult::Ok
            }
            (state, arg) => {
                let err = MaildirMessageCopyError::Invalid(arg, state);
                MaildirMessageCopyResult::Err(err)
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
