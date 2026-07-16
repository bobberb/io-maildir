//! I/O-free coroutine locating a Maildir entry by its ID.
//!
//! # Example
//!
//! ```rust,no_run
//! use io_maildir::{
//!     client::MaildirClient,
//!     entry::locate::{MaildirEntryLocate, MaildirEntryLocateOutput},
//! };
//!
//! let client = MaildirClient::new("/path/to/root");
//! let maildir = client.load_maildir("inbox").unwrap();
//!
//! let coroutine = MaildirEntryLocate::new(maildir, "1700000000.1.M0P1.host");
//! let MaildirEntryLocateOutput { path, subdir, flags } = client.run(coroutine).unwrap();
//!
//! println!("found {path} in /{subdir} with flags `{flags}`");
//! ```

use core::{fmt, mem};

use alloc::{
    collections::BTreeSet,
    string::{String, ToString},
};

use log::{debug, trace};
use thiserror::Error;

use crate::{
    coroutine::*,
    flag::MaildirFlags,
    maildir::{Maildir, MaildirSubdir},
    path::MaildirFsPath,
};

/// Failure causes during a [`MaildirEntryLocate`] step.
#[derive(Clone, Debug, Error)]
pub enum MaildirEntryLocateError {
    /// A reply arrived that does not match the awaited step.
    #[error("Maildir message locate failed: unexpected arg {0:?}")]
    UnexpectedArg(Option<MaildirReply>),
    /// No entry with the given ID exists in the Maildir.
    #[error("Maildir message locate failed: message {0} not found")]
    NotFound(String),
}

/// Successful output of [`MaildirEntryLocate`].
#[derive(Clone, Debug)]
pub struct MaildirEntryLocateOutput {
    /// The filesystem path of the located entry.
    pub path: MaildirFsPath,
    /// The subdir the entry was found in.
    pub subdir: MaildirSubdir,
    /// The flags parsed from the entry file name.
    pub flags: MaildirFlags,
}

/// Locates a Maildir entry file by ID: probes `/new` and `/tmp`,
/// then scans `/cur`.
#[derive(Clone, Debug)]
pub struct MaildirEntryLocate {
    maildir: Maildir,
    id: String,
    state: State,
}

impl MaildirEntryLocate {
    /// Builds a coroutine locating the Maildir entry with the given ID.
    pub fn new(maildir: Maildir, id: impl ToString) -> Self {
        Self {
            maildir,
            id: id.to_string(),
            state: State::Start,
        }
    }
}

impl MaildirCoroutine for MaildirEntryLocate {
    type Yield = MaildirYield;
    type Return = Result<MaildirEntryLocateOutput, MaildirEntryLocateError>;

    fn resume(
        &mut self,
        arg: Option<MaildirReply>,
    ) -> MaildirCoroutineState<Self::Yield, Self::Return> {
        match (&mut self.state, arg) {
            (State::Start, None) => {
                let new_path = self.maildir.new().join(&self.id);
                let tmp_path = self.maildir.tmp().join(&self.id);
                let probes = BTreeSet::from_iter([new_path.clone(), tmp_path.clone()]);
                self.state = State::AwaitProbe { new_path, tmp_path };
                MaildirCoroutineState::Yielded(MaildirYield::WantsFileExists(probes))
            }
            (State::AwaitProbe { new_path, tmp_path }, Some(MaildirReply::FileExists(probes))) => {
                if probes.get(new_path).copied().unwrap_or(false) {
                    let out = MaildirEntryLocateOutput {
                        path: mem::take(new_path),
                        subdir: MaildirSubdir::New,
                        flags: MaildirFlags::default(),
                    };
                    debug!("located entry");
                    trace!("path: {}", out.path);
                    return MaildirCoroutineState::Complete(Ok(out));
                }

                if probes.get(tmp_path).copied().unwrap_or(false) {
                    let out = MaildirEntryLocateOutput {
                        path: mem::take(tmp_path),
                        subdir: MaildirSubdir::Tmp,
                        flags: MaildirFlags::default(),
                    };
                    debug!("located entry");
                    trace!("path: {}", out.path);
                    return MaildirCoroutineState::Complete(Ok(out));
                }

                let paths = BTreeSet::from_iter([self.maildir.cur()]);
                self.state = State::AwaitScan;
                MaildirCoroutineState::Yielded(MaildirYield::WantsDirRead(paths))
            }
            (State::AwaitScan, Some(MaildirReply::DirRead(entries))) => {
                let paths = entries.into_values().next().unwrap_or_default();

                for path in paths {
                    let Some(name) = path.file_name() else {
                        continue;
                    };

                    if !name.starts_with(&self.id) {
                        continue;
                    }

                    let flags = MaildirFlags::from(&path);
                    let out = MaildirEntryLocateOutput {
                        path,
                        subdir: MaildirSubdir::Cur,
                        flags,
                    };
                    debug!("located entry");
                    trace!("path: {}", out.path);
                    return MaildirCoroutineState::Complete(Ok(out));
                }

                let err = MaildirEntryLocateError::NotFound(self.id.clone());
                MaildirCoroutineState::Complete(Err(err))
            }
            (_, arg) => {
                let err = MaildirEntryLocateError::UnexpectedArg(arg);
                MaildirCoroutineState::Complete(Err(err))
            }
        }
    }
}

#[derive(Clone, Debug)]
enum State {
    Start,
    AwaitProbe {
        new_path: MaildirFsPath,
        tmp_path: MaildirFsPath,
    },
    AwaitScan,
}

impl fmt::Display for State {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Start => f.write_str("start"),
            Self::AwaitProbe { .. } => f.write_str("await probe reply"),
            Self::AwaitScan => f.write_str("await scan reply"),
        }
    }
}

#[cfg(test)]
mod tests {
    use alloc::collections::BTreeMap;

    use crate::entry::locate::*;

    fn maildir() -> Maildir {
        Maildir::from_path("root")
    }

    #[test]
    fn found_in_new_returns_ok() {
        let mut cor = MaildirEntryLocate::new(maildir(), "abc");

        let probes = expect_wants_file_exists(&mut cor);
        let new_path = MaildirFsPath::from("root/new/abc");
        let tmp_path = MaildirFsPath::from("root/tmp/abc");
        assert!(probes.contains(&new_path));
        assert!(probes.contains(&tmp_path));

        let mut map = BTreeMap::new();
        map.insert(new_path.clone(), true);
        map.insert(tmp_path, false);
        let out = expect_complete_ok(&mut cor, Some(MaildirReply::FileExists(map)));
        assert_eq!(out.path, new_path);
        assert_eq!(out.subdir, MaildirSubdir::New);
    }

    #[test]
    fn not_found_returns_error() {
        let mut cor = MaildirEntryLocate::new(maildir(), "abc");

        expect_wants_file_exists(&mut cor);

        let mut probe = BTreeMap::new();
        probe.insert(MaildirFsPath::from("root/new/abc"), false);
        probe.insert(MaildirFsPath::from("root/tmp/abc"), false);
        match cor.resume(Some(MaildirReply::FileExists(probe))) {
            MaildirCoroutineState::Yielded(MaildirYield::WantsDirRead(paths)) => {
                assert!(paths.contains(&MaildirFsPath::from("root/cur")));
            }
            state => panic!("expected WantsDirRead, got {state:?}"),
        }

        let mut entries = BTreeMap::new();
        entries.insert(MaildirFsPath::from("root/cur"), BTreeSet::new());
        let err = expect_complete_err(&mut cor, Some(MaildirReply::DirRead(entries)));
        assert!(matches!(err, MaildirEntryLocateError::NotFound(_)));
    }

    #[test]
    fn unexpected_reply_returns_error() {
        let mut cor = MaildirEntryLocate::new(maildir(), "abc");
        expect_wants_file_exists(&mut cor);

        let err = expect_complete_err(&mut cor, Some(MaildirReply::DirCreate));
        assert!(matches!(err, MaildirEntryLocateError::UnexpectedArg(_)));
    }

    fn expect_wants_file_exists(cor: &mut MaildirEntryLocate) -> BTreeSet<MaildirFsPath> {
        match cor.resume(None) {
            MaildirCoroutineState::Yielded(MaildirYield::WantsFileExists(paths)) => paths,
            state => panic!("expected WantsFileExists, got {state:?}"),
        }
    }

    fn expect_complete_ok(
        cor: &mut MaildirEntryLocate,
        arg: Option<MaildirReply>,
    ) -> MaildirEntryLocateOutput {
        match cor.resume(arg) {
            MaildirCoroutineState::Complete(Ok(out)) => out,
            state => panic!("expected Complete(Ok), got {state:?}"),
        }
    }

    fn expect_complete_err(
        cor: &mut MaildirEntryLocate,
        arg: Option<MaildirReply>,
    ) -> MaildirEntryLocateError {
        match cor.resume(arg) {
            MaildirCoroutineState::Complete(Err(err)) => err,
            state => panic!("expected Complete(Err), got {state:?}"),
        }
    }
}
