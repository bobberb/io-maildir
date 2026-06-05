//! Maildir entries: I/O-free coroutines for the delivery protocol
//! and entry lifecycle (store, get, list, locate, copy, move), plus
//! RFC 5322 header helpers under [`headers`].

pub mod copy;
pub mod get;
pub mod headers;
pub mod list;
pub mod locate;
pub mod r#move;
pub mod store;
pub mod types;
