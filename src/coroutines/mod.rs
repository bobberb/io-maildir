//! Collection of I/O-free, resumable and composable Maildir state
//! machines.
//!
//! Every coroutine reports progression through the unified
//! [`MaildirCoroutineState`](crate::coroutine::MaildirCoroutineState)
//! enum (filesystem-flavoured `Wants*` variants, [`Done`], [`Err`])
//! and consumes its own per-coroutine `Arg` enum on resume. Drive any
//! coroutine end-to-end against the local filesystem via
//! [`MaildirClient::run`](crate::client::MaildirClient::run).
//!
//! [`Done`]: crate::coroutine::MaildirCoroutineState::Done
//! [`Err`]: crate::coroutine::MaildirCoroutineState::Err

pub mod dovecot_load;
pub mod dovecot_store;
pub mod flags_add;
pub mod flags_remove;
pub mod flags_set;
pub mod maildir_create;
pub mod maildir_delete;
pub mod maildir_list;
pub mod maildir_rename;
pub mod message_copy;
pub mod message_get;
pub mod message_list;
pub mod message_locate;
pub mod message_move;
pub mod message_store;
