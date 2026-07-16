//! Maildir directory structure: the [`Maildir`] handle over a
//! cur/new/tmp tree, the [`MaildirSubdir`] enum naming its three
//! subdirectories and the [`MaildirError`] validation error.
//!
//! The I/O-free coroutines managing the tree live in the submodules
//! next to this file: [`create`], [`delete`], [`list`] and [`rename`].

pub mod create;
pub mod delete;
pub mod list;
pub mod rename;

use core::{
    fmt,
    hash::{Hash, Hasher},
    str::FromStr,
};

use alloc::string::String;

use thiserror::Error;

use crate::path::MaildirFsPath;

/// Failure causes when validating a Maildir on disk.
#[derive(Clone, Debug, Error)]
pub enum MaildirError {
    /// The resolved path exists but is not a directory.
    #[error("path {0} is not a directory")]
    NotDir(MaildirFsPath),
    /// One of the cur/new/tmp subdirectories is missing.
    #[error("missing {0}/ subdirectory at Maildir {1}")]
    MissingSubdir(&'static str, MaildirFsPath),
    /// A subdirectory name is none of cur, new or tmp.
    #[error("invalid Maildir subdir {0:?}: expected cur, new or tmp")]
    InvalidSubdir(String),
}

/// Name of the `cur` subdirectory, holding entries already seen by a
/// client.
pub const CUR: &str = "cur";
/// Name of the `new` subdirectory, holding freshly delivered entries.
pub const NEW: &str = "new";
/// Name of the `tmp` subdirectory, holding entries mid-delivery.
pub const TMP: &str = "tmp";

/// One of the three Maildir subdirectories: cur, new, tmp.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MaildirSubdir {
    /// The `cur` subdirectory: entries already seen by a client.
    Cur,
    /// The `new` subdirectory: freshly delivered, unseen entries.
    New,
    /// The `tmp` subdirectory: entries still being written.
    Tmp,
}

impl FromStr for MaildirSubdir {
    type Err = MaildirError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            CUR => Ok(Self::Cur),
            NEW => Ok(Self::New),
            TMP => Ok(Self::Tmp),
            _ => Err(MaildirError::InvalidSubdir(s.into())),
        }
    }
}

impl fmt::Display for MaildirSubdir {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Cur => write!(f, "{CUR}"),
            Self::New => write!(f, "{NEW}"),
            Self::Tmp => write!(f, "{TMP}"),
        }
    }
}

/// A Maildir root on the filesystem, holding the cur/new/tmp subdirs.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct Maildir {
    root: MaildirFsPath,
}

impl Maildir {
    /// Wraps `root` without checking that the subdirectories exist.
    pub fn from_path(root: impl Into<MaildirFsPath>) -> Self {
        Self { root: root.into() }
    }

    /// Returns the filesystem path of the Maildir root.
    pub fn path(&self) -> &MaildirFsPath {
        &self.root
    }

    /// Returns the final component of the root path, if any.
    pub fn name(&self) -> Option<&str> {
        self.root.file_name()
    }

    /// Returns the path of the given subdirectory under this Maildir.
    pub fn subdir(&self, subdir: &MaildirSubdir) -> MaildirFsPath {
        match subdir {
            MaildirSubdir::Cur => self.cur(),
            MaildirSubdir::New => self.new(),
            MaildirSubdir::Tmp => self.tmp(),
        }
    }

    /// Returns the path of the `cur` subdirectory.
    pub fn cur(&self) -> MaildirFsPath {
        self.root.join(CUR)
    }

    /// Returns the path of the `new` subdirectory.
    // NOTE: `new` names the Maildir subdirectory, not a constructor, so
    // returning a path rather than Self is intended.
    #[allow(clippy::new_ret_no_self)]
    pub fn new(&self) -> MaildirFsPath {
        self.root.join(NEW)
    }

    /// Returns the path of the `tmp` subdirectory.
    pub fn tmp(&self) -> MaildirFsPath {
        self.root.join(TMP)
    }
}

impl Hash for Maildir {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.root.hash(state);
    }
}

impl AsRef<MaildirFsPath> for Maildir {
    fn as_ref(&self) -> &MaildirFsPath {
        &self.root
    }
}

impl From<MaildirFsPath> for Maildir {
    fn from(root: MaildirFsPath) -> Self {
        Self { root }
    }
}

impl From<String> for Maildir {
    fn from(root: String) -> Self {
        Self { root: root.into() }
    }
}

impl From<&str> for Maildir {
    fn from(root: &str) -> Self {
        Self { root: root.into() }
    }
}
