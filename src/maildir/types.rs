//! Maildir directory structure.

use core::{
    fmt,
    hash::{Hash, Hasher},
    str::FromStr,
};

use alloc::string::String;

use thiserror::Error;

use crate::path::FsPath;

/// Failure causes when validating a Maildir on disk.
#[derive(Clone, Debug, Error)]
pub enum MaildirError {
    #[error("path {0} is not a directory")]
    NotDir(FsPath),

    #[error("missing {0}/ subdirectory at Maildir {1}")]
    MissingSubdir(&'static str, FsPath),

    #[error("invalid Maildir subdir {0:?}: expected cur, new or tmp")]
    InvalidSubdir(String),
}

pub const CUR: &str = "cur";
pub const NEW: &str = "new";
pub const TMP: &str = "tmp";

/// One of the three Maildir subdirectories: cur, new, tmp.
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

/// A Maildir root on the filesystem (with cur/new/tmp subdirs).
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct Maildir {
    root: FsPath,
}

impl Maildir {
    /// Wraps `root` without checking that the subdirectories exist.
    pub fn from_path(root: impl Into<FsPath>) -> Self {
        Self { root: root.into() }
    }

    pub fn path(&self) -> &FsPath {
        &self.root
    }

    pub fn name(&self) -> Option<&str> {
        self.root.file_name()
    }

    pub fn subdir(&self, subdir: &MaildirSubdir) -> FsPath {
        match subdir {
            MaildirSubdir::Cur => self.cur(),
            MaildirSubdir::New => self.new(),
            MaildirSubdir::Tmp => self.tmp(),
        }
    }

    pub fn cur(&self) -> FsPath {
        self.root.join(CUR)
    }

    pub fn new(&self) -> FsPath {
        self.root.join(NEW)
    }

    pub fn tmp(&self) -> FsPath {
        self.root.join(TMP)
    }
}

impl Hash for Maildir {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.root.hash(state);
    }
}

impl AsRef<FsPath> for Maildir {
    fn as_ref(&self) -> &FsPath {
        &self.root
    }
}

impl From<FsPath> for Maildir {
    fn from(root: FsPath) -> Self {
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
