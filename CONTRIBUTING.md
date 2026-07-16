# Contributing guide

Thank you for investing your time in contributing to io-maildir.

Whether you are a human or an AI agent, read these in order before touching the code:

1. the [Pimalaya README](https://github.com/pimalaya) for what the project is and how its repositories stack;
2. the [Pimalaya CONTRIBUTING](https://github.com/pimalaya/.github/blob/master/CONTRIBUTING.md) guide, which chains to the shared architecture and guidelines;
3. the inline header documentation, starting with src/lib.rs: it is the architecture document of this crate;
4. the docs/ folder for the development history and living plans.

Everything below documents only what differs from the Pimalaya standards.

## Feature matrix

The I/O-free coroutines are the featureless no_std core; every cargo feature only gates additional code and dependencies on top. Check the core stays no_std and each layer still builds:

- no default features: the pure no_std core, no std, no filesystem, no parser.
- client: the std-blocking driver over the filesystem.
- parser: mail-parser support exposing the parsed-entry helpers.
- serde: forwards serde support to mail-parser.
- all features: the default set, plus anything gated behind docsrs.

Run the test suite with the default features and, at least, once with no default features to keep the core std-free.
