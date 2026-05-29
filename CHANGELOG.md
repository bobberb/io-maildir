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

[unreleased]: https://github.com/pimalaya/io-maildir/compare/root..HEAD
