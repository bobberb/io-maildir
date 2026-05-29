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

[unreleased]: https://github.com/pimalaya/io-maildir/compare/root..HEAD
