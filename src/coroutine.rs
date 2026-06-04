//! # Generator-shape coroutine driver
//!
//! Mirrors the shape of `core::ops::Coroutine`: a `Yield` associated type for
//! intermediate progress, a `Return` associated type for terminal output, and a
//! two-variant [`MaildirCoroutineState`] (`Yielded` / `Complete`).
//!
//! io-maildir is filesystem-flavoured: every coroutine in this crate picks
//! [`MaildirYield`] as its `Yield`, mixing filesystem `Wants*` requests with
//! environmental inputs (time / pid / hostname) needed to mint Maildir message
//! identifiers. The terminal `Return` is always a per-coroutine `Result<Output,
//! Error>`.
//!
//! [`MaildirClient::run`] drives any standard-Yield coroutine to completion
//! against the local filesystem.
//!
//! [`MaildirClient::run`]: crate::client::MaildirClient::run

use alloc::{
    collections::{BTreeMap, BTreeSet},
    string::String,
    vec::Vec,
};

use crate::path::FsPath;

/// State yielded by a [`MaildirCoroutine::resume`] step.
///
/// Two-variant by design (matches std's `core::ops::CoroutineState`): any
/// further variation lives inside the per-coroutine `Yield` type.
#[derive(Debug)]
pub enum MaildirCoroutineState<Y, R> {
    /// Intermediate yield. The driver reacts to `Y` (perform the requested
    /// filesystem / environment op, feed the answer back) and resumes the
    /// coroutine again.
    Yielded(Y),
    /// Terminal yield. By convention `R = Result<Output, Error>`.
    Complete(R),
}

/// Standard-shape Maildir coroutine.
///
/// Implementors own their internal state machine and declare their per-step
/// `Yield` plus a terminal `Return`. The driver reacts to each `Yield` variant
/// (run a filesystem op, sample the clock, look up the hostname) and resumes
/// until `Complete`.
pub trait MaildirCoroutine {
    /// Intermediate value handed back on every step. Per-coroutine: each
    /// implementor picks exactly the variants it needs.
    type Yield;
    /// Terminal value. By convention `Result<Output, Error>`; the "ok" arm
    /// carries the operation's final output, the "error" arm carries the cause.
    type Return;

    /// Advances the coroutine one step.
    ///
    /// Pass [`None`] on the initial call or after a yield that does not request
    /// data (e.g. [`MaildirYield::WantsFileCreate`]). Pass `Some(reply)`
    /// carrying the answer to the previous yield (e.g.  the existence map for
    /// [`MaildirYield::WantsFileExists`]).
    fn resume(
        &mut self,
        arg: Option<MaildirReply>,
    ) -> MaildirCoroutineState<Self::Yield, Self::Return>;
}

/// Standard I/O Yield used by every io-maildir coroutine.
///
/// Mixes filesystem step requests with the three environmental inputs the
/// Maildir delivery protocol needs to mint message identifiers (time, pid,
/// hostname).
#[derive(Debug)]
pub enum MaildirYield {
    /// Driver must check each path for existence as a regular file and resume
    /// with [`MaildirReply::FileExists`].
    WantsFileExists(BTreeSet<FsPath>),

    /// Driver must check each path for existence as a directory and resume with
    /// [`MaildirReply::DirExists`].
    WantsDirExists(BTreeSet<FsPath>),

    /// Driver must list each directory's entries and resume with
    /// [`MaildirReply::DirRead`].
    WantsDirRead(BTreeSet<FsPath>),

    /// Driver must read each file's bytes and resume with
    /// [`MaildirReply::FileRead`].
    WantsFileRead(BTreeSet<FsPath>),

    /// Driver must write each `(path, bytes)` pair and resume with
    /// [`MaildirReply::FileCreate`].
    WantsFileCreate(BTreeMap<FsPath, Vec<u8>>),

    /// Driver must create each directory (with parents) and resume with
    /// [`MaildirReply::DirCreate`].
    WantsDirCreate(BTreeSet<FsPath>),

    /// Driver must recursively remove each directory and resume with
    /// [`MaildirReply::DirRemove`].
    WantsDirRemove(BTreeSet<FsPath>),

    /// Driver must rename each `(from, to)` pair and resume with
    /// [`MaildirReply::Rename`].
    WantsRename(Vec<(FsPath, FsPath)>),

    /// Driver must copy each `(from, to)` pair and resume with
    /// [`MaildirReply::Copy`].
    WantsCopy(Vec<(FsPath, FsPath)>),

    /// Driver must supply the current Unix time and resume with
    /// [`MaildirReply::Time`].
    WantsTime,

    /// Driver must supply the current process id and resume with
    /// [`MaildirReply::Pid`].
    WantsPid,

    /// Driver must supply the host name and resume with
    /// [`MaildirReply::Hostname`].
    WantsHostname,
}

/// Reply fed back into [`MaildirCoroutine::resume`] by the driver.
///
/// One variant per [`MaildirYield`] request; the coroutine asserts the variant
/// it expects and ignores the rest.
#[derive(Clone, Debug)]
pub enum MaildirReply {
    /// Answer to [`MaildirYield::WantsFileExists`].
    FileExists(BTreeMap<FsPath, bool>),

    /// Answer to [`MaildirYield::WantsDirExists`].
    DirExists(BTreeMap<FsPath, bool>),

    /// Answer to [`MaildirYield::WantsDirRead`].
    DirRead(BTreeMap<FsPath, BTreeSet<FsPath>>),

    /// Answer to [`MaildirYield::WantsFileRead`].
    FileRead(BTreeMap<FsPath, Vec<u8>>),

    /// Acknowledgement of [`MaildirYield::WantsFileCreate`].
    FileCreate,

    /// Acknowledgement of [`MaildirYield::WantsDirCreate`].
    DirCreate,

    /// Acknowledgement of [`MaildirYield::WantsDirRemove`].
    DirRemove,

    /// Acknowledgement of [`MaildirYield::WantsRename`].
    Rename,

    /// Acknowledgement of [`MaildirYield::WantsCopy`].
    Copy,

    /// Answer to [`MaildirYield::WantsTime`].
    Time { secs: u64, nanos: u32 },

    /// Answer to [`MaildirYield::WantsPid`].
    Pid(u32),

    /// Answer to [`MaildirYield::WantsHostname`].
    Hostname(String),
}

/// Coroutine `?`: forwards `Yielded` (via `Into`), short-circuits on `Err`,
/// evaluates to the inner `Ok` value.
#[macro_export]
macro_rules! maildir_try {
    ($coroutine:expr, $arg:expr $(,)?) => {
        match $crate::coroutine::MaildirCoroutine::resume($coroutine, $arg) {
            $crate::coroutine::MaildirCoroutineState::Yielded(y) => {
                return $crate::coroutine::MaildirCoroutineState::Yielded(y.into());
            }
            $crate::coroutine::MaildirCoroutineState::Complete(Err(err)) => {
                return $crate::coroutine::MaildirCoroutineState::Complete(Err(err.into()));
            }
            $crate::coroutine::MaildirCoroutineState::Complete(Ok(value)) => value,
        }
    };
}
