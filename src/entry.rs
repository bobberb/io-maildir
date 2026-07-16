//! Maildir entries: the [`MaildirEntry`] lightweight handle, the
//! [`MaildirFullEntry`] body-carrying entry and the platform-specific
//! [`INFORMATIONAL_SUFFIX_SEPARATOR`].
//!
//! The I/O-free coroutines for the delivery protocol and the entry
//! lifecycle (store, get, list, locate, copy, move) live in the
//! submodules next to this file, alongside the RFC 5322 header helpers
//! under [`headers`].

pub mod copy;
pub mod get;
pub mod headers;
pub mod list;
pub mod locate;
pub mod r#move;
pub mod store;

use core::{
    hash::{Hash, Hasher},
    sync::atomic::{AtomicU32, Ordering},
};

use alloc::{string::String, vec::Vec};

use crate::{flag::MaildirFlags, path::MaildirFsPath};

/// Character separating the entry id from its `2,<flags>` info
/// section in a filename.
///
/// A colon on Unix; a semicolon on Windows, where the colon is
/// reserved.
#[cfg(unix)]
pub static INFORMATIONAL_SUFFIX_SEPARATOR: char = ':';
/// Character separating the entry id from its `2,<flags>` info
/// section in a filename.
///
/// A colon on Unix; a semicolon on Windows, where the colon is
/// reserved.
#[cfg(windows)]
pub static INFORMATIONAL_SUFFIX_SEPARATOR: char = ';';

/// Process-wide counter disambiguating ids minted within the same
/// clock tick. Shared by every delivery site (store, copy, move) so
/// two deliveries into the same Maildir can never collide.
static ID_COUNTER: AtomicU32 = AtomicU32::new(0);

/// Mints a fresh Maildir unique name following the delivery
/// convention: `{secs}.#{counter:x}M{nanos}P{pid}.{hostname}`.
///
/// Used by the store, copy and move coroutines so a relocated entry
/// never reuses the source basename, which may carry foreign,
/// folder-specific metadata such as mbsync's `,U=<uid>` infix.
pub(crate) fn mint_id(secs: u64, nanos: u32, pid: u32, hostname: &str) -> String {
    let counter = ID_COUNTER.fetch_add(1, Ordering::AcqRel);
    format!("{secs}.#{counter:x}M{nanos}P{pid}.{hostname}")
}

/// A Maildir entry: on-disk path plus body bytes.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct MaildirFullEntry {
    pub(crate) path: MaildirFsPath,
    pub(crate) contents: Vec<u8>,
}

impl MaildirFullEntry {
    /// Returns the on-disk path of the entry file.
    pub fn path(&self) -> &MaildirFsPath {
        &self.path
    }

    /// Returns the entry id (filename before the info-section
    /// separator).
    pub fn id(&self) -> Option<&str> {
        let file_name = self.path.file_name()?;

        let id = match file_name.rsplit_once(INFORMATIONAL_SUFFIX_SEPARATOR) {
            Some((id, _)) => id,
            None => file_name,
        };

        Some(id)
    }

    /// Returns the raw body bytes.
    pub fn contents(&self) -> &[u8] {
        &self.contents
    }

    /// Parses the full message, headers and body, with mail-parser.
    #[cfg(feature = "parser")]
    pub fn parsed(&self) -> Option<mail_parser::Message<'_>> {
        mail_parser::MessageParser::new().parse(&self.contents)
    }

    /// Parses only the minimal headers with mail-parser.
    #[cfg(feature = "parser")]
    pub fn headers(&self) -> Option<mail_parser::Message<'_>> {
        mail_parser::MessageParser::new()
            .with_minimal_headers()
            .parse(&self.contents)
    }
}

impl From<MaildirFullEntry> for Vec<u8> {
    fn from(msg: MaildirFullEntry) -> Self {
        msg.contents
    }
}

impl From<(MaildirFsPath, Vec<u8>)> for MaildirFullEntry {
    fn from((path, contents): (MaildirFsPath, Vec<u8>)) -> Self {
        Self { path, contents }
    }
}

impl Hash for MaildirFullEntry {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.path.hash(state);
    }
}

/// Lightweight handle to a Maildir entry file (path only, no body).
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct MaildirEntry {
    path: MaildirFsPath,
}

impl MaildirEntry {
    /// Wraps `path` as an entry handle.
    pub fn from_path(path: impl Into<MaildirFsPath>) -> Self {
        Self { path: path.into() }
    }

    /// Returns the on-disk path of the entry file.
    pub fn path(&self) -> &MaildirFsPath {
        &self.path
    }

    /// Returns the entry id (filename before the `:2,` flags
    /// separator).
    pub fn id(&self) -> Option<&str> {
        let file_name = self.path.file_name()?;

        Some(
            match file_name.rsplit_once(INFORMATIONAL_SUFFIX_SEPARATOR) {
                Some((id, _)) => id,
                None => file_name,
            },
        )
    }

    /// Parses the flags encoded in the filename.
    pub fn flags(&self) -> MaildirFlags {
        MaildirFlags::from(&self.path)
    }
}

impl From<MaildirFsPath> for MaildirEntry {
    fn from(path: MaildirFsPath) -> Self {
        Self::from_path(path)
    }
}

impl From<MaildirEntry> for MaildirFsPath {
    fn from(entry: MaildirEntry) -> Self {
        entry.path
    }
}
