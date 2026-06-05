# I/O Maildir [![Documentation](https://img.shields.io/docsrs/io-maildir?style=flat&logo=docs.rs&logoColor=white)](https://docs.rs/io-maildir/latest/io_maildir) [![Matrix](https://img.shields.io/badge/chat-%23pimalaya-blue?style=flat&logo=matrix&logoColor=white)](https://matrix.to/#/#pimalaya:matrix.org) [![Mastodon](https://img.shields.io/badge/news-%40pimalaya-blue?style=flat&logo=mastodon&logoColor=white)](https://fosstodon.org/@pimalaya)

Maildir client library, written in Rust

This library is composed of 2 feature-gated layers:

- Low-level **I/O-free** coroutines: these `no_std`-compatible state machines contain the whole Maildir logic and can be used anywhere
- Mid-level **std client**: a standard, blocking Maildir client built on `std::fs`

## Table of contents

- [Features](#features)
- [Specification coverage](#specification-coverage)
- [Usage](#usage)
  - [I/O-free coroutines](#io-free-coroutines)
  - [Std client](#std-client)
- [Examples](#examples)
- [AI disclosure](#ai-disclosure)
- [License](#license)
- [Social](#social)
- [Sponsoring](#sponsoring)

## Features

- **I/O-free** coroutines: `no_std` state machines; no filesystem calls, no async runtime, no `std` required, drive against any blocking, async, or fuzz harness.
- Standard, blocking client (requires `client` feature) backed by `std::fs`.
- **Maildir delivery protocol**: the entry-store coroutine writes to `/tmp` first, then atomically renames into `/cur` or `/new`, producing IDs of the shape `secs.#counter.M<nanos>P<pid>.<host>`.
- **Maildir++** mode: optional dotted folder enumeration (`.Work.Foo`) and inbox surfacing, gated by the `maildirpp` store flag.
- **Dovecot keywords** resolution: read / write the `dovecot-keywords` slot table (`a..z` letters), gated by the `dovecot_keywords` client option.
- **Header round-trip** for custom keywords: inject and strip `X-Keywords` / `X-Label` headers, gated by `keywords_header` and `strip_headers`.

> [!TIP]
> I/O Maildir is written in [Rust](https://www.rust-lang.org/) and uses [cargo features](https://doc.rust-lang.org/cargo/reference/features.html) to gate backend support. The default feature set is declared in [Cargo.toml](./Cargo.toml) or on [docs.rs](https://docs.rs/crate/io-maildir/latest/features).

## Specification coverage

This library implements the [Maildir](https://en.wikipedia.org/wiki/Maildir) format as I/O-agnostic coroutines.

| Coroutine             | What it does                                                                                                |
|-----------------------|-------------------------------------------------------------------------------------------------------------|
| `MaildirCreate`       | Creates `root`, `cur`, `new`, `tmp` in lexicographic order                                                  |
| `MaildirDelete`       | Recursively removes a Maildir                                                                               |
| `MaildirRename`       | Renames a Maildir within its parent directory                                                               |
| `MaildirList`         | Lists every valid Maildir inside a root directory                                                           |
| `MaildirEntryStore`   | Writes to `/tmp`, then atomically renames into `/cur` or `/new` with optional flags                         |
| `MaildirEntryGet`     | Locates an entry by ID and reads its contents                                                               |
| `MaildirEntryList`    | Scans both `/new` and `/cur` and returns every confirmed entry                                              |
| `MaildirEntryCopy`    | Copies an entry between Maildirs                                                                            |
| `MaildirEntryMove`    | Moves an entry between Maildirs                                                                             |
| `MaildirEntryLocate`  | Finds an entry file by ID across `cur`, `new` and `tmp`                                                     |
| `MaildirFlagsAdd`     | Adds flags to an entry in `/cur` (no-op for `/new` and `/tmp`)                                              |
| `MaildirFlagsRemove`  | Removes flags from an entry in `/cur` (no-op for `/new` and `/tmp`)                                         |
| `MaildirFlagsSet`     | Replaces the flags of an entry in `/cur` (no-op for `/new` and `/tmp`)                                      |
| `DovecotLoad`         | Reads the per-folder `dovecot-keywords` slot table                                                          |
| `DovecotStore`        | Writes a per-folder `dovecot-keywords` slot table                                                           |

## Usage

I/O Maildir can be consumed two ways, depending on how much of the I/O stack you want to own. Each mode is gated by cargo features.

Whichever mode you pick, every coroutine implements the `MaildirCoroutine` trait. Its `resume(arg: Option<MaildirReply>)` method returns a `MaildirCoroutineState<Yield, Return>` with two variants:

- `Yielded(Y)`: intermediate. `Y` is `MaildirYield`, mixing filesystem step requests (`WantsDirCreate`, `WantsDirRead`, `WantsDirRemove`, `WantsDirExists`, `WantsFileCreate`, `WantsFileRead`, `WantsFileExists`, `WantsRename`, `WantsCopy`) with the three environmental inputs used by the delivery protocol to mint entry identifiers (`WantsTime`, `WantsPid`, `WantsHostname`).
- `Complete(R)`: terminal. By convention `R = Result<Output, Error>` carrying the operation's final value.

The driver answers each `Yielded(MaildirYield::Wants*)` with the matching `MaildirReply` variant on the next resume.

### I/O-free coroutines

No features required: works in `#![no_std]`, no filesystem calls, no async runtime. You own the loop and the syscalls; the library only computes the operations to perform and consumes their results.

Create a fresh Maildir against a blocking caller (the same shape works under async or in-memory replay):

```rust,no_run
use std::fs;

use io_maildir::{
    coroutine::*,
    maildir::create::MaildirCreate,
    path::{FsPath, MaildirPath},
    store::MaildirStore,
};

let store = MaildirStore { root: FsPath::new("/path/to/root"), maildirpp: false };
let name = MaildirPath::from("inbox");

let mut coroutine = MaildirCreate::new(&store, name);
let mut arg: Option<MaildirReply> = None;

loop {
    match coroutine.resume(arg.take()) {
        MaildirCoroutineState::Complete(Ok(())) => break,
        MaildirCoroutineState::Complete(Err(err)) => panic!("{err}"),
        MaildirCoroutineState::Yielded(MaildirYield::WantsDirCreate(paths)) => {
            for path in paths {
                fs::create_dir_all(path.as_str()).unwrap();
            }
            arg = Some(MaildirReply::DirCreate);
        }
        MaildirCoroutineState::Yielded(other) => unreachable!("MaildirCreate yielded {other:?}"),
    }
}
```

Drive a multi-step command (store an entry) the same way:

```rust,no_run
use std::{
    fs, process,
    time::{SystemTime, UNIX_EPOCH},
};

use gethostname::gethostname;
use io_maildir::{
    coroutine::*,
    entry::store::{MaildirEntryStore, MaildirEntryStoreOutput},
    flag::types::MaildirFlags,
    maildir::types::{Maildir, MaildirSubdir},
    path::FsPath,
};

let maildir = Maildir::from_path(FsPath::new("/path/to/root/inbox"));
let contents = b"From: alice@example.com\r\nSubject: Hello\r\n\r\nHello!\r\n".to_vec();

let mut coroutine = MaildirEntryStore::new(maildir, MaildirSubdir::New, MaildirFlags::default(), contents);
let mut arg: Option<MaildirReply> = None;

let MaildirEntryStoreOutput { id, path } = loop {
    match coroutine.resume(arg.take()) {
        MaildirCoroutineState::Complete(Ok(out)) => break out,
        MaildirCoroutineState::Complete(Err(err)) => panic!("{err}"),
        MaildirCoroutineState::Yielded(MaildirYield::WantsTime) => {
            let ts = SystemTime::now().duration_since(UNIX_EPOCH).unwrap();
            arg = Some(MaildirReply::Time { secs: ts.as_secs(), nanos: ts.subsec_nanos() });
        }
        MaildirCoroutineState::Yielded(MaildirYield::WantsPid) => {
            arg = Some(MaildirReply::Pid(process::id()));
        }
        MaildirCoroutineState::Yielded(MaildirYield::WantsHostname) => {
            arg = Some(MaildirReply::Hostname(gethostname().into_string().unwrap_or_default()));
        }
        MaildirCoroutineState::Yielded(MaildirYield::WantsFileCreate(files)) => {
            for (path, bytes) in files {
                fs::write(path.as_str(), &bytes).unwrap();
            }
            arg = Some(MaildirReply::FileCreate);
        }
        MaildirCoroutineState::Yielded(MaildirYield::WantsRename(pairs)) => {
            for (from, to) in pairs {
                fs::rename(from.as_str(), to.as_str()).unwrap();
            }
            arg = Some(MaildirReply::Rename);
        }
        MaildirCoroutineState::Yielded(other) => unreachable!("MaildirEntryStore yielded {other:?}"),
    }
};

println!("stored {id} at {path}");
```

### Std client

Enable the `client` feature (on by default). `MaildirClient::new(root)` wraps a filesystem root and exposes one method per coroutine; the resume loop is driven for you via `MaildirClient::run` and `std::fs`.

```toml,ignore
[dependencies]
io-maildir = "0.0.1" # client is enabled by default
```

```rust,no_run
use io_maildir::{
    client::{MaildirClient, MaildirClientError},
    flag::types::MaildirFlags,
    maildir::types::MaildirSubdir,
};

# fn main() -> Result<(), MaildirClientError> {
let mut client = MaildirClient::new("/path/to/root");
// client.store.maildirpp = true; // opt into Maildir++ if needed

client.create_maildir("inbox")?;
let maildir = client.load_maildir("inbox")?;

let contents = b"From: alice@example.com\r\nSubject: Hello\r\n\r\nHello!\r\n".to_vec();
let (id, path) = client.store(maildir, MaildirSubdir::New, MaildirFlags::default(), contents)?;

println!("stored {id} at {path}");
# Ok(())
# }
```

Logical mailbox names ("inbox", "Archive/2024") are translated to on-disk paths by `client.store` according to its `maildirpp` flag: in fs layout (default) "Archive/2024" becomes `<root>/Archive/2024/`; in Maildir++ it becomes `<root>/.Archive.2024/`.

## Examples

See complete examples at [./examples](https://github.com/pimalaya/io-maildir/blob/master/examples).

Have also a look at real-world projects built on top of this library:

- [Himalaya CLI](https://github.com/pimalaya/himalaya): CLI to manage emails
- [Himalaya TUI](https://github.com/pimalaya/himalaya-tui): TUI to manage emails
- [Neverest](https://github.com/pimalaya/neverest): CLI to synchronize emails

## AI disclosure

This project is developed with AI assistance. This section documents how, so users and downstream packagers can make informed decisions.

- **Tools**: Claude Code (Anthropic), Opus 4.7, invoked locally with a persistent project-scoped memory and a small set of repo-specific rules.

- **Used for**: Refactors, mechanical multi-file edits, boilerplate (feature gates, error enums, derive macros, trait impls), test scaffolding, doc polish, exploratory design conversations.

- **Not used for**: Engineering, critical code, git manipulation (commit, merge, rebase…), real-world tests.

- **Verification**: Every AI-assisted change is read, compiled, tested, and formatted before commit (`nix develop --command cargo check / cargo test / cargo
fmt`). Behavioural correctness is verified against the relevant spec, not assumed from the model output. Tests are never adjusted to fit
AI-generated code; the code is adjusted to fit correct behaviour.

- **Limitations**: AI models occasionally produce code that compiles and passes tests but is subtly wrong: off-by-one errors, missed edge cases, plausible
but nonexistent APIs, stale spec references. The verification workflow catches most of this; it does not catch all of it. Bug reports are welcome and taken
seriously.

- **Last reviewed**: 05/06/2026

## License

This project is licensed under either of:

- [MIT license](LICENSE-MIT)
- [Apache License, Version 2.0](LICENSE-APACHE)

at your option.

## Social

- Chat on [Matrix](https://matrix.to/#/#pimalaya:matrix.org)
- News on [Mastodon](https://fosstodon.org/@pimalaya) or [RSS](https://fosstodon.org/@pimalaya.rss)
- Mail at [pimalaya.org@posteo.net](mailto:pimalaya.org@posteo.net)

## Sponsoring

[![nlnet](https://nlnet.nl/logo/banner-160x60.png)](https://nlnet.nl/)

Special thanks to the [NLnet foundation](https://nlnet.nl/) and the [European Commission](https://www.ngi.eu/) that have been financially supporting the project for years:

- 2022 → 2023: [NGI Assure](https://nlnet.nl/project/Himalaya/)
- 2023 → 2024: [NGI Zero Entrust](https://nlnet.nl/project/Pimalaya/)
- 2024 → 2026: [NGI Zero Core](https://nlnet.nl/project/Pimalaya-PIM/)
- *2027 in preparation…*

If you appreciate the project, feel free to donate using one of the following providers:

[![GitHub](https://img.shields.io/badge/-GitHub%20Sponsors-fafbfc?logo=GitHub%20Sponsors)](https://github.com/sponsors/soywod)
[![Ko-fi](https://img.shields.io/badge/-Ko--fi-ff5e5a?logo=Ko-fi&logoColor=ffffff)](https://ko-fi.com/soywod)
[![Buy Me a Coffee](https://img.shields.io/badge/-Buy%20Me%20a%20Coffee-ffdd00?logo=Buy%20Me%20A%20Coffee&logoColor=000000)](https://www.buymeacoffee.com/soywod)
[![Liberapay](https://img.shields.io/badge/-Liberapay-f6c915?logo=Liberapay&logoColor=222222)](https://liberapay.com/soywod)
[![thanks.dev](https://img.shields.io/badge/-thanks.dev-000000?logo=data:image/svg+xml;base64,PHN2ZyB3aWR0aD0iMjQuMDk3IiBoZWlnaHQ9IjE3LjU5NyIgY2xhc3M9InctMzYgbWwtMiBsZzpteC0wIHByaW50Om14LTAgcHJpbnQ6aW52ZXJ0IiB4bWxucz0iaHR0cDovL3d3dy53My5vcmcvMjAwMC9zdmciPjxwYXRoIGQ9Ik05Ljc4MyAxNy41OTdINy4zOThjLTEuMTY4IDAtMi4wOTItLjI5Ny0yLjc3My0uODktLjY4LS41OTMtMS4wMi0xLjQ2Mi0xLjAyLTIuNjA2di0xLjM0NmMwLTEuMDE4LS4yMjctMS43NS0uNjc4LTIuMTk1LS40NTItLjQ0Ni0xLjIzMi0uNjY5LTIuMzQtLjY2OUgwVjcuNzA1aC41ODdjMS4xMDggMCAxLjg4OC0uMjIyIDIuMzQtLjY2OC40NTEtLjQ0Ni42NzctMS4xNzcuNjc3LTIuMTk1VjMuNDk2YzAtMS4xNDQuMzQtMi4wMTMgMS4wMjEtMi42MDZDNS4zMDUuMjk3IDYuMjMgMCA3LjM5OCAwaDIuMzg1djEuOTg3aC0uOTg1Yy0uMzYxIDAtLjY4OC4wMjctLjk4LjA4MmExLjcxOSAxLjcxOSAwIDAgMC0uNzM2LjMwN2MtLjIwNS4xNTYtLjM1OC4zODQtLjQ2LjY4Mi0uMTAzLjI5OC0uMTU0LjY4Mi0uMTU0IDEuMTUxVjUuMjNjMCAuODY3LS4yNDkgMS41ODYtLjc0NSAyLjE1NS0uNDk3LjU2OS0xLjE1OCAxLjAwNC0xLjk4MyAxLjMwNXYuMjE3Yy44MjUuMyAxLjQ4Ni43MzYgMS45ODMgMS4zMDUuNDk2LjU3Ljc0NSAxLjI4Ny43NDUgMi4xNTR2MS4wMjFjMCAuNDcuMDUxLjg1NC4xNTMgMS4xNTIuMTAzLjI5OC4yNTYuNTI1LjQ2MS42ODIuMTkzLjE1Ny40MzcuMjYuNzMyLjMxMi4yOTUuMDUuNjIzLjA3Ni45ODQuMDc2aC45ODVabTE0LjMxNC03LjcwNmgtLjU4OGMtMS4xMDggMC0xLjg4OC4yMjMtMi4zNC42NjktLjQ1LjQ0Ni0uNjc3IDEuMTc3LS42NzcgMi4xOTVWMTQuMWMwIDEuMTQ0LS4zNCAyLjAxMy0xLjAyIDIuNjA2LS42OC41OTMtMS42MDUuODktMi43NzQuODloLTIuMzg0di0xLjk4OGguOTg0Yy4zNjIgMCAuNjg4LS4wMjcuOTgtLjA4LjI5Mi0uMDU1LjUzOC0uMTU3LjczNy0uMzA4LjIwNC0uMTU3LjM1OC0uMzg0LjQ2LS42ODIuMTAzLS4yOTguMTU0LS42ODIuMTU0LTEuMTUydi0xLjAyYzAtLjg2OC4yNDgtMS41ODYuNzQ1LTIuMTU1LjQ5Ny0uNTcgMS4xNTgtMS4wMDQgMS45ODMtMS4zMDV2LS4yMTdjLS44MjUtLjMwMS0xLjQ4Ni0uNzM2LTEuOTgzLTEuMzA1LS40OTctLjU3LS43NDUtMS4yODgtLjc0NS0yLjE1NXYtMS4wMmMwLS40Ny0uMDUxLS44NTQtLjE1NC0xLjE1Mi0uMTAyLS4yOTgtLjI1Ni0uNTI2LS40Ni0uNjgyYTEuNzE5IDEuNzE5IDAgMCAwLS43MzctLjMwNyA1LjM5NSA1LjM5NSAwIDAgMC0uOTgtLjA4MmgtLjk4NFYwaDIuMzg0YzEuMTY5IDAgMi4wOTMuMjk3IDIuNzc0Ljg5LjY4LjU5MyAxLjAyIDEuNDYyIDEuMDIgMi42MDZ2MS4zNDZjMCAxLjAxOC4yMjYgMS43NS42NzggMi4xOTUuNDUxLjQ0NiAxLjIzMS42NjggMi4zNC42NjhoLjU4N3oiIGZpbGw9IiNmZmYiLz48L3N2Zz4=)](https://thanks.dev/soywod)
[![PayPal](https://img.shields.io/badge/-PayPal-0079c1?logo=PayPal&logoColor=ffffff)](https://www.paypal.com/paypalme/soywod)
