# Guidelines alignment

Log of the realignment of io-maildir to the Pimalaya documentation and naming guidelines.

## 2026-07-16: full alignment (Landed)

Structure:

- Removed the banned types catch-all modules. The shared types of each concept moved into a sibling module file next to its folder: entry/mod.rs plus entry/types.rs became entry.rs, and likewise for flag and maildir. Public paths lost the types segment (entry::MaildirEntry, flag::MaildirFlags, maildir::Maildir).
- Split the oversized MaildirClient impl block, whose sections were marked by dashed banner comments, into one documented impl block per concept (lifecycle, flags, entries).

Naming (strict domain prefix):

- FsPath became MaildirFsPath. The logical MaildirPath kept its name.
- DovecotLoad, DovecotStore and their error companions became MaildirDovecotLoad, MaildirDovecotStore, MaildirDovecotLoadError and MaildirDovecotStoreError.

Documentation:

- Rewrote the src/lib.rs header as the crate architecture document and dropped the README include; the README and the header are two separate documents by design.
- Rewrote the README to the guideline shape: no code and no identifier references, Usage and Examples as redirects, the standard AI disclosure block, and the Contributing section.
- Rewrote CONTRIBUTING.md to the deviations-only template, documenting the feature matrix.
- Added this docs/ folder.
- Brought every public item to full rustdoc coverage: documented every coroutine constructor, error variant, output field and helper, and removed the blank lines between enum variants and struct fields.

Manifest:

- Fixed the package field order, enabled all-features on the docs.rs metadata, mixed the family keywords, added the no-std category and set default-features to false on every dependency.

Logging:

- Dropped the backticks from log messages, keeping them lowercase and prefix-free.
