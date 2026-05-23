use core::fmt;

use alloc::collections::BTreeSet;

use log::trace;

use crate::path::MaildirPath;

#[derive(Clone, Debug, Default, Eq, Ord, PartialEq, PartialOrd)]
pub struct Flags(BTreeSet<Flag>);

impl From<&MaildirPath> for Flags {
    fn from(path: &MaildirPath) -> Self {
        let Some(file_name) = path.file_name() else {
            return Default::default();
        };

        let Some((_, flags)) = file_name.rsplit_once(',') else {
            return Default::default();
        };

        Flags::from_iter(flags.chars().filter_map(Flag::from_char))
    }
}

impl fmt::Display for Flags {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // BTreeSet iterates in sorted order, so the on-disk
        // representation is deterministic.
        for flag in &self.0 {
            write!(f, "{flag}")?;
        }
        Ok(())
    }
}

impl Flags {
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn contains(&self, flag: &Flag) -> bool {
        self.0.contains(flag)
    }

    pub fn extend(&mut self, flags: Flags) {
        self.0.extend(flags.0)
    }

    pub fn difference(&mut self, flags: &Flags) {
        self.0 = self.0.difference(&flags.0).cloned().collect();
    }
}

impl FromIterator<Flag> for Flags {
    fn from_iter<I: IntoIterator<Item = Flag>>(iter: I) -> Self {
        Flags(iter.into_iter().collect())
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum Flag {
    Passed,
    Replied,
    Seen,
    Trashed,
    Draft,
    Flagged,
}

impl Flag {
    pub fn from_char(c: char) -> Option<Flag> {
        match c {
            'P' => Some(Flag::Passed),
            'R' => Some(Flag::Replied),
            'S' => Some(Flag::Seen),
            'T' => Some(Flag::Trashed),
            'D' => Some(Flag::Draft),
            'F' => Some(Flag::Flagged),
            c => {
                trace!("invalid maildir flag `{c}`, ignoring");
                None
            }
        }
    }
}

impl fmt::Display for Flag {
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
