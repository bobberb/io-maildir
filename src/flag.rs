use core::fmt;

use alloc::collections::BTreeSet;

use log::trace;

use crate::path::MaildirPath;

#[derive(Clone, Debug, Default, Eq, Ord, PartialEq, PartialOrd)]
pub struct MaildirFlags(BTreeSet<MaildirFlag>);

impl From<&MaildirPath> for MaildirFlags {
    fn from(path: &MaildirPath) -> Self {
        let Some(file_name) = path.file_name() else {
            return Default::default();
        };

        let Some((_, flags)) = file_name.rsplit_once(',') else {
            return Default::default();
        };

        MaildirFlags::from_iter(flags.chars().filter_map(MaildirFlag::from_char))
    }
}

impl fmt::Display for MaildirFlags {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // BTreeSet iterates in sorted order, so the on-disk
        // representation is deterministic.
        for flag in &self.0 {
            write!(f, "{flag}")?;
        }
        Ok(())
    }
}

impl MaildirFlags {
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn contains(&self, flag: &MaildirFlag) -> bool {
        self.0.contains(flag)
    }

    pub fn extend(&mut self, flags: MaildirFlags) {
        self.0.extend(flags.0)
    }

    pub fn difference(&mut self, flags: &MaildirFlags) {
        self.0 = self.0.difference(&flags.0).cloned().collect();
    }
}

impl FromIterator<MaildirFlag> for MaildirFlags {
    fn from_iter<I: IntoIterator<Item = MaildirFlag>>(iter: I) -> Self {
        MaildirFlags(iter.into_iter().collect())
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum MaildirFlag {
    Passed,
    Replied,
    Seen,
    Trashed,
    Draft,
    Flagged,
}

impl MaildirFlag {
    pub fn from_char(c: char) -> Option<MaildirFlag> {
        match c {
            'P' => Some(MaildirFlag::Passed),
            'R' => Some(MaildirFlag::Replied),
            'S' => Some(MaildirFlag::Seen),
            'T' => Some(MaildirFlag::Trashed),
            'D' => Some(MaildirFlag::Draft),
            'F' => Some(MaildirFlag::Flagged),
            c => {
                trace!("invalid maildir flag `{c}`, ignoring");
                None
            }
        }
    }
}

impl fmt::Display for MaildirFlag {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Passed => write!(f, "P"),
            Self::Replied => write!(f, "R"),
            Self::Seen => write!(f, "S"),
            Self::Trashed => write!(f, "T"),
            Self::Draft => write!(f, "D"),
            Self::Flagged => write!(f, "F"),
        }
    }
}
