//! Collection of I/O-free, resumable and composable Maildir state
//! machines.
//!
//! Each coroutine emits filesystem requests via the `Wants*` variants
//! of its `*Result` enum (e.g. `WantsDirCreate`, `WantsFileRead`,
//! `WantsRename`). The caller performs the matching operation and
//! feeds the corresponding `*Arg` variant back into the next
//! `resume` call to make progress.

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
