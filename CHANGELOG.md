# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- Added basic I/O-free coroutines.

- Added standard, blocking client.

### Changed

- Unified every coroutine under a single `MaildirCoroutine` trait (in `crate::coroutine`) with associated `Arg` / `Output` / `Error`. `resume` now returns `MaildirCoroutineState<Output, Error>` directly; the per-coroutine `Maildir*Result` / `Dovecot*Result` enums are gone, replaced by small `Maildir*Ok` output structs where the previous `Ok { ... }` variant carried more than one field. `MaildirClient::run<C: MaildirCoroutine>` drives any coroutine to completion against the local filesystem.

- Migrated every coroutine to the generator-shape trait pattern: `MaildirCoroutine` now exposes associated `Yield` / `Return` types and `MaildirCoroutineState<Y, R>` shrinks to two variants (`Yielded(Y)` / `Complete(R)`). Filesystem and environment requests live on a single shared `MaildirYield` enum and are answered with a single shared `MaildirReply` enum; per-coroutine `Maildir*Arg` enums are gone. `MaildirMessageLocateOk` and `MaildirMessageStoreOk` are renamed to `MaildirMessageLocateOutput` and `MaildirMessageStoreOutput` for consistency. `MaildirClient::run` is now generic over any standard-yield coroutine and every client method collapses to a one-line call.

- Aligned every coroutine on the canonical io-imap / io-smtp / io-http reference model: private `State` enums with `fmt::Display`, top-of-loop `trace!("<op>: {}", self.state)` instrumentation, the new `maildir_try!` macro for nested coroutines, uniform `"Maildir <op> failed: <reason>"` error message format, and a simplified canonical test layout (success + invalid-reply) per coroutine file.

- Renamed the protocol-agnostic items away from mail-specialised vocabulary to align io-maildir with io-m2dir:
  - Module `message` renamed to `entry`.
  - Type `MaildirMessage` renamed to `MaildirFullEntry`.
  - Coroutines `MaildirMessageCopy`, `MaildirMessageGet`, `MaildirMessageLocate`, `MaildirMessagesList`, `MaildirMessageMove`, `MaildirMessageStore` (and their `*Error`/`*Output` siblings) renamed to `MaildirEntryCopy`, `MaildirEntryGet`, `MaildirEntryLocate`, `MaildirEntryList`, `MaildirEntryMove`, `MaildirEntryStore`.

- Collapsed the `MaildirClient` layout fields into a single `maildirpp: bool`. Dropped the unused `maildirpp_inbox: String` (presentation concern, not a client one) and `fs_layout: bool` (now the default behaviour when `maildirpp = false`). Renamed `maildir_plus` to `maildirpp`.

- Reworked `MaildirList` for the simplified layout model: default (`maildirpp = false`) is fs layout with full recursive descent through nested subfolders; `MaildirList::new(root).maildirpp(true)` switches to the Maildir++ flat-dotted-siblings layout. Replaced the `include_dotted` / `include_root` builder methods with the single `maildirpp` switch. Plain Maildir works as the zero-subfolders degenerate case of fs.

- Split the single `MaildirPath` type into two distinct flavours and introduced a `MaildirStore` to translate between them:
  - `FsPath` (formerly `MaildirPath`) is the literal `/`-separated filesystem path.
  - `MaildirPath` is now the logical mailbox hierarchy ("Inbox/2024/Q1"). The empty path designates the store root.
  - New `crate::store::MaildirStore { pub root: FsPath, pub maildirpp: bool }` carries the layout and resolves logical names to fs paths: `Foo/Bar` → `<root>/Foo/Bar` in fs, `<root>/.Foo.Bar` in Maildir++.
  - Layout-aware coroutines `MaildirCreate`, `MaildirDelete`, `MaildirRename`, `MaildirList` now take `&MaildirStore` plus (where applicable) `MaildirPath`. The resolution happens once at construction time; the coroutines themselves stay layout-agnostic at runtime.
  - `MaildirClient` now exposes `pub store: MaildirStore` (replacing the per-client `maildirpp` field); its `create_maildir`, `delete_maildir`, `rename_maildir`, `load_maildir` methods take logical names instead of fs paths.
  - `MaildirListOptions` removed: its sole field moved to `MaildirStore`.
  - Entry- and flag-level coroutines (`entry/*`, `flag/*`) are unchanged: they operate on resolved `Maildir` handles, below the layout abstraction.

- Merged `LoadMaildirError` into `MaildirClientError` (its `NotDir` and `MissingSubdir` are now top-level variants). The `load_maildir` free function was inlined into `MaildirClient::load_maildir`, and every filesystem helper (`create_dirs`, `remove_dirs`, `write_files`, `read_dirs`, `read_files`, `rename_paths`, `copy_paths`, `file_exists`, `dir_exists`, `normalize_path`) was inlined directly into `MaildirClient::run`. The windows `\\` → `/` normalisation moved from the deleted `normalize_path` helper into the `impl From<PathBuf>` / `From<&Path>` for `MaildirPath` in `src/path.rs`.

[unreleased]: https://github.com/pimalaya/io-maildir/compare/root..HEAD
