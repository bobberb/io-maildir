# I/O Maildir [![Documentation](https://img.shields.io/docsrs/io-maildir?style=flat&logo=docs.rs&logoColor=white)](https://docs.rs/io-maildir/latest/io_maildir) [![Matrix](https://img.shields.io/badge/chat-%23pimalaya-blue?style=flat&logo=matrix&logoColor=white)](https://matrix.to/#/#pimalaya:matrix.org) [![Mastodon](https://img.shields.io/badge/news-%40pimalaya-blue?style=flat&logo=mastodon&logoColor=white)](https://fosstodon.org/@pimalaya)

Maildir client library, written in Rust

## Table of contents

- [Features](#features)
- [Specification coverage](#specification-coverage)
- [Examples](#examples)
  - [As a coroutine library](#as-a-coroutine-library)
  - [As a std client](#as-a-std-client)
- [More examples](#more-examples)
- [License](#license)
- [Social](#social)
- [Sponsoring](#sponsoring)

## Features

- **I/O-free** coroutines: every Maildir operation is exposed as a `resume(arg)` state machine. No filesystem calls, no async runtime. Drive against any blocking, async, or fuzz harness.
- **Standard, blocking client** (requires `client` feature): `MaildirClient::new(root)` wraps a filesystem root and exposes one method per coroutine; the resume loop is run for you via [`std::fs`].
- **Maildir delivery protocol**: the message-store coroutine writes to `/tmp` first, then atomically renames into `/cur` or `/new`, producing IDs of the shape `secs.#counter.M<nanos>P<pid>.<host>`.

*The `io-maildir` library is written in [Rust](https://www.rust-lang.org/), and relies on [cargo features](https://doc.rust-lang.org/cargo/reference/features.html) to enable or disable functionalities. Default features can be found in the `features` section of the [`Cargo.toml`](https://github.com/pimalaya/io-maildir/blob/master/Cargo.toml), or on [docs.rs](https://docs.rs/crate/io-maildir/latest/features).*

## Specification coverage

This library implements the [Maildir](https://en.wikipedia.org/wiki/Maildir) format as I/O-agnostic coroutines: no filesystem calls, no async runtime.

| Coroutine             | What it does                                                                                                |
|-----------------------|-------------------------------------------------------------------------------------------------------------|
| `MaildirCreate`       | Creates `root`, `cur`, `new`, `tmp` in lexicographic order                                                  |
| `MaildirDelete`       | Recursively removes a Maildir                                                                               |
| `MaildirRename`       | Renames a Maildir within its parent directory                                                               |
| `MaildirList`         | Lists every valid Maildir inside a root directory                                                           |
| `MaildirMessageStore` | Writes to `/tmp`, then atomically renames into `/cur` or `/new` with optional flags                         |
| `MaildirMessageGet`   | Locates a message by ID and reads its contents                                                              |
| `MaildirMessagesList` | Scans both `/new` and `/cur` and returns every message it finds                                             |
| `MaildirMessageCopy`  | Copies a message between Maildirs                                                                           |
| `MaildirMessageMove`  | Moves a message between Maildirs                                                                            |
| `MaildirMessageLocate`| Finds a message file by ID across `cur`, `new` and `tmp`                                                    |
| `MaildirFlagsAdd`     | Adds flags to a message in `/cur` (no-op for `/new` and `/tmp`)                                             |
| `MaildirFlagsRemove`  | Removes flags from a message in `/cur` (no-op for `/new` and `/tmp`)                                        |
| `MaildirFlagsSet`     | Replaces the flags of a message in `/cur` (no-op for `/new` and `/tmp`)                                     |

## Examples

`io-maildir` can be consumed two ways, depending on how much of the I/O stack you want to own. Each mode is gated by cargo features.

Whichever mode you pick, every coroutine exposes `resume(arg)` returning a result enum with four shapes:

- `WantsDirRead`, `WantsDirCreate`, `WantsDirRemove`, `WantsFileRead`, `WantsFileCreate`, `WantsRename`, `WantsCopy`: caller performs the matching filesystem operation and feeds back the corresponding `*Arg` variant.
- `Ok { … }` / `Ok(…)` / `Ok`: terminal success.
- `Err(…)`: terminal failure.

### As a coroutine library

No features required: works without [`std::fs`] and without an async runtime. You own the loop and the syscalls; the library only computes the operations to perform and consumes their results.

Create a fresh Maildir against a blocking caller (the same shape works under async or in-memory replay):

```rust,ignore
use std::fs;

use io_maildir::{coroutines::maildir_create::*, path::MaildirPath};

let root = MaildirPath::new("/path/to/maildir");

let mut coroutine = MaildirCreate::new(root);
let mut arg: Option<MaildirCreateArg> = None;

loop {
    match coroutine.resume(arg.take()) {
        MaildirCreateResult::Ok => break,
        MaildirCreateResult::WantsDirCreate(paths) => {
            for path in paths {
                fs::create_dir_all(path.as_str()).unwrap();
            }
            arg = Some(MaildirCreateArg::DirCreate);
        }
        MaildirCreateResult::Err(err) => panic!("{err}"),
    }
}
```

Drive a multi-step command (store a message) the same way:

```rust,ignore
use std::{
    fs, process,
    time::{SystemTime, UNIX_EPOCH},
};

use gethostname::gethostname;
use io_maildir::{
    coroutines::message_store::*,
    flag::Flags,
    maildir::{Maildir, MaildirSubdir},
    path::MaildirPath,
};

let maildir = Maildir::from_path(MaildirPath::new("/path/to/maildir"));
let contents = b"From: alice@example.com\r\nSubject: Hello\r\n\r\nHello!\r\n".to_vec();

let mut coroutine = MaildirMessageStore::new(
    maildir,
    MaildirSubdir::New,
    Flags::default(),
    contents,
);
let mut arg: Option<MaildirMessageStoreArg> = None;

let (id, path) = loop {
    match coroutine.resume(arg.take()) {
        MaildirMessageStoreResult::Ok { id, path } => break (id, path),
        MaildirMessageStoreResult::WantsTime => {
            let ts = SystemTime::now().duration_since(UNIX_EPOCH).unwrap();
            arg = Some(MaildirMessageStoreArg::Time { secs: ts.as_secs(), nanos: ts.subsec_nanos() });
        }
        MaildirMessageStoreResult::WantsPid => {
            arg = Some(MaildirMessageStoreArg::Pid(process::id()));
        }
        MaildirMessageStoreResult::WantsHostname => {
            arg = Some(MaildirMessageStoreArg::Hostname(gethostname().into_string().unwrap_or_default()));
        }
        MaildirMessageStoreResult::WantsFileCreate(files) => {
            for (path, bytes) in files {
                fs::write(path.as_str(), &bytes).unwrap();
            }
            arg = Some(MaildirMessageStoreArg::FileCreate);
        }
        MaildirMessageStoreResult::WantsRename(pairs) => {
            for (from, to) in pairs {
                fs::rename(from.as_str(), to.as_str()).unwrap();
            }
            arg = Some(MaildirMessageStoreArg::Rename);
        }
        MaildirMessageStoreResult::Err(err) => panic!("{err}"),
    }
};

println!("stored {id} at {path}");
```

### As a std client

Enable the `client` feature (on by default). `MaildirClient::new(root)` wraps a filesystem root and exposes one method per coroutine, driving the resume loop for you via [`std::fs`].

```toml,ignore
[dependencies]
io-maildir = "0.0.1" # client is enabled by default
```

```rust,ignore
use io_maildir::{client::MaildirClient, flag::Flags, maildir::MaildirSubdir};

let client = MaildirClient::new("/path/to/root");

client.create_maildir("/path/to/root/inbox")?;
let maildir = client.load_maildir("/path/to/root/inbox")?;

let contents = b"From: alice@example.com\r\nSubject: Hello\r\n\r\nHello!\r\n".to_vec();
let (id, path) = client.store(maildir, MaildirSubdir::New, Flags::default(), contents)?;

println!("stored {id} at {path}");
```

*See complete examples at [./examples](https://github.com/pimalaya/io-maildir/blob/master/examples).*

## More examples

Have a look at projects built on top of this library:

- [himalaya](https://github.com/pimalaya/himalaya): CLI to manage emails

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
