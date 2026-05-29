//! Standard, blocking Maildir client.
//!
//! Holds a single filesystem root and exposes one method per
//! coroutine. Every method runs its coroutine to completion through
//! [`MaildirClient::run`] by performing the requested filesystem
//! operations via [`std::fs`].

use alloc::{
    collections::{BTreeMap, BTreeSet},
    string::{String, ToString},
    vec::Vec,
};
use std::{
    fs, io, process, thread,
    time::{SystemTime, UNIX_EPOCH},
};

use gethostname::gethostname;
use log::trace;
use thiserror::Error;

use crate::{
    coroutine::*,
    coroutines::{
        dovecot_load::*, dovecot_store::*, flags_add::*, flags_remove::*, flags_set::*,
        maildir_create::*, maildir_delete::*, maildir_list::*, maildir_rename::*, message_copy::*,
        message_get::*, message_list::*, message_locate::*, message_move::*, message_store::*,
    },
    entry::MaildirEntry,
    flag::{KeywordHeader, MaildirFlags},
    maildir::{CUR, Maildir, MaildirSubdir, NEW, TMP},
    message::MaildirMessage,
    path::MaildirPath,
};

/// Errors returned by the [`MaildirClient`] helpers.
#[derive(Debug, Error)]
pub enum MaildirClientError {
    #[error(transparent)]
    LoadMaildir(#[from] LoadMaildirError),

    #[error(transparent)]
    DovecotLoad(#[from] DovecotLoadError),
    #[error(transparent)]
    DovecotStore(#[from] DovecotStoreError),

    #[error(transparent)]
    FlagsAdd(#[from] MaildirFlagsAddError),
    #[error(transparent)]
    FlagsRemove(#[from] MaildirFlagsRemoveError),
    #[error(transparent)]
    FlagsSet(#[from] MaildirFlagsSetError),

    #[error(transparent)]
    MaildirCreate(#[from] MaildirCreateError),
    #[error(transparent)]
    MaildirDelete(#[from] MaildirDeleteError),
    #[error(transparent)]
    MaildirList(#[from] MaildirListError),
    #[error(transparent)]
    MaildirRename(#[from] MaildirRenameError),

    #[error(transparent)]
    MessageCopy(#[from] MaildirMessageCopyError),
    #[error(transparent)]
    MessageGet(#[from] MaildirMessageGetError),
    #[error(transparent)]
    MessageLocate(#[from] MaildirMessageLocateError),
    #[error(transparent)]
    MessagesList(#[from] MaildirMessagesListError),
    #[error(transparent)]
    MessageMove(#[from] MaildirMessageMoveError),
    #[error(transparent)]
    MessageStore(#[from] MaildirMessageStoreError),

    #[error(transparent)]
    Io(#[from] io::Error),
}

/// Errors returned when opening an existing Maildir on disk.
#[derive(Clone, Debug, Error)]
pub enum LoadMaildirError {
    #[error("path {0} is not a directory")]
    NotDir(MaildirPath),

    #[error("missing {0}/ subdirectory at Maildir {1}")]
    MissingSubdir(&'static str, MaildirPath),
}

/// Std-blocking Maildir client wrapping a filesystem root.
///
/// The four `pub` knobs customise how the client interprets and
/// serialises non-standard flag metadata (custom keywords) and folder
/// naming. Defaults preserve strict Maildir semantics: no
/// `dovecot-keywords` resolution, no header round-trip, no header
/// stripping and a flat namespace.
#[derive(Debug)]
pub struct MaildirClient {
    root: MaildirPath,
    /// Resolve / persist custom keywords via the `dovecot-keywords`
    /// file at each folder root (Dovecot / mbsync convention).
    pub dovecot_keywords: bool,
    /// Mirror custom keywords into a body header (`X-Keywords` or
    /// `X-Label`) on read, and inject them on write.
    pub keywords_header: Option<KeywordHeader>,
    /// Header names to remove from message bytes on read.
    pub strip_headers: Vec<String>,
    /// Treat the root as a Maildir++ store: enumerate dotted folder
    /// siblings and translate logical `Work/Foo` ↔ physical
    /// `.Work.Foo`.
    pub maildir_plus: bool,
    /// Logical name reported for the root Maildir in Maildir++ mode
    /// (the inbox). Defaults to `INBOX`. Ignored when `maildir_plus`
    /// is `false`.
    pub maildirpp_inbox: String,
    /// Treat the root as a Dovecot fs-layout store: subfolders are
    /// stored as nested filesystem directories (`Work/Foo/`) instead
    /// of flat dotted siblings. Mutually exclusive with `maildir_plus`.
    pub fs_layout: bool,
}

impl MaildirClient {
    /// Builds a client rooted at `root`. No filesystem check is
    /// performed at construction time.
    pub fn new(root: impl Into<MaildirPath>) -> Self {
        Self {
            root: root.into(),
            dovecot_keywords: false,
            keywords_header: None,
            strip_headers: Vec::new(),
            maildir_plus: false,
            maildirpp_inbox: String::from("INBOX"),
            fs_layout: false,
        }
    }

    /// Returns the filesystem root this client operates on.
    pub fn root(&self) -> &MaildirPath {
        &self.root
    }

    /// Drives any [`MaildirCoroutine`] to completion. The `handler`
    /// closure receives each non-terminal [`MaildirCoroutineState`]
    /// variant and returns the [`Arg`](MaildirCoroutine::Arg) to
    /// feed back. Variants the coroutine never emits can be matched
    /// with an `unreachable!()` arm.
    pub fn run<C, F>(
        &self,
        mut coroutine: C,
        mut handler: F,
    ) -> Result<C::Output, MaildirClientError>
    where
        C: MaildirCoroutine,
        MaildirClientError: From<C::Error>,
        F: FnMut(MaildirCoroutineState<C::Output, C::Error>) -> Result<C::Arg, MaildirClientError>,
    {
        let mut arg: Option<C::Arg> = None;
        loop {
            match coroutine.resume(arg.take()) {
                MaildirCoroutineState::Done(out) => return Ok(out),
                MaildirCoroutineState::Err(err) => return Err(err.into()),
                other => arg = Some(handler(other)?),
            }
        }
    }

    /// Runs [`DovecotLoad`] for `maildir`, returning the slot table
    /// when the `dovecot-keywords` file is present and an empty
    /// table otherwise.
    pub fn load_dovecot_keywords(
        &self,
        maildir: &Maildir,
    ) -> Result<BTreeMap<char, String>, MaildirClientError> {
        self.run(DovecotLoad::new(maildir), |state| match state {
            MaildirCoroutineState::WantsFileExists(paths) => {
                Ok(DovecotLoadArg::FileExists(file_exists(paths)))
            }
            MaildirCoroutineState::WantsFileRead(paths) => {
                Ok(DovecotLoadArg::FileRead(read_files(paths)?))
            }
            other => unreachable!("DovecotLoad yielded {other:?}"),
        })
    }

    /// Runs [`DovecotStore`] for `maildir` with the given table.
    pub fn store_dovecot_keywords(
        &self,
        maildir: &Maildir,
        table: &BTreeMap<char, String>,
    ) -> Result<(), MaildirClientError> {
        self.run(DovecotStore::new(maildir, table), |state| match state {
            MaildirCoroutineState::WantsFileCreate(files) => {
                write_files(files)?;
                Ok(DovecotStoreArg::FileCreate)
            }
            other => unreachable!("DovecotStore yielded {other:?}"),
        })
    }

    /// Opens an existing Maildir at `path`, validating that
    /// `cur`, `new`, and `tmp` are present.
    pub fn load_maildir(
        &self,
        path: impl Into<MaildirPath>,
    ) -> Result<Maildir, MaildirClientError> {
        load_maildir(path.into()).map_err(Into::into)
    }

    // ---- Maildir lifecycle --------------------------------------

    /// Runs [`MaildirCreate`]: creates the Maildir at `path`.
    pub fn create_maildir(&self, path: impl Into<MaildirPath>) -> Result<(), MaildirClientError> {
        self.run(MaildirCreate::new(path), |state| match state {
            MaildirCoroutineState::WantsDirCreate(paths) => {
                create_dirs(paths)?;
                Ok(MaildirCreateArg::DirCreate)
            }
            other => unreachable!("MaildirCreate yielded {other:?}"),
        })
    }

    /// Runs [`MaildirDelete`]: recursively removes the Maildir
    /// rooted at `path`.
    pub fn delete_maildir(&self, path: impl Into<MaildirPath>) -> Result<(), MaildirClientError> {
        self.run(MaildirDelete::new(path), |state| match state {
            MaildirCoroutineState::WantsDirRemove(paths) => {
                remove_dirs(paths)?;
                Ok(MaildirDeleteArg::DirRemove)
            }
            other => unreachable!("MaildirDelete yielded {other:?}"),
        })
    }

    /// Runs [`MaildirList`]: lists every valid Maildir directly
    /// under [`self.root`](Self::root).
    ///
    /// Dotted (`.`-prefixed) siblings are surfaced when
    /// [`Self::maildir_plus`] is set; the root itself is also probed
    /// in that mode so the inbox shows up alongside its children.
    pub fn list_maildirs(&self) -> Result<BTreeSet<Maildir>, MaildirClientError> {
        let coroutine = MaildirList::new(self.root.clone())
            .include_dotted(self.maildir_plus)
            .include_root(self.maildir_plus);
        self.run(coroutine, |state| match state {
            MaildirCoroutineState::WantsDirRead(paths) => {
                Ok(MaildirListArg::DirRead(read_dirs(paths)?))
            }
            MaildirCoroutineState::WantsDirExists(paths) => {
                Ok(MaildirListArg::DirExists(dir_exists(paths)))
            }
            other => unreachable!("MaildirList yielded {other:?}"),
        })
    }

    /// Runs [`MaildirRename`]: renames the Maildir at `path` to
    /// `name` (keeping the same parent directory).
    pub fn rename_maildir(
        &self,
        path: impl Into<MaildirPath>,
        name: impl ToString,
    ) -> Result<(), MaildirClientError> {
        self.run(MaildirRename::new(path, name), |state| match state {
            MaildirCoroutineState::WantsRename(pairs) => {
                rename_paths(pairs)?;
                Ok(MaildirRenameArg::Rename)
            }
            other => unreachable!("MaildirRename yielded {other:?}"),
        })
    }

    // ---- MaildirFlags --------------------------------------------------

    /// Runs [`MaildirFlagsAdd`]: adds `flags` to message `id` in
    /// `maildir`. Messages in `/new` or `/tmp` are left unchanged.
    ///
    /// When [`Self::dovecot_keywords`] is enabled, every
    /// [`MaildirFlag::Keyword`](crate::flag::MaildirFlag::Keyword)
    /// is resolved against the per-folder slot table (allocating a
    /// fresh slot when needed) and the resulting letter is appended
    /// to the filename. Otherwise, keyword variants are silently
    /// dropped.
    pub fn add_flags(
        &self,
        maildir: Maildir,
        id: impl ToString,
        mut flags: MaildirFlags,
    ) -> Result<(), MaildirClientError> {
        self.resolve_keywords(&maildir, &mut flags)?;
        self.run(
            MaildirFlagsAdd::new(maildir, id, flags),
            |state| match state {
                MaildirCoroutineState::WantsFileExists(paths) => {
                    Ok(MaildirFlagsAddArg::FileExists(file_exists(paths)))
                }
                MaildirCoroutineState::WantsDirRead(paths) => {
                    Ok(MaildirFlagsAddArg::DirRead(read_dirs(paths)?))
                }
                MaildirCoroutineState::WantsRename(pairs) => {
                    rename_paths(pairs)?;
                    Ok(MaildirFlagsAddArg::Rename)
                }
                other => unreachable!("MaildirFlagsAdd yielded {other:?}"),
            },
        )
    }

    /// Runs [`MaildirFlagsRemove`]: removes `flags` from message
    /// `id` in `maildir`. Messages in `/new` or `/tmp` are left
    /// unchanged.
    ///
    /// Keyword variants are resolved through the per-folder dovecot
    /// table (when [`Self::dovecot_keywords`] is set) so that already
    /// stored slot letters can be cleared.
    pub fn remove_flags(
        &self,
        maildir: Maildir,
        id: impl ToString,
        mut flags: MaildirFlags,
    ) -> Result<(), MaildirClientError> {
        self.resolve_keywords(&maildir, &mut flags)?;
        self.run(
            MaildirFlagsRemove::new(maildir, id, flags),
            |state| match state {
                MaildirCoroutineState::WantsFileExists(paths) => {
                    Ok(MaildirFlagsRemoveArg::FileExists(file_exists(paths)))
                }
                MaildirCoroutineState::WantsDirRead(paths) => {
                    Ok(MaildirFlagsRemoveArg::DirRead(read_dirs(paths)?))
                }
                MaildirCoroutineState::WantsRename(pairs) => {
                    rename_paths(pairs)?;
                    Ok(MaildirFlagsRemoveArg::Rename)
                }
                other => unreachable!("MaildirFlagsRemove yielded {other:?}"),
            },
        )
    }

    /// Runs [`MaildirFlagsSet`]: replaces the flags of message `id`
    /// in `maildir` with `flags`. Messages in `/new` or `/tmp` are
    /// left unchanged.
    ///
    /// Keyword variants are resolved through the per-folder dovecot
    /// table when [`Self::dovecot_keywords`] is set.
    pub fn set_flags(
        &self,
        maildir: Maildir,
        id: impl ToString,
        mut flags: MaildirFlags,
    ) -> Result<(), MaildirClientError> {
        self.resolve_keywords(&maildir, &mut flags)?;
        self.run(
            MaildirFlagsSet::new(maildir, id, flags),
            |state| match state {
                MaildirCoroutineState::WantsFileExists(paths) => {
                    Ok(MaildirFlagsSetArg::FileExists(file_exists(paths)))
                }
                MaildirCoroutineState::WantsDirRead(paths) => {
                    Ok(MaildirFlagsSetArg::DirRead(read_dirs(paths)?))
                }
                MaildirCoroutineState::WantsRename(pairs) => {
                    rename_paths(pairs)?;
                    Ok(MaildirFlagsSetArg::Rename)
                }
                other => unreachable!("MaildirFlagsSet yielded {other:?}"),
            },
        )
    }

    // ---- Messages -----------------------------------------------

    /// Runs [`MaildirMessageLocate`]: finds the on-disk path of
    /// message `id` inside `maildir` and returns its subdir and
    /// flags.
    pub fn locate(
        &self,
        maildir: Maildir,
        id: impl ToString,
    ) -> Result<(MaildirPath, MaildirSubdir, MaildirFlags), MaildirClientError> {
        let MaildirMessageLocateOk {
            path,
            subdir,
            flags,
        } = self.run(
            MaildirMessageLocate::new(maildir, id),
            |state| match state {
                MaildirCoroutineState::WantsFileExists(paths) => {
                    Ok(MaildirMessageLocateArg::FileExists(file_exists(paths)))
                }
                MaildirCoroutineState::WantsDirRead(paths) => {
                    Ok(MaildirMessageLocateArg::DirRead(read_dirs(paths)?))
                }
                other => unreachable!("MaildirMessageLocate yielded {other:?}"),
            },
        )?;
        Ok((path, subdir, flags))
    }

    /// Runs [`MaildirMessageGet`]: locates message `id` in
    /// `maildir` and reads its contents from disk.
    pub fn get(
        &self,
        maildir: Maildir,
        id: impl ToString,
    ) -> Result<MaildirMessage, MaildirClientError> {
        self.run(MaildirMessageGet::new(maildir, id), |state| match state {
            MaildirCoroutineState::WantsFileExists(paths) => {
                Ok(MaildirMessageGetArg::FileExists(file_exists(paths)))
            }
            MaildirCoroutineState::WantsDirRead(paths) => {
                Ok(MaildirMessageGetArg::DirRead(read_dirs(paths)?))
            }
            MaildirCoroutineState::WantsFileRead(paths) => {
                Ok(MaildirMessageGetArg::FileRead(read_files(paths)?))
            }
            other => unreachable!("MaildirMessageGet yielded {other:?}"),
        })
    }

    /// Runs [`MaildirMessagesList`]: scans both `/new` and `/cur`
    /// of `maildir` and returns every confirmed entry. Bodies are
    /// not loaded; pair with [`Self::read_entry`] /
    /// [`Self::read_entries`] / [`Self::read_entries_par`] to read
    /// contents.
    pub fn list_entries(
        &self,
        maildir: Maildir,
    ) -> Result<BTreeSet<MaildirEntry>, MaildirClientError> {
        self.run(MaildirMessagesList::new(maildir), |state| match state {
            MaildirCoroutineState::WantsDirRead(paths) => {
                Ok(MaildirMessagesListArg::DirRead(read_dirs(paths)?))
            }
            MaildirCoroutineState::WantsFileExists(paths) => {
                Ok(MaildirMessagesListArg::FileExists(file_exists(paths)))
            }
            other => unreachable!("MaildirMessagesList yielded {other:?}"),
        })
    }

    /// Reads the file backing `entry` and returns it as a
    /// [`MaildirMessage`].
    ///
    /// When [`Self::strip_headers`] is non-empty, the listed headers
    /// are removed from the returned bytes via
    /// [`crate::headers::strip_headers`].
    pub fn read_entry(&self, entry: &MaildirEntry) -> Result<MaildirMessage, MaildirClientError> {
        let path = entry.path();
        trace!("read entry at {path}");
        let contents = fs::read(path.as_str())?;
        let contents = if self.strip_headers.is_empty() {
            contents
        } else {
            let names: Vec<&str> = self.strip_headers.iter().map(String::as_str).collect();
            crate::headers::strip_headers(&contents, &names)
        };
        Ok(MaildirMessage::from((path.clone(), contents)))
    }

    /// Reads every entry sequentially.
    ///
    /// Returns an unordered set: callers that need a specific order
    /// must sort the result themselves. Use [`Self::read_entries_par`]
    /// for the parallel variant.
    pub fn read_entries(
        &self,
        entries: &[MaildirEntry],
    ) -> Result<BTreeSet<MaildirMessage>, MaildirClientError> {
        entries.iter().map(|entry| self.read_entry(entry)).collect()
    }

    /// Parallel variant of [`Self::read_entries`] backed by a
    /// `std::thread::scope` worker pool sized to
    /// [`thread::available_parallelism`].
    pub fn read_entries_par(
        &self,
        entries: &[MaildirEntry],
    ) -> Result<BTreeSet<MaildirMessage>, MaildirClientError> {
        if entries.len() <= 1 {
            return entries.iter().map(|entry| self.read_entry(entry)).collect();
        }

        let n_threads = thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(8)
            .min(entries.len());
        let chunk_size = entries.len().div_ceil(n_threads);

        thread::scope(
            |s| -> Result<BTreeSet<MaildirMessage>, MaildirClientError> {
                let mut handles = Vec::with_capacity(n_threads);

                for chunk in entries.chunks(chunk_size) {
                    let this = self;
                    handles.push(s.spawn(
                        move || -> Result<Vec<MaildirMessage>, MaildirClientError> {
                            chunk.iter().map(|entry| this.read_entry(entry)).collect()
                        },
                    ));
                }

                let mut out = BTreeSet::new();
                for handle in handles {
                    for msg in handle.join().expect("maildir worker thread panicked")? {
                        out.insert(msg);
                    }
                }
                Ok(out)
            },
        )
    }

    /// Runs [`MaildirMessageStore`]: writes `contents` to `tmp`
    /// then atomically renames it under `subdir` of `maildir` with
    /// the given `flags`. Returns the generated message id and
    /// final path.
    ///
    /// Behaviour adjustments controlled by [`Self::keywords_header`]
    /// and [`Self::dovecot_keywords`]:
    /// 1. when [`Self::keywords_header`] is `Some`, every
    ///    [`MaildirFlag::Keyword`](crate::flag::MaildirFlag::Keyword)
    ///    is injected into `contents` as a single header line;
    /// 2. when [`Self::dovecot_keywords`] is `true`, the per-folder
    ///    table is loaded, slots are allocated for the keywords
    ///    (with a `warn!` and drop on 26-slot overflow), the
    ///    resulting letters are appended to the filename info
    ///    section and the grown table is persisted.
    pub fn store(
        &self,
        maildir: Maildir,
        subdir: MaildirSubdir,
        mut flags: MaildirFlags,
        mut contents: Vec<u8>,
    ) -> Result<(String, MaildirPath), MaildirClientError> {
        let keywords = flags.drain_keywords();

        if let Some(header) = self.keywords_header {
            if !keywords.is_empty() {
                let sep = match header.separator() {
                    ',' => ", ",
                    ' ' => " ",
                    _ => ", ",
                };
                let value = keywords.join(sep);
                contents = crate::headers::inject_header(&contents, header.header_name(), &value);
            }
        }

        if self.dovecot_keywords && !keywords.is_empty() {
            let mut table = self.load_dovecot_keywords(&maildir)?;
            let original_len = table.len();
            for keyword in &keywords {
                match crate::headers::allocate_keyword_slot(&mut table, keyword) {
                    Some(letter) => {
                        flags.extend_letters([letter]);
                    }
                    None => {
                        log::warn!(
                            "dovecot-keywords table full; dropping keyword `{keyword}` at {}",
                            maildir.path()
                        );
                    }
                }
            }
            if table.len() != original_len {
                self.store_dovecot_keywords(&maildir, &table)?;
            }
        }

        let MaildirMessageStoreOk { id, path } = self.run(
            MaildirMessageStore::new(maildir, subdir, flags, contents),
            |state| match state {
                MaildirCoroutineState::WantsTime => {
                    let ts = SystemTime::now().duration_since(UNIX_EPOCH).unwrap();
                    Ok(MaildirMessageStoreArg::Time {
                        secs: ts.as_secs(),
                        nanos: ts.subsec_nanos(),
                    })
                }
                MaildirCoroutineState::WantsPid => Ok(MaildirMessageStoreArg::Pid(process::id())),
                MaildirCoroutineState::WantsHostname => {
                    let hostname = gethostname().into_string().unwrap_or_default();
                    Ok(MaildirMessageStoreArg::Hostname(hostname))
                }
                MaildirCoroutineState::WantsFileCreate(files) => {
                    write_files(files)?;
                    Ok(MaildirMessageStoreArg::FileCreate)
                }
                MaildirCoroutineState::WantsRename(pairs) => {
                    rename_paths(pairs)?;
                    Ok(MaildirMessageStoreArg::Rename)
                }
                other => unreachable!("MaildirMessageStore yielded {other:?}"),
            },
        )?;
        Ok((id, path))
    }

    /// Runs [`MaildirMessageCopy`]: copies message `id` from
    /// `source` into `target`.
    pub fn copy(
        &self,
        id: impl ToString,
        source: Maildir,
        target: Maildir,
        target_subdir: Option<MaildirSubdir>,
    ) -> Result<(), MaildirClientError> {
        self.run(
            MaildirMessageCopy::new(id, source, target, target_subdir),
            |state| match state {
                MaildirCoroutineState::WantsFileExists(paths) => {
                    Ok(MaildirMessageCopyArg::FileExists(file_exists(paths)))
                }
                MaildirCoroutineState::WantsDirRead(paths) => {
                    Ok(MaildirMessageCopyArg::DirRead(read_dirs(paths)?))
                }
                MaildirCoroutineState::WantsCopy(pairs) => {
                    copy_paths(pairs)?;
                    Ok(MaildirMessageCopyArg::Copy)
                }
                other => unreachable!("MaildirMessageCopy yielded {other:?}"),
            },
        )
    }

    /// Projects every [`MaildirFlag::Keyword`] out of `flags`,
    /// allocates a slot for it in the dovecot-keywords table (when
    /// [`Self::dovecot_keywords`] is set) and appends the slot
    /// letter to the filename. Keyword variants are dropped
    /// otherwise.
    ///
    /// [`MaildirFlag::Keyword`]: crate::flag::MaildirFlag::Keyword
    fn resolve_keywords(
        &self,
        maildir: &Maildir,
        flags: &mut MaildirFlags,
    ) -> Result<(), MaildirClientError> {
        let keywords = flags.drain_keywords();
        if !self.dovecot_keywords || keywords.is_empty() {
            return Ok(());
        }

        let mut table = self.load_dovecot_keywords(maildir)?;
        let original_len = table.len();

        for keyword in &keywords {
            match crate::headers::allocate_keyword_slot(&mut table, keyword) {
                Some(letter) => {
                    flags.extend_letters([letter]);
                }
                None => {
                    log::warn!(
                        "dovecot-keywords table full; dropping keyword `{keyword}` at {}",
                        maildir.path()
                    );
                }
            }
        }

        if table.len() != original_len {
            self.store_dovecot_keywords(maildir, &table)?;
        }

        Ok(())
    }

    /// Runs [`MaildirMessageMove`]: moves message `id` from
    /// `source` into `target`.
    pub fn r#move(
        &self,
        id: impl ToString,
        source: Maildir,
        target: Maildir,
        target_subdir: Option<MaildirSubdir>,
    ) -> Result<(), MaildirClientError> {
        self.run(
            MaildirMessageMove::new(id, source, target, target_subdir),
            |state| match state {
                MaildirCoroutineState::WantsFileExists(paths) => {
                    Ok(MaildirMessageMoveArg::FileExists(file_exists(paths)))
                }
                MaildirCoroutineState::WantsDirRead(paths) => {
                    Ok(MaildirMessageMoveArg::DirRead(read_dirs(paths)?))
                }
                MaildirCoroutineState::WantsRename(pairs) => {
                    rename_paths(pairs)?;
                    Ok(MaildirMessageMoveArg::Rename)
                }
                other => unreachable!("MaildirMessageMove yielded {other:?}"),
            },
        )
    }
}

// ---- Loaders ----------------------------------------------------

fn load_maildir(root: MaildirPath) -> Result<Maildir, LoadMaildirError> {
    if !std::path::Path::new(root.as_str()).is_dir() {
        return Err(LoadMaildirError::NotDir(root));
    }

    for sub in [CUR, NEW, TMP] {
        let path = root.join(sub);
        if !std::path::Path::new(path.as_str()).is_dir() {
            return Err(LoadMaildirError::MissingSubdir(sub, root));
        }
    }

    Ok(Maildir::from_path(root))
}

// ---- Path normalization -----------------------------------------

fn normalize_path(path: std::path::PathBuf) -> MaildirPath {
    let s = path.to_string_lossy().into_owned();
    #[cfg(windows)]
    let s = s.replace('\\', "/");
    MaildirPath::new(s)
}

// ---- Filesystem helpers -----------------------------------------

fn create_dirs(paths: BTreeSet<MaildirPath>) -> Result<(), io::Error> {
    for path in paths {
        trace!("create_dir_all {path}");
        fs::create_dir_all(path.as_str())?;
    }
    Ok(())
}

fn remove_dirs(paths: BTreeSet<MaildirPath>) -> Result<(), io::Error> {
    for path in paths {
        trace!("remove_dir_all {path}");
        fs::remove_dir_all(path.as_str())?;
    }
    Ok(())
}

fn write_files(files: BTreeMap<MaildirPath, Vec<u8>>) -> Result<(), io::Error> {
    for (path, contents) in files {
        trace!("write {path} ({} bytes)", contents.len());

        if let Some(parent) = std::path::Path::new(path.as_str()).parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(path.as_str(), &contents)?;
    }
    Ok(())
}

fn read_dirs(
    paths: BTreeSet<MaildirPath>,
) -> Result<BTreeMap<MaildirPath, BTreeSet<MaildirPath>>, io::Error> {
    let mut entries = BTreeMap::new();

    for path in paths {
        trace!("read_dir {path}");

        let mut names = BTreeSet::new();
        match fs::read_dir(path.as_str()) {
            Ok(iter) => {
                for entry in iter {
                    let entry = entry?;
                    names.insert(normalize_path(entry.path()));
                }
            }
            Err(err) if err.kind() == io::ErrorKind::NotFound => {}
            Err(err) => return Err(err),
        }

        entries.insert(path, names);
    }

    Ok(entries)
}

fn read_files(paths: BTreeSet<MaildirPath>) -> Result<BTreeMap<MaildirPath, Vec<u8>>, io::Error> {
    let mut contents = BTreeMap::new();

    for path in paths {
        trace!("read_file {path}");
        let bytes = fs::read(path.as_str())?;
        contents.insert(path, bytes);
    }

    Ok(contents)
}

fn rename_paths(pairs: Vec<(MaildirPath, MaildirPath)>) -> Result<(), io::Error> {
    for (from, to) in pairs {
        trace!("rename {from} -> {to}");
        fs::rename(from.as_str(), to.as_str())?;
    }
    Ok(())
}

fn copy_paths(pairs: Vec<(MaildirPath, MaildirPath)>) -> Result<(), io::Error> {
    for (from, to) in pairs {
        trace!("copy {from} -> {to}");
        fs::copy(from.as_str(), to.as_str())?;
    }
    Ok(())
}

fn file_exists(paths: BTreeSet<MaildirPath>) -> BTreeMap<MaildirPath, bool> {
    let mut out = BTreeMap::new();
    for path in paths {
        let exists = fs::metadata(path.as_str())
            .map(|m| m.is_file())
            .unwrap_or(false);
        trace!("file_exists {path}: {exists}");
        out.insert(path, exists);
    }
    out
}

fn dir_exists(paths: BTreeSet<MaildirPath>) -> BTreeMap<MaildirPath, bool> {
    let mut out = BTreeMap::new();
    for path in paths {
        let exists = fs::metadata(path.as_str())
            .map(|m| m.is_dir())
            .unwrap_or(false);
        trace!("dir_exists {path}: {exists}");
        out.insert(path, exists);
    }
    out
}
