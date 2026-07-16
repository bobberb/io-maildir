# Design

## I/O-free coroutines

Every Maildir operation is a resumable state machine that computes the next filesystem or environment operation and yields a request for it, instead of performing the operation itself. The caller performs the request and resumes the coroutine with the answer. This keeps the whole Maildir logic in a no_std core usable from a blocking driver, an async runtime or an in-memory test harness, with no I/O baked in.

The vocabulary lives in the coroutine module: the MaildirCoroutine trait, the MaildirYield request enum (filesystem create/read/exists/rename/copy/remove plus the time/pid/hostname environment inputs), the MaildirReply answer enum and the two-variant MaildirCoroutineState. The maildir_try! macro is the coroutine equivalent of the question-mark operator, forwarding yields and short-circuiting on error.

## Concepts as modules

The tree lives under maildir (the cur/new/tmp lifecycle: create, delete, list, rename), entries under entry (the delivery protocol and the store/get/list/locate/copy/move lifecycle), flags under flag (the info-suffix rewrite: add, remove, set) and the dovecot-keywords sidecar under dovecot. Each concept folder keeps its coroutines next to a sibling file holding the shared types they operate on.

## Path split and layout resolution

Two path types keep the logical and the physical apart. MaildirFsPath is the literal, forward-slash filesystem path. MaildirPath is the logical mailbox hierarchy. MaildirStore translates one into the other under its layout: fs (nested real directories, plain Maildir being the zero-subfolder degenerate case) or Maildir++ (the root is itself INBOX and subfolders are flat dot-prefixed siblings). The translation is a pure function, so the same coroutines drive both layouts.

## Keyword strategies

Maildir encodes only the six IANA flag letters in the filename. Custom keywords round-trip through one of two optional strategies, both driven by the std client: the dovecot-keywords sidecar mapping slot letters to keyword strings, or an inline X-Keywords / X-Label header injected into and stripped from the message body.
