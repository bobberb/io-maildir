# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- Added the `MaildirCoroutine` trait mirroring `core::ops::Coroutine`.

  Composed of `Yield` and `Return` associated types, plus a two-variant `MaildirCoroutineState<Y, R>` (`Yielded(Y)` and `Complete(R)`). Every io-maildir coroutine picks the shared `MaildirYield` enum, mixing filesystem `Wants*` requests (file/dir read, create, remove, rename, copy, exists) with the three environmental inputs the Maildir delivery protocol needs to mint message identifiers (`WantsTime`, `WantsPid`, `WantsHostname`). Replies are fed back via the matching `MaildirReply` enum.

- Added the `maildir_try!` macro: coroutine equivalent of `?`.

  Advances one inner resume step, re-yields intermediate `Yielded(y)` (via `Into`), and short-circuits on `Complete(Err(_))`.

- Added I/O-free Maildir layout coroutines: `MaildirCreate`, `MaildirDelete`, `MaildirRename`, `MaildirList`.

  `MaildirCreate` creates `root`, `cur`, `new`, `tmp` in lexicographic order. `MaildirDelete` recursively removes a Maildir. `MaildirRename` renames within the parent directory. `MaildirList` walks every valid Maildir under a root: default fs layout descends recursively through nested subfolders; `MaildirList::new(root).maildirpp(true)` switches to the Maildir++ flat-dotted-siblings layout. Plain Maildir is the zero-subfolders degenerate case of fs.

- Added I/O-free entry coroutines: `MaildirEntryStore`, `MaildirEntryGet`, `MaildirEntryList`, `MaildirEntryCopy`, `MaildirEntryMove`, `MaildirEntryLocate`.

  `MaildirEntryStore` follows the Maildir delivery protocol: writes to `/tmp` first, then atomically renames into `/cur` or `/new`, producing IDs of the shape `secs.#counter.M<nanos>P<pid>.<host>`. `MaildirEntryList` scans both `/new` and `/cur` and returns every confirmed entry. `MaildirEntryLocate` finds an entry file by ID across `cur`, `new` and `tmp`.

- Added I/O-free flag coroutines: `MaildirFlagsAdd`, `MaildirFlagsRemove`, `MaildirFlagsSet`.

  Each rewrites the `:2,<flags>` suffix on the entry filename in place. Custom keywords are surfaced via the optional `X-Keywords` / `X-Label` header round-trip, gated by per-client `keywords_header` / `strip_headers` switches.

- Added I/O-free Dovecot keywords coroutines: `DovecotLoad`, `DovecotStore`.

  Read / write the `dovecot-keywords` slot table mapping `a..z` letters to user-defined keyword strings, gated by the per-client `dovecot_keywords` switch.

- Added the `FsPath` / `MaildirPath` split with `MaildirStore` as the translator.

  `FsPath` is the literal `/`-separated filesystem path (always uses `/` regardless of host OS). `MaildirPath` is the logical mailbox hierarchy (`"Inbox/2024/Q1"`); the empty path designates the store root. `MaildirStore { root: FsPath, maildirpp: bool }` resolves logical names to fs paths: `Foo/Bar` becomes `<root>/Foo/Bar` in fs layout, `<root>/.Foo.Bar` in Maildir++. Layout-aware coroutines (`MaildirCreate`, `MaildirDelete`, `MaildirRename`, `MaildirList`) take `&MaildirStore` plus (where applicable) `MaildirPath`; entry- and flag-level coroutines operate on resolved `Maildir` handles below the layout abstraction.

- Added the `client` cargo feature (default) enabling `MaildirClient`.

  Standard, blocking client backed by `std::fs` that drives any standard-Yield coroutine to completion. Exposes one method per coroutine plus high-level helpers (`create_maildir`, `delete_maildir`, `rename_maildir`, `load_maildir`) that take logical mailbox names instead of fs paths. Per-protocol behaviour is configured via `pub` fields: `store: MaildirStore` (layout), `dovecot_keywords: bool`, `keywords_header: bool`, `strip_headers: bool`.

- Added the `parser` cargo feature (default).

  Pulls in `mail-parser` to expose `MaildirFullEntry`, an entry paired with its parsed headers.

- Added the `serde` cargo feature (default).

  Forwards `serde` support to `mail-parser` so parsed entries can be serialized.

[unreleased]: https://github.com/pimalaya/io-maildir/compare/root..HEAD
