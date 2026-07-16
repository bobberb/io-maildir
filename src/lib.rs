#![no_std]
#![cfg_attr(docsrs, feature(doc_cfg))]

//! # io-maildir
//!
//! I/O-free Maildir coroutines: every filesystem access and every
//! environment lookup is a resumable state machine that emits a
//! request (create this directory, read that file, give me the current
//! time) instead of performing the operation itself. The caller owns
//! the syscalls and pumps the coroutine with the answers, whatever the
//! runtime (blocking, async, in-memory tests). The `client` feature
//! ships a ready-made std-blocking pump over [`std::fs`] for callers
//! who just want a working client.
//!
//! ## Layout: one module per Maildir concept
//!
//! Maildir is a de-facto format rather than an RFC, so the source tree
//! is organised by concept instead of by specification number. Each
//! concept folder holds its coroutines next to the sibling file
//! carrying the shared types they operate on:
//!
//! [`maildir`] manages the cur/new/tmp directory tree (create, delete,
//! list, rename) and owns the [`maildir::Maildir`] handle and the
//! [`maildir::MaildirSubdir`] enum. [`entry`] implements the delivery
//! protocol and the entry lifecycle (store, get, list, locate, copy,
//! move) and owns the [`entry::MaildirEntry`] handle and the
//! [`entry::MaildirFullEntry`] body. [`flag`] rewrites the
//! `:2,<flags>` info suffix on entry filenames (add, remove, set) and
//! owns the [`flag::MaildirFlags`] set. [`dovecot`] reads and writes
//! the `dovecot-keywords` sidecar mapping slot letters to custom
//! keywords.
//!
//! Code shared across those concepts lives at the crate root:
//! [`coroutine`] holds the [`coroutine::MaildirCoroutine`] trait and
//! its [`coroutine::MaildirYield`] / [`coroutine::MaildirReply`]
//! vocabulary; [`path`] holds the literal [`path::MaildirFsPath`] and
//! the logical [`path::MaildirPath`]; [`store`] holds the
//! [`store::MaildirStore`] that resolves a logical mailbox name to an
//! on-disk path under the fs or Maildir++ layout. The optional
//! [`client`] module (`client` feature) is the std-blocking
//! [`client::MaildirClient`] pump spanning every concept, which is why
//! it lives at the crate root rather than under one of them.
//!
//! ## The coroutine contract
//!
//! Every coroutine implements [`coroutine::MaildirCoroutine`]. Its
//! `resume(arg: Option<MaildirReply>)` returns a
//! [`coroutine::MaildirCoroutineState`]: either a yield or the
//! terminal result. The shared yield is [`coroutine::MaildirYield`],
//! mixing filesystem requests (create, read, exists, rename, copy,
//! remove) with the three environment inputs the delivery protocol
//! needs to mint message identifiers (time, pid, hostname). The driver
//! answers each yield with the matching [`coroutine::MaildirReply`] on
//! the next resume. The [`maildir_try!`] macro is the coroutine
//! equivalent of `?`.
//!
//! ## Naming
//!
//! Public types follow the Maildir-Target-Verb scheme
//! ([`entry::store::MaildirEntryStore`],
//! [`flag::add::MaildirFlagsAdd`]) with Error and Output companions;
//! single-step coroutines hold the request directly, multi-step ones
//! keep a private State enum.
//!
//! ## Features
//!
//! The I/O-free coroutines are always present and need no feature. The
//! `client` feature adds the std-blocking [`client::MaildirClient`].
//! The `parser` feature pulls mail-parser in to expose
//! [`entry::MaildirFullEntry::parsed`], and the `serde` feature
//! forwards serde support to mail-parser so parsed entries serialise.
//!
//! Runnable programs live in the [examples] folder, and the tests
//! demonstrate real usage.
//!
//! [examples]: https://github.com/pimalaya/io-maildir/tree/master/examples

#[macro_use]
extern crate alloc;
#[cfg(feature = "client")]
extern crate std;

#[cfg(feature = "client")]
pub mod client;
pub mod coroutine;
pub mod dovecot;
pub mod entry;
pub mod flag;
pub mod maildir;
pub mod path;
pub mod store;
