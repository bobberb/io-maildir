//! Maildir flags: I/O-free coroutines rewriting the `:2,<flags>`
//! suffix on entry filenames, plus the flag set / keyword-header
//! types under [`types`].

pub mod add;
pub mod remove;
pub mod set;
pub mod types;
