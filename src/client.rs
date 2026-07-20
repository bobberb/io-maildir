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

    /// Drives any standard-shape coroutine (`Yield = MaildirYield`,
    /// `Return = Result<Output, Error>`) against the local filesystem
    /// until it terminates. Each [`MaildirYield`] variant is
    /// translated into the corresponding [`std::fs`] call (or env
    /// lookup) and its [`MaildirReply`] is fed back on the next
    /// resume.
    pub fn run<C, T, E>(&self, mut coroutine: C) -> Result<T, MaildirClientError>
    where
        C: MaildirCoroutine<Yield = MaildirYield, Return = Result<T, E>>,
        MaildirClientError: From<E>,
    {
        let mut arg: Option<MaildirReply> = None;

        loop {
            match coroutine.resume(arg.take()) {
                MaildirCoroutineState::Complete(Ok(out)) => return Ok(out),
                MaildirCoroutineState::Complete(Err(err)) => return Err(err.into()),
                MaildirCoroutineState::Yielded(MaildirYield::WantsFileExists(paths)) => {
                    arg = Some(MaildirReply::FileExists(file_exists(paths)));
                }
                MaildirCoroutineState::Yielded(MaildirYield::WantsDirExists(paths)) => {
                    arg = Some(MaildirReply::DirExists(dir_exists(paths)));
                }
                MaildirCoroutineState::Yielded(MaildirYield::WantsDirRead(paths)) => {
                    arg = Some(MaildirReply::DirRead(read_dirs(paths)?));
                }
                MaildirCoroutineState::Yielded(MaildirYield::WantsFileRead(paths)) => {
                    arg = Some(MaildirReply::FileRead(read_files(paths)?));
                }
                MaildirCoroutineState::Yielded(MaildirYield::WantsFileCreate(files)) => {
                    write_files(files)?;
                    arg = Some(MaildirReply::FileCreate);
                }
                MaildirCoroutineState::Yielded(MaildirYield::WantsDirCreate(paths)) => {
                    create_dirs(paths)?;
                    arg = Some(MaildirReply::DirCreate);
                }
                MaildirCoroutineState::Yielded(MaildirYield::WantsDirRemove(paths)) => {
                    remove_dirs(paths)?;
                    arg = Some(MaildirReply::DirRemove);
                }
                MaildirCoroutineState::Yielded(MaildirYield::WantsRename(pairs)) => {
                    rename_paths(pairs)?;
                    arg = Some(MaildirReply::Rename);
                }
                MaildirCoroutineState::Yielded(MaildirYield::WantsCopy(pairs)) => {
                    copy_paths(pairs)?;
                    arg = Some(MaildirReply::Copy);
                }
                MaildirCoroutineState::Yielded(MaildirYield::WantsTime) => {
                    let ts = SystemTime::now().duration_since(UNIX_EPOCH).unwrap();
                    arg = Some(MaildirReply::Time {
                        secs: ts.as_secs(),
                        nanos: ts.subsec_nanos(),
                    });
                }
                MaildirCoroutineState::Yielded(MaildirYield::WantsPid) => {
                    arg = Some(MaildirReply::Pid(process::id()));
                }
                MaildirCoroutineState::Yielded(MaildirYield::WantsHostname) => {
                    let hostname = gethostname().into_string().unwrap_or_default();
                    arg = Some(MaildirReply::Hostname(hostname));
                }
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
        self.run(DovecotLoad::new(maildir))
    }

    /// Runs [`DovecotStore`] for `maildir` with the given table.
    pub fn store_dovecot_keywords(
        &self,
        maildir: &Maildir,
        table: &BTreeMap<char, String>,
    ) -> Result<(), MaildirClientError> {
        self.run(DovecotStore::new(maildir, table))
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
        self.run(MaildirCreate::new(path))
    }

    /// Runs [`MaildirDelete`]: recursively removes the Maildir
    /// rooted at `path`.
    pub fn delete_maildir(&self, path: impl Into<MaildirPath>) -> Result<(), MaildirClientError> {
        self.run(MaildirDelete::new(path))
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
        self.run(coroutine)
    }

    /// Runs [`MaildirRename`]: renames the Maildir at `path` to
    /// `name` (keeping the same parent directory).
    pub fn rename_maildir(
        &self,
        path: impl Into<MaildirPath>,
        name: impl ToString,
    ) -> Result<(), MaildirClientError> {
        self.run(MaildirRename::new(path, name))
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
        self.run(MaildirFlagsAdd::new(maildir, id, flags))
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
        self.run(MaildirFlagsRemove::new(maildir, id, flags))
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
        self.run(MaildirFlagsSet::new(maildir, id, flags))
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
        let MaildirMessageLocateOutput {
            path,
            subdir,
            flags,
        } = self.run(MaildirMessageLocate::new(maildir, id))?;
        Ok((path, subdir, flags))
    }

    /// Runs [`MaildirMessageGet`]: locates message `id` in
    /// `maildir` and reads its contents from disk.
    pub fn get(
        &self,
        maildir: Maildir,
        id: impl ToString,
    ) -> Result<MaildirMessage, MaildirClientError> {
        self.run(MaildirMessageGet::new(maildir, id))
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
        self.run(MaildirMessagesList::new(maildir))
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
            let separator = header.separator();
            // A keyword containing the header separator would split into
            // multiple corrupted keywords on read-back; skip it rather
            // than emit a corruptible header. Dropping is acceptable
            // degradation; silent corruption is not.
            let safe: Vec<&String> = keywords
                .iter()
                .filter(|k| {
                    if k.contains(separator) {
                        log::warn!(
                            "keyword `{k}` contains header separator `{separator}`; \
                             dropping from {} header",
                            header.header_name()
                        );
                        false
                    } else {
                        true
                    }
                })
                .collect();

            if !safe.is_empty() {
                let sep = match separator {
                    ',' => ", ",
                    ' ' => " ",
                    _ => ", ",
                };
                let value = safe
                    .iter()
                    .map(|k| k.as_str())
                    .collect::<Vec<_>>()
                    .join(sep);
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

        let MaildirMessageStoreOutput { id, path } =
            self.run(MaildirMessageStore::new(maildir, subdir, flags, contents))?;
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
        self.run(MaildirMessageCopy::new(id, source, target, target_subdir))
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
        self.run(MaildirMessageMove::new(id, source, target, target_subdir))
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
