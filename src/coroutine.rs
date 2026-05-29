//! # Generic coroutine driver
//!
//! Every standard-shape coroutine in this crate exposes the same loop
//! contract: report progress through [`MaildirCoroutineState`] and
//! receive replies as its associated [`Arg`]. The
//! [`MaildirCoroutine`] trait unifies that contract behind a single
//! method so a generic driver ([`MaildirClient::run`]) can advance
//! any coroutine without macros.
//!
//! io-maildir is filesystem-flavoured, so the state enum carries
//! `Wants*` variants for each filesystem (or environmental) primitive
//! the crate emits: directory create / remove / read / exists, file
//! create / read / exists, path rename / copy, and the
//! time / process id / hostname inputs needed to mint Maildir message
//! identifiers. Per-coroutine `Arg` enums only declare the subset of
//! replies that coroutine actually consumes.
//!
//! [`MaildirClient::run`]: crate::client::MaildirClient::run
//! [`Arg`]: MaildirCoroutine::Arg

use alloc::{
    collections::{BTreeMap, BTreeSet},
    vec::Vec,
};

use crate::path::MaildirPath;

/// State yielded by a [`MaildirCoroutine`] resume.
///
/// Single generic enum so a generic driver can pattern match on
/// progression without naming a per-coroutine `Result` type.
#[derive(Debug)]
pub enum MaildirCoroutineState<T, E> {
    /// Coroutine terminated successfully with this payload.
    Done(T),

    /// Caller must supply the current Unix time and feed back the
    /// coroutine's `Arg::Time` variant.
    WantsTime,

    /// Caller must supply the current process id and feed back the
    /// coroutine's `Arg::Pid` variant.
    WantsPid,

    /// Caller must supply the host name and feed back the
    /// coroutine's `Arg::Hostname` variant.
    WantsHostname,

    /// Caller must check each path for existence as a regular file
    /// and feed back the coroutine's `Arg::FileExists` variant.
    WantsFileExists(BTreeSet<MaildirPath>),

    /// Caller must check each path for existence as a directory and
    /// feed back the coroutine's `Arg::DirExists` variant.
    WantsDirExists(BTreeSet<MaildirPath>),

    /// Caller must list each directory's entries and feed back the
    /// coroutine's `Arg::DirRead` variant.
    WantsDirRead(BTreeSet<MaildirPath>),

    /// Caller must create each directory (with parents) and feed
    /// back the coroutine's `Arg::DirCreate` variant.
    WantsDirCreate(BTreeSet<MaildirPath>),

    /// Caller must recursively remove each directory and feed back
    /// the coroutine's `Arg::DirRemove` variant.
    WantsDirRemove(BTreeSet<MaildirPath>),

    /// Caller must read each file's bytes and feed back the
    /// coroutine's `Arg::FileRead` variant.
    WantsFileRead(BTreeSet<MaildirPath>),

    /// Caller must write each `(path, bytes)` pair and feed back the
    /// coroutine's `Arg::FileCreate` variant.
    WantsFileCreate(BTreeMap<MaildirPath, Vec<u8>>),

    /// Caller must rename each `(from, to)` pair and feed back the
    /// coroutine's `Arg::Rename` variant.
    WantsRename(Vec<(MaildirPath, MaildirPath)>),

    /// Caller must copy each `(from, to)` pair and feed back the
    /// coroutine's `Arg::Copy` variant.
    WantsCopy(Vec<(MaildirPath, MaildirPath)>),

    /// Coroutine terminated with this error.
    Err(E),
}

/// Standard-shape Maildir coroutine: anything whose progression maps
/// onto [`MaildirCoroutineState`].
///
/// `resume` is the single source of truth: each implementor's body
/// returns [`MaildirCoroutineState::Done`] / `Wants*` / `Err`
/// directly. [`MaildirClient::run`] drives any [`MaildirCoroutine`]
/// to completion against the local filesystem; downstream code can
/// write its own driver against the same trait.
///
/// [`MaildirClient::run`]: crate::client::MaildirClient::run
pub trait MaildirCoroutine {
    /// Reply fed back into [`resume`](Self::resume) by the driver.
    type Arg;

    /// Payload yielded on terminal success.
    type Output;

    /// Error yielded on terminal failure.
    type Error;

    /// Advances the coroutine one step.
    fn resume(
        &mut self,
        arg: Option<Self::Arg>,
    ) -> MaildirCoroutineState<Self::Output, Self::Error>;
}
