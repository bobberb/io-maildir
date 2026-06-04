//! Maildir entry types: full body, lightweight handle, and the
//! platform-specific info-section separator.

use core::hash::{Hash, Hasher};

use alloc::vec::Vec;

use crate::{flag::types::MaildirFlags, path::FsPath};

#[cfg(unix)]
pub static INFORMATIONAL_SUFFIX_SEPARATOR: char = ':';
#[cfg(windows)]
pub static INFORMATIONAL_SUFFIX_SEPARATOR: char = ';';

/// A Maildir entry: on-disk path plus body bytes.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct MaildirFullEntry {
    pub(crate) path: FsPath,
    pub(crate) contents: Vec<u8>,
}

impl MaildirFullEntry {
    pub fn path(&self) -> &FsPath {
        &self.path
    }

    pub fn id(&self) -> Option<&str> {
        let file_name = self.path.file_name()?;

        let id = match file_name.rsplit_once(INFORMATIONAL_SUFFIX_SEPARATOR) {
            Some((id, _)) => id,
            None => file_name,
        };

        Some(id)
    }

    pub fn contents(&self) -> &[u8] {
        &self.contents
    }

    #[cfg(feature = "parser")]
    pub fn parsed(&self) -> Option<mail_parser::Message<'_>> {
        mail_parser::MessageParser::new().parse(&self.contents)
    }

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

impl From<(FsPath, Vec<u8>)> for MaildirFullEntry {
    fn from((path, contents): (FsPath, Vec<u8>)) -> Self {
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
    path: FsPath,
}

impl MaildirEntry {
    pub fn from_path(path: impl Into<FsPath>) -> Self {
        Self { path: path.into() }
    }

    pub fn path(&self) -> &FsPath {
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

impl From<FsPath> for MaildirEntry {
    fn from(path: FsPath) -> Self {
        Self::from_path(path)
    }
}

impl From<MaildirEntry> for FsPath {
    fn from(entry: MaildirEntry) -> Self {
        entry.path
    }
}
