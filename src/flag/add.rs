//! I/O-free coroutine adding flags to a Maildir entry.
//!
//! # Example
//!
//! ```rust,no_run
//! use io_maildir::{
//!     client::MaildirClient,
//!     flag::{add::MaildirFlagsAdd, MaildirFlag, MaildirFlags},
//! };
//!
//! let client = MaildirClient::new("/path/to/root");
//! let maildir = client.load_maildir("inbox").unwrap();
//!
//! let flags = MaildirFlags::from_iter([MaildirFlag::Seen]);
//! let coroutine = MaildirFlagsAdd::new(maildir, "1700000000.1.M0P1.host", flags);
//! client.run(coroutine).unwrap();
//! ```

use core::fmt;

use alloc::string::{String, ToString};

use log::debug;
use thiserror::Error;

use crate::{
    coroutine::*,
    entry::INFORMATIONAL_SUFFIX_SEPARATOR,
    entry::locate::*,
    flag::MaildirFlags,
    maildir::{Maildir, MaildirSubdir},
    maildir_try,
    path::MaildirFsPath,
};

/// Failure causes during a [`MaildirFlagsAdd`] step.
#[derive(Clone, Debug, Error)]
pub enum MaildirFlagsAddError {
    /// A reply arrived that does not match the awaited step.
    #[error("Maildir flags add failed: unexpected arg {0:?}")]
    UnexpectedArg(Option<MaildirReply>),
    /// The inner locate step failed.
    #[error(transparent)]
    Locate(#[from] MaildirEntryLocateError),
}

/// Adds flags to a Maildir entry. No-op on `/new` and `/tmp`.
#[derive(Debug)]
pub struct MaildirFlagsAdd {
    id: String,
    flags: MaildirFlags,
    state: State,
}

impl MaildirFlagsAdd {
    /// Builds a coroutine adding flags to a Maildir entry.
    pub fn new(maildir: Maildir, id: impl ToString, flags: MaildirFlags) -> Self {
        let id = id.to_string();
        Self {
            state: State::Locate(MaildirEntryLocate::new(maildir, &id)),
            id,
            flags,
        }
    }
}

impl MaildirCoroutine for MaildirFlagsAdd {
    type Yield = MaildirYield;
    type Return = Result<(), MaildirFlagsAddError>;

    fn resume(
        &mut self,
        arg: Option<MaildirReply>,
    ) -> MaildirCoroutineState<Self::Yield, Self::Return> {
        match (&mut self.state, arg) {
            (State::Locate(c), arg) => {
                let out = maildir_try!(c, arg);

                match out.subdir {
                    MaildirSubdir::New | MaildirSubdir::Tmp => {
                        debug!("added flags");
                        MaildirCoroutineState::Complete(Ok(()))
                    }
                    MaildirSubdir::Cur => {
                        let mut existing = out.flags;
                        existing.extend(self.flags.clone());
                        let new_path = rename_with_flags(&out.path, &self.id, &existing);
                        let pairs = vec![(out.path, new_path)];
                        self.state = State::AwaitRename;
                        MaildirCoroutineState::Yielded(MaildirYield::WantsRename(pairs))
                    }
                }
            }
            (State::AwaitRename, Some(MaildirReply::Rename)) => {
                debug!("added flags");
                MaildirCoroutineState::Complete(Ok(()))
            }
            (_, arg) => {
                let err = MaildirFlagsAddError::UnexpectedArg(arg);
                MaildirCoroutineState::Complete(Err(err))
            }
        }
    }
}

#[derive(Debug)]
enum State {
    Locate(MaildirEntryLocate),
    AwaitRename,
}

impl fmt::Display for State {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Locate(_) => f.write_str("locate message"),
            Self::AwaitRename => f.write_str("await rename reply"),
        }
    }
}

fn rename_with_flags(path: &MaildirFsPath, id: &str, flags: &MaildirFlags) -> MaildirFsPath {
    let mut name = String::from(id);
    name.push(INFORMATIONAL_SUFFIX_SEPARATOR);
    name.push_str("2,");
    name.push_str(&flags.to_string());
    path.with_file_name(&name)
}

#[cfg(test)]
mod tests {
    use alloc::collections::BTreeMap;

    use crate::flag::add::*;

    fn maildir() -> Maildir {
        Maildir::from_path("root")
    }

    #[test]
    fn new_subdir_returns_noop_ok() {
        let mut cor = MaildirFlagsAdd::new(maildir(), "abc", MaildirFlags::default());

        expect_wants_file_exists(&mut cor);

        let mut probes = BTreeMap::new();
        probes.insert(MaildirFsPath::from("root/new/abc"), true);
        probes.insert(MaildirFsPath::from("root/tmp/abc"), false);
        expect_complete_ok(&mut cor, Some(MaildirReply::FileExists(probes)));
    }

    #[test]
    fn unexpected_reply_returns_error() {
        let mut cor = MaildirFlagsAdd::new(maildir(), "abc", MaildirFlags::default());
        expect_wants_file_exists(&mut cor);

        let err = expect_complete_err(&mut cor, Some(MaildirReply::DirCreate));
        assert!(matches!(err, MaildirFlagsAddError::Locate(_)));
    }

    fn expect_wants_file_exists(cor: &mut MaildirFlagsAdd) {
        match cor.resume(None) {
            MaildirCoroutineState::Yielded(MaildirYield::WantsFileExists(_)) => {}
            state => panic!("expected WantsFileExists, got {state:?}"),
        }
    }

    fn expect_complete_ok(cor: &mut MaildirFlagsAdd, arg: Option<MaildirReply>) {
        match cor.resume(arg) {
            MaildirCoroutineState::Complete(Ok(())) => {}
            state => panic!("expected Complete(Ok), got {state:?}"),
        }
    }

    fn expect_complete_err(
        cor: &mut MaildirFlagsAdd,
        arg: Option<MaildirReply>,
    ) -> MaildirFlagsAddError {
        match cor.resume(arg) {
            MaildirCoroutineState::Complete(Err(err)) => err,
            state => panic!("expected Complete(Err), got {state:?}"),
        }
    }
}
