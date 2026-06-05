//! Maildir layout: I/O-free coroutines managing the cur/new/tmp
//! directory tree (create, delete, list, rename), plus the
//! [`Maildir`](types::Maildir) handle and subdir enum under [`types`].

pub mod create;
pub mod delete;
pub mod list;
pub mod rename;
pub mod types;
