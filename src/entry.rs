//! Single Maildir message entry: the on-disk filename, with helpers
//! to extract the id and flags it encodes.

use crate::{flag::MaildirFlags, message::INFORMATIONAL_SUFFIX_SEPARATOR, path::MaildirPath};

/// A lightweight handle to a single Maildir message file.
///
/// Maildir encodes the message id and flags directly in the filename:
/// no body is loaded by [`MaildirClient::list_entries`]. Use
/// [`MaildirClient::read_entry`] (or the bulk variants) to load the
/// contents.
///
/// [`MaildirClient::list_entries`]: crate::client::MaildirClient::list_entries
/// [`MaildirClient::read_entry`]: crate::client::MaildirClient::read_entry
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct MaildirEntry {
    path: MaildirPath,
}

impl MaildirEntry {
    /// Wraps `path` as an entry. Performs no validation.
    pub fn from_path(path: impl Into<MaildirPath>) -> Self {
        Self { path: path.into() }
    }

    /// Returns the on-disk path of the message file.
    pub fn path(&self) -> &MaildirPath {
        &self.path
    }

    /// Returns the message id (the filename portion before the
    /// `:2,` flags separator).
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

impl From<MaildirPath> for MaildirEntry {
    fn from(path: MaildirPath) -> Self {
        Self::from_path(path)
    }
}

impl From<MaildirEntry> for MaildirPath {
    fn from(entry: MaildirEntry) -> Self {
        entry.path
    }
}
