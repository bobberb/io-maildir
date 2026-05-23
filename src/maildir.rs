//! Maildir directory structure.

use core::{
    fmt,
    hash::{Hash, Hasher},
    str::FromStr,
};

use alloc::string::String;

use thiserror::Error;

use crate::path::MaildirPath;

#[derive(Clone, Debug, Error)]
pub enum MaildirError {
    /// The given path is not a directory.
    #[error("path {0} is not a directory")]
    NotDir(MaildirPath),

    /// The directory does not look like a Maildir (missing `cur`,
    /// `new`, or `tmp` subdirectory).
    #[error("missing {0}/ subdirectory at Maildir {1}")]
    MissingSubdir(&'static str, MaildirPath),

    /// The name does not match `cur`, `new`, or `tmp`.
    #[error("invalid Maildir subdir {0:?}: expected cur, new or tmp")]
    InvalidSubdir(String),
}

pub const CUR: &str = "cur";
pub const NEW: &str = "new";
pub const TMP: &str = "tmp";

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MaildirSubdir {
    Cur,
    New,
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

/// A Maildir on the filesystem.
///
/// Represents a directory with the standard `cur`, `new`, and `tmp`
/// subdirectories. Use [`MaildirCreate`] to initialise one and the
/// client `load_maildir` helper to open an existing one.
///
/// [`MaildirCreate`]: crate::coroutines::maildir_create::MaildirCreate
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct Maildir {
    root: MaildirPath,
}

impl Maildir {
    /// Builds a [`Maildir`] from `root` without checking the
    /// subdirectories exist.
    pub fn from_path(root: impl Into<MaildirPath>) -> Self {
        Self { root: root.into() }
    }

    pub fn path(&self) -> &MaildirPath {
        &self.root
    }

    pub fn name(&self) -> Option<&str> {
        self.root.file_name()
    }

    pub fn subdir(&self, subdir: &MaildirSubdir) -> MaildirPath {
        match subdir {
            MaildirSubdir::Cur => self.cur(),
            MaildirSubdir::New => self.new(),
            MaildirSubdir::Tmp => self.tmp(),
        }
    }

    pub fn cur(&self) -> MaildirPath {
        self.root.join(CUR)
    }

    pub fn new(&self) -> MaildirPath {
        self.root.join(NEW)
    }

    pub fn tmp(&self) -> MaildirPath {
        self.root.join(TMP)
    }
}

impl Hash for Maildir {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.root.hash(state);
    }
}

impl AsRef<MaildirPath> for Maildir {
    fn as_ref(&self) -> &MaildirPath {
        &self.root
    }
}

impl From<MaildirPath> for Maildir {
    fn from(root: MaildirPath) -> Self {
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
