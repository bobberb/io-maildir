//! I/O-free coroutine to store a message in a Maildir.

use core::{
    mem,
    sync::atomic::{AtomicU32, Ordering},
};

use alloc::{
    collections::BTreeMap,
    string::{String, ToString},
    vec::Vec,
};

use log::trace;
use thiserror::Error;

use crate::{
    flag::MaildirFlags,
    maildir::{Maildir, MaildirSubdir},
    message::INFORMATIONAL_SUFFIX_SEPARATOR,
    path::MaildirPath,
};

static COUNTER: AtomicU32 = AtomicU32::new(0);

/// Errors that can occur during the coroutine progression.
#[derive(Clone, Debug, Error)]
pub enum MaildirMessageStoreError {
    #[error("invalid Maildir message store arg {0:?} for state {1:?}")]
    Invalid(Option<MaildirMessageStoreArg>, State),
}

/// Result returned by [`MaildirMessageStore::resume`].
#[derive(Clone, Debug)]
pub enum MaildirMessageStoreResult {
    /// The coroutine has successfully terminated its progression.
    Ok { id: String, path: MaildirPath },

    /// The caller must supply the current time as `(secs, nanos)`
    /// since the Unix epoch and feed back
    /// [`MaildirMessageStoreArg::Time`].
    WantsTime,

    /// The caller must supply the current process id and feed back
    /// [`MaildirMessageStoreArg::Pid`].
    WantsPid,

    /// The caller must supply the host name (or any caller-defined
    /// uniqueness suffix) and feed back
    /// [`MaildirMessageStoreArg::Hostname`].
    WantsHostname,

    /// The caller must write the given files and feed back
    /// [`MaildirMessageStoreArg::FileCreate`].
    WantsFileCreate(BTreeMap<MaildirPath, Vec<u8>>),

    /// The caller must rename each `(from, to)` pair and feed back
    /// [`MaildirMessageStoreArg::Rename`].
    WantsRename(Vec<(MaildirPath, MaildirPath)>),

    /// The coroutine encountered an error.
    Err(MaildirMessageStoreError),
}

/// Internal progression state of [`MaildirMessageStore`].
#[derive(Clone, Debug, Default)]
pub enum State {
    Start {
        maildir: Maildir,
        subdir: MaildirSubdir,
        flags: MaildirFlags,
        contents: Vec<u8>,
    },
    AwaitingTime {
        maildir: Maildir,
        subdir: MaildirSubdir,
        flags: MaildirFlags,
        contents: Vec<u8>,
    },
    AwaitingPid {
        maildir: Maildir,
        subdir: MaildirSubdir,
        flags: MaildirFlags,
        contents: Vec<u8>,
        secs: u64,
        nanos: u32,
    },
    AwaitingHostname {
        maildir: Maildir,
        subdir: MaildirSubdir,
        flags: MaildirFlags,
        contents: Vec<u8>,
        secs: u64,
        nanos: u32,
        pid: u32,
    },
    Created {
        tmp_path: MaildirPath,
        final_path: MaildirPath,
        id: String,
    },
    Renamed {
        final_path: MaildirPath,
        id: String,
    },
    #[default]
    Invalid,
}

/// Argument fed back to [`MaildirMessageStore::resume`].
#[derive(Clone, Debug)]
pub enum MaildirMessageStoreArg {
    /// Response to [`MaildirMessageStoreResult::WantsTime`].
    Time { secs: u64, nanos: u32 },

    /// Response to [`MaildirMessageStoreResult::WantsPid`].
    Pid(u32),

    /// Response to [`MaildirMessageStoreResult::WantsHostname`].
    Hostname(String),

    /// Response to [`MaildirMessageStoreResult::WantsFileCreate`].
    FileCreate,

    /// Response to [`MaildirMessageStoreResult::WantsRename`].
    Rename,
}

/// I/O-free coroutine to store a message in a Maildir.
///
/// Follows the Maildir delivery protocol: write to `/tmp` first,
/// then atomically rename under the target subdir (`/new` or
/// `/cur`).
#[derive(Debug)]
pub struct MaildirMessageStore {
    state: State,
}

impl MaildirMessageStore {
    /// Creates a new coroutine that will store `contents` as a new
    /// message in `maildir` under `subdir` with the given `flags`.
    pub fn new(
        maildir: Maildir,
        subdir: MaildirSubdir,
        flags: MaildirFlags,
        contents: Vec<u8>,
    ) -> Self {
        Self {
            state: State::Start {
                maildir,
                subdir,
                flags,
                contents,
            },
        }
    }

    /// Makes the message store progress.
    pub fn resume(
        &mut self,
        arg: Option<impl Into<MaildirMessageStoreArg>>,
    ) -> MaildirMessageStoreResult {
        match (mem::take(&mut self.state), arg.map(Into::into)) {
            (
                State::Start {
                    maildir,
                    subdir,
                    flags,
                    contents,
                },
                None,
            ) => {
                trace!("wants time");
                self.state = State::AwaitingTime {
                    maildir,
                    subdir,
                    flags,
                    contents,
                };
                MaildirMessageStoreResult::WantsTime
            }
            (
                State::AwaitingTime {
                    maildir,
                    subdir,
                    flags,
                    contents,
                },
                Some(MaildirMessageStoreArg::Time { secs, nanos }),
            ) => {
                trace!("wants pid");
                self.state = State::AwaitingPid {
                    maildir,
                    subdir,
                    flags,
                    contents,
                    secs,
                    nanos,
                };
                MaildirMessageStoreResult::WantsPid
            }
            (
                State::AwaitingPid {
                    maildir,
                    subdir,
                    flags,
                    contents,
                    secs,
                    nanos,
                },
                Some(MaildirMessageStoreArg::Pid(pid)),
            ) => {
                trace!("wants hostname");
                self.state = State::AwaitingHostname {
                    maildir,
                    subdir,
                    flags,
                    contents,
                    secs,
                    nanos,
                    pid,
                };
                MaildirMessageStoreResult::WantsHostname
            }
            (
                State::AwaitingHostname {
                    maildir,
                    subdir,
                    flags,
                    contents,
                    secs,
                    nanos,
                    pid,
                },
                Some(MaildirMessageStoreArg::Hostname(hostname)),
            ) => {
                let counter = COUNTER.fetch_add(1, Ordering::AcqRel);
                let id = format!("{secs}.#{counter:x}M{nanos}P{pid}.{hostname}");

                let mut final_name = id.clone();
                if let MaildirSubdir::Cur = subdir {
                    final_name.push(INFORMATIONAL_SUFFIX_SEPARATOR);
                    final_name.push_str("2,");
                    final_name.push_str(&flags.to_string());
                }

                let tmp_path = maildir.tmp().join(&id);
                let final_path = maildir.subdir(&subdir).join(&final_name);

                trace!("wants tmp file create at {tmp_path}");

                let files = BTreeMap::from_iter([(tmp_path.clone(), contents)]);
                self.state = State::Created {
                    tmp_path,
                    final_path,
                    id,
                };
                MaildirMessageStoreResult::WantsFileCreate(files)
            }
            (
                State::Created {
                    tmp_path,
                    final_path,
                    id,
                },
                Some(MaildirMessageStoreArg::FileCreate),
            ) => {
                trace!("created tmp file, wants rename to {final_path}");

                let pairs = vec![(tmp_path, final_path.clone())];
                self.state = State::Renamed { final_path, id };
                MaildirMessageStoreResult::WantsRename(pairs)
            }
            (State::Renamed { final_path, id }, Some(MaildirMessageStoreArg::Rename)) => {
                trace!("renamed tmp file to {final_path}");

                MaildirMessageStoreResult::Ok {
                    id,
                    path: final_path,
                }
            }
            (state, arg) => {
                let err = MaildirMessageStoreError::Invalid(arg, state);
                MaildirMessageStoreResult::Err(err)
            }
        }
    }
}
