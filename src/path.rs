//! Two path flavours used across the crate: literal filesystem paths
//! ([`FsPath`]) and logical mailbox-hierarchy paths ([`MaildirPath`]).
//! A [`crate::store::MaildirStore`] translates between them under its
//! configured layout (fs nested, or Maildir++ flat-dotted).

use core::fmt;

use alloc::string::String;

/// Forward-slash separated literal filesystem path.
///
/// Always uses `/` regardless of host OS. `std::fs::*` accepts `/`-paths on
/// both Unix and Windows, so no boundary conversion is needed in the client
/// layer.
#[derive(Clone, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct FsPath(String);

impl FsPath {
    /// Builds a new path from `s` without validation.
    pub fn new(s: impl Into<String>) -> Self {
        Self(s.into())
    }

    /// Returns the path as a `&str`.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Returns the underlying [`String`].
    pub fn into_string(self) -> String {
        self.0
    }

    /// Returns `true` when the path is empty.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Returns a new path with `segment` appended after a `/`
    /// separator.
    ///
    /// If `self` is empty the result is `segment` alone (no leading
    /// `/`). Trailing `/` in `self` is normalized.
    pub fn join(&self, segment: &str) -> Self {
        let mut out = self.clone();
        out.push(segment);
        out
    }

    /// Appends `segment` to this path in place, inserting a `/`
    /// separator unless `self` is empty or already ends with one.
    pub fn push(&mut self, segment: &str) {
        if !self.0.is_empty() && !self.0.ends_with('/') {
            self.0.push('/');
        }
        self.0.push_str(segment);
    }

    /// Returns the final path component, if any.
    pub fn file_name(&self) -> Option<&str> {
        match self.0.rsplit_once('/') {
            Some((_, name)) if !name.is_empty() => Some(name),
            None if !self.0.is_empty() => Some(&self.0),
            _ => None,
        }
    }

    /// Returns the path without its final component, if any.
    pub fn parent(&self) -> Option<&str> {
        self.0.rsplit_once('/').map(|(parent, _)| parent)
    }

    /// Replaces the final component of this path with `name`.
    ///
    /// If `self` has no parent, the result is `name` alone.
    pub fn with_file_name(&self, name: &str) -> Self {
        match self.parent() {
            Some(parent) => Self::new(parent).join(name),
            None => Self::new(name),
        }
    }

    /// If `self` is rooted at `prefix`, returns the relative remainder
    /// (without leading `/`).
    pub fn strip_prefix(&self, prefix: &Self) -> Option<&str> {
        let rest = self.0.strip_prefix(prefix.as_str())?;
        Some(rest.strip_prefix('/').unwrap_or(rest))
    }

    /// Returns `true` when this path begins with `prefix`.
    pub fn starts_with(&self, prefix: &Self) -> bool {
        self.0.starts_with(prefix.as_str())
    }

    /// Iterates over the non-empty components of this path.
    pub fn components(&self) -> impl Iterator<Item = &str> {
        self.0.split('/').filter(|c| !c.is_empty())
    }
}

impl fmt::Display for FsPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.0, f)
    }
}

impl From<String> for FsPath {
    fn from(s: String) -> Self {
        Self(s)
    }
}

impl From<&str> for FsPath {
    fn from(s: &str) -> Self {
        Self(s.into())
    }
}

#[cfg(feature = "client")]
impl From<std::path::PathBuf> for FsPath {
    fn from(path: std::path::PathBuf) -> Self {
        let s = path.to_string_lossy().into_owned();
        #[cfg(windows)]
        let s = s.replace('\\', "/");
        Self(s)
    }
}

#[cfg(feature = "client")]
impl From<&std::path::Path> for FsPath {
    fn from(path: &std::path::Path) -> Self {
        let s = path.to_string_lossy().into_owned();
        #[cfg(windows)]
        let s = s.replace('\\', "/");
        Self(s)
    }
}

#[cfg(feature = "client")]
impl From<FsPath> for std::path::PathBuf {
    fn from(path: FsPath) -> Self {
        Self::from(path.0)
    }
}

impl AsRef<str> for FsPath {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

#[cfg(feature = "client")]
impl AsRef<std::path::Path> for FsPath {
    fn as_ref(&self) -> &std::path::Path {
        std::path::Path::new(&self.0)
    }
}

/// Logical mailbox-hierarchy path, always `/`-separated regardless of
/// the on-disk layout.
///
/// The empty path designates the store root itself (which is INBOX in
/// Maildir++). A [`crate::store::MaildirStore`] turns this into a
/// concrete [`FsPath`] under its layout: fs nested ("Foo/Bar" →
/// `<root>/Foo/Bar`) or Maildir++ flat-dotted ("Foo/Bar" →
/// `<root>/.Foo.Bar`).
#[derive(Clone, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct MaildirPath(String);

impl MaildirPath {
    /// Returns the path as a `&str`.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Returns `true` when the path is empty (i.e. designates the
    /// store root).
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Iterates over the non-empty hierarchy segments.
    pub fn components(&self) -> impl Iterator<Item = &str> {
        self.0.split('/').filter(|c| !c.is_empty())
    }
}

impl fmt::Display for MaildirPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.0, f)
    }
}

impl From<String> for MaildirPath {
    fn from(s: String) -> Self {
        Self(s)
    }
}

impl From<&str> for MaildirPath {
    fn from(s: &str) -> Self {
        Self(s.into())
    }
}

impl AsRef<str> for MaildirPath {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use alloc::vec::Vec;

    use crate::path::{FsPath, MaildirPath};

    #[test]
    fn maildir_path_components_skips_empties() {
        let p = MaildirPath::from("/Foo//Bar/");
        let parts: Vec<&str> = p.components().collect();
        assert_eq!(parts, ["Foo", "Bar"]);
    }

    #[test]
    fn maildir_path_empty_is_root() {
        assert!(MaildirPath::default().is_empty());
        assert!(MaildirPath::from("").is_empty());
        assert!(!MaildirPath::from("Foo").is_empty());
    }

    #[test]
    fn join_inserts_separator() {
        let p = FsPath::new("a");
        assert_eq!(p.join("b").as_str(), "a/b");
    }

    #[test]
    fn join_on_empty_skips_separator() {
        let p = FsPath::default();
        assert_eq!(p.join("a").as_str(), "a");
    }

    #[test]
    fn join_normalises_trailing_separator() {
        let p = FsPath::new("a/");
        assert_eq!(p.join("b").as_str(), "a/b");
    }

    #[test]
    fn file_name_returns_last_segment() {
        assert_eq!(FsPath::new("a/b/c").file_name(), Some("c"));
        assert_eq!(FsPath::new("c").file_name(), Some("c"));
        assert_eq!(FsPath::default().file_name(), None);
        assert_eq!(FsPath::new("a/").file_name(), None);
    }

    #[test]
    fn parent_returns_path_without_last_segment() {
        assert_eq!(FsPath::new("a/b/c").parent(), Some("a/b"));
        assert_eq!(FsPath::new("a").parent(), None);
    }

    #[test]
    fn with_file_name_replaces_last_segment() {
        let p = FsPath::new("a/b/c");
        assert_eq!(p.with_file_name("d").as_str(), "a/b/d");

        let p = FsPath::new("a");
        assert_eq!(p.with_file_name("z").as_str(), "z");
    }

    #[test]
    fn strip_prefix_removes_leading_separator() {
        let p = FsPath::new("root/sub/leaf");
        let root = FsPath::new("root");
        assert_eq!(p.strip_prefix(&root), Some("sub/leaf"));
    }

    #[test]
    fn components_skips_empties() {
        let p = FsPath::new("/a//b/");
        let parts: Vec<&str> = p.components().collect();
        assert_eq!(parts, ["a", "b"]);
    }
}
