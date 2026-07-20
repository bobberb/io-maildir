use core::fmt;

use alloc::{
    collections::{BTreeMap, BTreeSet},
    string::String,
    vec::Vec,
};
use core::fmt::Write as _;

use log::trace;

use crate::path::MaildirPath;

#[derive(Clone, Debug, Default, Eq, Ord, PartialEq, PartialOrd)]
pub struct MaildirFlags {
    flags: BTreeSet<MaildirFlag>,
    /// Extra info-section letters appended verbatim by [`Display`].
    /// Used to carry resolved dovecot `a..z` slot letters that have no
    /// counterpart in the named-variant set.
    extra_letters: BTreeSet<char>,
}

impl From<&MaildirPath> for MaildirFlags {
    fn from(path: &MaildirPath) -> Self {
        let Some(file_name) = path.file_name() else {
            return Default::default();
        };

        let Some((_, letters)) = file_name.rsplit_once(',') else {
            return Default::default();
        };

        // Partition the info-section: named IANA letters (P/R/S/T/D/F)
        // become variants; every other letter (dovecot `a..z` slot
        // letters in particular) is carried verbatim in `extra_letters`
        // so flag-op round-trips do not strip the keyword's slot.
        let mut flags = BTreeSet::new();
        let mut extra_letters = BTreeSet::new();
        for c in letters.chars() {
            match MaildirFlag::from_char(c) {
                Some(flag) => {
                    flags.insert(flag);
                }
                None => {
                    extra_letters.insert(c);
                }
            }
        }

        MaildirFlags {
            flags,
            extra_letters,
        }
    }
}

impl fmt::Display for MaildirFlags {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // BTreeSet iterates in sorted order so the on-disk
        // representation is deterministic.
        for flag in &self.flags {
            write!(f, "{flag}")?;
        }
        for letter in &self.extra_letters {
            f.write_char(*letter)?;
        }
        Ok(())
    }
}

impl MaildirFlags {
    pub fn is_empty(&self) -> bool {
        self.flags.is_empty() && self.extra_letters.is_empty()
    }

    pub fn len(&self) -> usize {
        self.flags.len() + self.extra_letters.len()
    }

    pub fn contains(&self, flag: &MaildirFlag) -> bool {
        self.flags.contains(flag)
    }

    pub fn extend(&mut self, flags: MaildirFlags) {
        self.flags.extend(flags.flags);
        self.extra_letters.extend(flags.extra_letters);
    }

    pub fn difference(&mut self, flags: &MaildirFlags) {
        self.flags = self.flags.difference(&flags.flags).cloned().collect();
        self.extra_letters = self
            .extra_letters
            .difference(&flags.extra_letters)
            .copied()
            .collect();
    }

    pub fn iter(&self) -> impl Iterator<Item = &MaildirFlag> {
        self.flags.iter()
    }

    pub fn insert(&mut self, flag: MaildirFlag) -> bool {
        self.flags.insert(flag)
    }

    /// Path-only constructor extended with a dovecot-keywords table.
    ///
    /// Standard IANA letters (P/R/S/T/D/F) parse as today; lowercase
    /// `a..z` letters resolve through `table` and produce
    /// [`MaildirFlag::Keyword`] entries. Letters absent from the table
    /// are dropped.
    pub fn with_dovecot(path: &MaildirPath, table: &BTreeMap<char, String>) -> Self {
        let Some(file_name) = path.file_name() else {
            return Default::default();
        };

        let Some((_, letters)) = file_name.rsplit_once(',') else {
            return Default::default();
        };

        let mut flags = BTreeSet::new();
        for c in letters.chars() {
            if let Some(flag) = MaildirFlag::from_char(c) {
                flags.insert(flag);
            } else if c.is_ascii_lowercase() {
                if let Some(name) = table.get(&c) {
                    flags.insert(MaildirFlag::Keyword(name.clone()));
                }
            }
        }
        MaildirFlags {
            flags,
            extra_letters: BTreeSet::new(),
        }
    }

    /// Adds raw keyword strings to the set as [`MaildirFlag::Keyword`].
    pub fn extend_keywords<I, S>(&mut self, keywords: I)
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        for k in keywords {
            self.flags.insert(MaildirFlag::Keyword(k.into()));
        }
    }

    /// Appends raw info-section letters that will be written verbatim
    /// by [`fmt::Display`]. Use it for dovecot `a..z` slot letters
    /// already resolved at the caller.
    pub fn extend_letters<I>(&mut self, letters: I)
    where
        I: IntoIterator<Item = char>,
    {
        self.extra_letters.extend(letters);
    }

    /// Drains every [`MaildirFlag::Keyword`] variant out of the set,
    /// returning the collected keyword strings in deterministic
    /// (lexicographic) order.
    pub fn drain_keywords(&mut self) -> Vec<String> {
        let keywords: BTreeSet<MaildirFlag> = self
            .flags
            .iter()
            .filter(|f| matches!(f, MaildirFlag::Keyword(_)))
            .cloned()
            .collect();

        for f in &keywords {
            self.flags.remove(f);
        }

        keywords
            .into_iter()
            .filter_map(|f| match f {
                MaildirFlag::Keyword(s) => Some(s),
                _ => None,
            })
            .collect()
    }
}

impl FromIterator<MaildirFlag> for MaildirFlags {
    fn from_iter<I: IntoIterator<Item = MaildirFlag>>(iter: I) -> Self {
        MaildirFlags {
            flags: iter.into_iter().collect(),
            extra_letters: BTreeSet::new(),
        }
    }
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum MaildirFlag {
    Passed,
    Replied,
    Seen,
    Trashed,
    Draft,
    Flagged,
    /// Custom keyword that does not map to a Maildir info-section
    /// letter; serialised externally via the dovecot-keywords file or
    /// a configured header.
    Keyword(String),
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

    pub fn keyword(s: impl Into<String>) -> Self {
        Self::Keyword(s.into())
    }

    pub fn as_keyword(&self) -> Option<&str> {
        match self {
            Self::Keyword(s) => Some(s.as_str()),
            _ => None,
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
            // NOTE: Keyword has no letter encoding; serialised through
            // the dovecot-keywords file or a header.
            Self::Keyword(_) => Ok(()),
        }
    }
}

/// Choice of body header used to ferry custom keywords alongside the
/// message. Selecting [`KeywordHeader::XKeywords`] follows the
/// OfflineIMAP / mbsync convention (comma-separated); selecting
/// [`KeywordHeader::XLabel`] follows the mutt / notmuch convention
/// (space-separated).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum KeywordHeader {
    XKeywords,
    XLabel,
}

impl KeywordHeader {
    pub fn header_name(&self) -> &'static str {
        match self {
            Self::XKeywords => "X-Keywords",
            Self::XLabel => "X-Label",
        }
    }

    pub fn separator(&self) -> char {
        match self {
            Self::XKeywords => ',',
            Self::XLabel => ' ',
        }
    }
}

impl core::str::FromStr for KeywordHeader {
    type Err = &'static str;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_ascii_lowercase().as_str() {
            "x-keywords" | "xkeywords" | "x_keywords" => Ok(Self::XKeywords),
            "x-label" | "xlabel" | "x_label" => Ok(Self::XLabel),
            _ => Err("expected `x-keywords` or `x-label`"),
        }
    }
}

impl fmt::Display for KeywordHeader {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.header_name())
    }
}
