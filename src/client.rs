//! Standard blocking Maildir client driving any coroutine against [`std::fs`].

use alloc::{
    collections::{BTreeMap, BTreeSet},
    string::{String, ToString},
    vec::Vec,
};

use std::{
    fs, io,
    path::Path,
    process, thread,
    time::{SystemTime, UNIX_EPOCH},
};

use gethostname::gethostname;
use log::trace;
use thiserror::Error;

use crate::{
    coroutine::*,
    dovecot::{load::*, store::*, utils::allocate_keyword_slot},
    entry::{
        MaildirEntry, MaildirFullEntry,
        copy::*,
        get::*,
        headers::{inject_header, strip_headers},
        list::*,
        locate::*,
        r#move::*,
        store::*,
    },
    flag::{KeywordHeader, MaildirFlags, add::*, remove::*, set::*},
    maildir::{CUR, Maildir, MaildirSubdir, NEW, TMP, create::*, delete::*, list::*, rename::*},
    path::{MaildirFsPath, MaildirPath},
    store::MaildirStore,
};

/// Errors returned by the [`MaildirClient`] helpers.
#[derive(Debug, Error)]
pub enum MaildirClientError {
    /// The resolved path exists but is not a directory.
    #[error("path {0} is not a directory")]
    NotDir(MaildirFsPath),
    /// One of the cur/new/tmp subdirectories is missing.
    #[error("missing {0}/ subdirectory at Maildir {1}")]
    MissingSubdir(&'static str, MaildirFsPath),
    /// The dovecot-keywords load coroutine failed.
    #[error(transparent)]
    MaildirDovecotLoad(#[from] MaildirDovecotLoadError),
    /// The dovecot-keywords store coroutine failed.
    #[error(transparent)]
    MaildirDovecotStore(#[from] MaildirDovecotStoreError),
    /// The flags-add coroutine failed.
    #[error(transparent)]
    FlagsAdd(#[from] MaildirFlagsAddError),
    /// The flags-remove coroutine failed.
    #[error(transparent)]
    FlagsRemove(#[from] MaildirFlagsRemoveError),
    /// The flags-set coroutine failed.
    #[error(transparent)]
    FlagsSet(#[from] MaildirFlagsSetError),
    /// The Maildir-create coroutine failed.
    #[error(transparent)]
    MaildirCreate(#[from] MaildirCreateError),
    /// The Maildir-delete coroutine failed.
    #[error(transparent)]
    MaildirDelete(#[from] MaildirDeleteError),
    /// The Maildir-list coroutine failed.
    #[error(transparent)]
    MaildirList(#[from] MaildirListError),
    /// The Maildir-rename coroutine failed.
    #[error(transparent)]
    MaildirRename(#[from] MaildirRenameError),
    /// The entry-copy coroutine failed.
    #[error(transparent)]
    EntryCopy(#[from] MaildirEntryCopyError),
    /// The entry-get coroutine failed.
    #[error(transparent)]
    EntryGet(#[from] MaildirEntryGetError),
    /// The entry-locate coroutine failed.
    #[error(transparent)]
    EntryLocate(#[from] MaildirEntryLocateError),
    /// The entry-list coroutine failed.
    #[error(transparent)]
    EntryList(#[from] MaildirEntryListError),
    /// The entry-move coroutine failed.
    #[error(transparent)]
    EntryMove(#[from] MaildirEntryMoveError),
    /// The entry-store coroutine failed.
    #[error(transparent)]
    EntryStore(#[from] MaildirEntryStoreError),
    /// A filesystem operation failed.
    #[error(transparent)]
    Io(#[from] io::Error),
}

/// Std-blocking Maildir client. Wraps a [`MaildirStore`] (filesystem
/// root + layout) and drives any [`MaildirCoroutine`] against
/// [`std::fs`].
#[derive(Debug)]
pub struct MaildirClient {
    /// Filesystem root + layout (fs / Maildir++).
    pub store: MaildirStore,
    /// Resolve and persist custom keywords via the `dovecot-keywords` sidecar.
    pub dovecot_keywords: bool,
    /// Header used to ferry custom keywords inline with the body.
    pub keywords_header: Option<KeywordHeader>,
    /// Header names to strip from message bytes on read.
    pub strip_headers: Vec<String>,
}

impl MaildirClient {
    /// Builds a client rooted at `root` in fs layout without filesystem
    /// checks. Flip `client.store.maildirpp = true` for Maildir++.
    pub fn new(root: impl Into<MaildirFsPath>) -> Self {
        Self {
            store: MaildirStore {
                root: root.into(),
                maildirpp: false,
            },
            dovecot_keywords: false,
            keywords_header: None,
            strip_headers: Vec::new(),
        }
    }

    /// Drives any standard-shape coroutine to completion against [`std::fs`].
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
                    let mut out = BTreeMap::new();
                    for path in paths {
                        let exists = fs::metadata(path.as_str())
                            .map(|m| m.is_file())
                            .unwrap_or(false);
                        trace!("file_exists {path}: {exists}");
                        out.insert(path, exists);
                    }
                    arg = Some(MaildirReply::FileExists(out));
                }
                MaildirCoroutineState::Yielded(MaildirYield::WantsDirExists(paths)) => {
                    let mut out = BTreeMap::new();
                    for path in paths {
                        let exists = fs::metadata(path.as_str())
                            .map(|m| m.is_dir())
                            .unwrap_or(false);
                        trace!("dir_exists {path}: {exists}");
                        out.insert(path, exists);
                    }
                    arg = Some(MaildirReply::DirExists(out));
                }
                MaildirCoroutineState::Yielded(MaildirYield::WantsDirRead(paths)) => {
                    let mut entries = BTreeMap::new();
                    for path in paths {
                        trace!("read_dir {path}");
                        let mut names = BTreeSet::new();
                        match fs::read_dir(path.as_str()) {
                            Ok(iter) => {
                                for entry in iter {
                                    names.insert(MaildirFsPath::from(entry?.path()));
                                }
                            }
                            Err(err) if err.kind() == io::ErrorKind::NotFound => {}
                            Err(err) => return Err(err.into()),
                        }
                        entries.insert(path, names);
                    }
                    arg = Some(MaildirReply::DirRead(entries));
                }
                MaildirCoroutineState::Yielded(MaildirYield::WantsFileRead(paths)) => {
                    let mut contents = BTreeMap::new();
                    for path in paths {
                        trace!("read_file {path}");
                        let bytes = fs::read(path.as_str())?;
                        contents.insert(path, bytes);
                    }
                    arg = Some(MaildirReply::FileRead(contents));
                }
                MaildirCoroutineState::Yielded(MaildirYield::WantsFileCreate(files)) => {
                    for (path, contents) in files {
                        trace!("write {path} ({} bytes)", contents.len());
                        if let Some(parent) = Path::new(path.as_str()).parent() {
                            fs::create_dir_all(parent)?;
                        }
                        fs::write(path.as_str(), &contents)?;
                    }
                    arg = Some(MaildirReply::FileCreate);
                }
                MaildirCoroutineState::Yielded(MaildirYield::WantsDirCreate(paths)) => {
                    for path in paths {
                        trace!("create_dir_all {path}");
                        fs::create_dir_all(path.as_str())?;
                    }
                    arg = Some(MaildirReply::DirCreate);
                }
                MaildirCoroutineState::Yielded(MaildirYield::WantsDirRemove(paths)) => {
                    for path in paths {
                        trace!("remove_dir_all {path}");
                        fs::remove_dir_all(path.as_str())?;
                    }
                    arg = Some(MaildirReply::DirRemove);
                }
                MaildirCoroutineState::Yielded(MaildirYield::WantsRename(pairs)) => {
                    for (from, to) in pairs {
                        trace!("rename {from} to {to}");
                        fs::rename(from.as_str(), to.as_str())?;
                    }
                    arg = Some(MaildirReply::Rename);
                }
                MaildirCoroutineState::Yielded(MaildirYield::WantsCopy(pairs)) => {
                    for (from, to) in pairs {
                        trace!("copy {from} to {to}");
                        fs::copy(from.as_str(), to.as_str())?;
                    }
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

    /// Runs [`MaildirDovecotLoad`] for `maildir`.
    pub fn load_dovecot_keywords(
        &self,
        maildir: &Maildir,
    ) -> Result<BTreeMap<char, String>, MaildirClientError> {
        self.run(MaildirDovecotLoad::new(maildir))
    }

    /// Runs [`MaildirDovecotStore`] for `maildir` with the given table.
    pub fn store_dovecot_keywords(
        &self,
        maildir: &Maildir,
        table: &BTreeMap<char, String>,
    ) -> Result<(), MaildirClientError> {
        self.run(MaildirDovecotStore::new(maildir, table))
    }

    /// Opens an existing Maildir named `name`, resolving the logical path
    /// through the store and validating its cur/new/tmp subdirs.
    pub fn load_maildir(
        &self,
        name: impl Into<MaildirPath>,
    ) -> Result<Maildir, MaildirClientError> {
        let root = self.store.resolve(&name.into());

        if !Path::new(root.as_str()).is_dir() {
            return Err(MaildirClientError::NotDir(root));
        }

        for sub in [CUR, NEW, TMP] {
            let path = root.join(sub);

            if !Path::new(path.as_str()).is_dir() {
                return Err(MaildirClientError::MissingSubdir(sub, root));
            }
        }

        Ok(Maildir::from_path(root))
    }
}

/// Maildir lifecycle helpers: create, delete, list and rename whole
/// Maildirs by logical mailbox name.
impl MaildirClient {
    /// Runs [`MaildirCreate`] for the logical mailbox `name`.
    pub fn create_maildir(&self, name: impl Into<MaildirPath>) -> Result<(), MaildirClientError> {
        self.run(MaildirCreate::new(&self.store, name.into()))
    }

    /// Runs [`MaildirDelete`] for the logical mailbox `name`.
    pub fn delete_maildir(&self, name: impl Into<MaildirPath>) -> Result<(), MaildirClientError> {
        self.run(MaildirDelete::new(&self.store, name.into()))
    }

    /// Runs [`MaildirList`] under the store root; honours the store's
    /// layout flag.
    pub fn list_maildirs(&self) -> Result<BTreeSet<Maildir>, MaildirClientError> {
        self.run(MaildirList::new(&self.store))
    }

    /// Runs [`MaildirRename`] from `from` to `to`, both logical mailbox
    /// names resolved through the store.
    pub fn rename_maildir(
        &self,
        from: impl Into<MaildirPath>,
        to: impl Into<MaildirPath>,
    ) -> Result<(), MaildirClientError> {
        self.run(MaildirRename::new(&self.store, from.into(), to.into()))
    }
}

/// Flag helpers: add, remove and set flags on an entry, resolving
/// custom keywords through the configured strategy.
impl MaildirClient {
    /// Runs [`MaildirFlagsAdd`] for `id` in `maildir`; resolves keywords
    /// through [`Self::dovecot_keywords`] if set.
    pub fn add_flags(
        &self,
        maildir: Maildir,
        id: impl ToString,
        mut flags: MaildirFlags,
    ) -> Result<(), MaildirClientError> {
        self.resolve_keywords(&maildir, &mut flags)?;
        self.run(MaildirFlagsAdd::new(maildir, id, flags))
    }

    /// Runs [`MaildirFlagsRemove`] for `id` in `maildir`; resolves keywords
    /// through [`Self::dovecot_keywords`] if set.
    pub fn remove_flags(
        &self,
        maildir: Maildir,
        id: impl ToString,
        mut flags: MaildirFlags,
    ) -> Result<(), MaildirClientError> {
        self.resolve_keywords(&maildir, &mut flags)?;
        self.run(MaildirFlagsRemove::new(maildir, id, flags))
    }

    /// Runs [`MaildirFlagsSet`] for `id` in `maildir`; resolves keywords
    /// through [`Self::dovecot_keywords`] if set.
    pub fn set_flags(
        &self,
        maildir: Maildir,
        id: impl ToString,
        mut flags: MaildirFlags,
    ) -> Result<(), MaildirClientError> {
        self.resolve_keywords(&maildir, &mut flags)?;
        self.run(MaildirFlagsSet::new(maildir, id, flags))
    }
}

/// Entry helpers: locate, read, list, store, copy and move entries.
impl MaildirClient {
    /// Runs [`MaildirEntryLocate`] for `id` in `maildir`.
    pub fn locate(
        &self,
        maildir: Maildir,
        id: impl ToString,
    ) -> Result<(MaildirFsPath, MaildirSubdir, MaildirFlags), MaildirClientError> {
        let MaildirEntryLocateOutput {
            path,
            subdir,
            flags,
        } = self.run(MaildirEntryLocate::new(maildir, id))?;
        Ok((path, subdir, flags))
    }

    /// Runs [`MaildirEntryGet`] for `id` in `maildir`.
    pub fn get(
        &self,
        maildir: Maildir,
        id: impl ToString,
    ) -> Result<MaildirFullEntry, MaildirClientError> {
        self.run(MaildirEntryGet::new(maildir, id))
    }

    /// Runs [`MaildirEntryList`] on `maildir`; bodies not loaded (pair with
    /// [`Self::read_entry`] / [`Self::read_entries`] /
    /// [`Self::read_entries_par`]).
    pub fn list_entries(
        &self,
        maildir: Maildir,
    ) -> Result<BTreeSet<MaildirEntry>, MaildirClientError> {
        self.run(MaildirEntryList::new(maildir))
    }

    /// Reads `entry`'s file as a [`MaildirFullEntry`]; applies
    /// [`Self::strip_headers`] when set.
    pub fn read_entry(&self, entry: &MaildirEntry) -> Result<MaildirFullEntry, MaildirClientError> {
        let path = entry.path();
        trace!("read entry at {path}");
        let contents = fs::read(path.as_str())?;
        let contents = if self.strip_headers.is_empty() {
            contents
        } else {
            let names: Vec<&str> = self.strip_headers.iter().map(String::as_str).collect();
            strip_headers(&contents, &names)
        };
        Ok(MaildirFullEntry::from((path.clone(), contents)))
    }

    /// Reads every entry sequentially into an unordered set.
    pub fn read_entries(
        &self,
        entries: &[MaildirEntry],
    ) -> Result<BTreeSet<MaildirFullEntry>, MaildirClientError> {
        entries.iter().map(|entry| self.read_entry(entry)).collect()
    }

    /// Parallel variant of [`Self::read_entries`] using
    /// [`thread::available_parallelism`].
    pub fn read_entries_par(
        &self,
        entries: &[MaildirEntry],
    ) -> Result<BTreeSet<MaildirFullEntry>, MaildirClientError> {
        if entries.len() <= 1 {
            return entries.iter().map(|entry| self.read_entry(entry)).collect();
        }

        let n_threads = thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(8)
            .min(entries.len());

        let chunk_size = entries.len().div_ceil(n_threads);

        thread::scope(
            |s| -> Result<BTreeSet<MaildirFullEntry>, MaildirClientError> {
                let mut handles = Vec::with_capacity(n_threads);

                for chunk in entries.chunks(chunk_size) {
                    let this = self;
                    handles.push(s.spawn(
                        move || -> Result<Vec<MaildirFullEntry>, MaildirClientError> {
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

    /// Runs [`MaildirEntryStore`] under `subdir` of `maildir`; honours
    /// [`Self::keywords_header`] and [`Self::dovecot_keywords`] for keyword
    /// serialisation.
    pub fn store(
        &self,
        maildir: Maildir,
        subdir: MaildirSubdir,
        mut flags: MaildirFlags,
        mut contents: Vec<u8>,
    ) -> Result<(String, MaildirFsPath), MaildirClientError> {
        let keywords = flags.drain_keywords();

        if let Some(header) = self.keywords_header {
            if !keywords.is_empty() {
                let sep = match header.separator() {
                    ' ' => " ",
                    _ => ", ",
                };
                let value = keywords.join(sep);
                contents = inject_header(&contents, header.header_name(), &value);
            }
        }

        if self.dovecot_keywords && !keywords.is_empty() {
            let mut table = self.load_dovecot_keywords(&maildir)?;
            let original_len = table.len();
            for keyword in &keywords {
                match allocate_keyword_slot(&mut table, keyword) {
                    Some(letter) => {
                        flags.extend_letters([letter]);
                    }
                    None => {
                        log::warn!(
                            "dovecot-keywords table full, dropping keyword {keyword:?} at {}",
                            maildir.path()
                        );
                    }
                }
            }
            if table.len() != original_len {
                self.store_dovecot_keywords(&maildir, &table)?;
            }
        }

        let MaildirEntryStoreOutput { id, path } =
            self.run(MaildirEntryStore::new(maildir, subdir, flags, contents))?;

        Ok((id, path))
    }

    /// Runs [`MaildirEntryCopy`] from `source` to `target`.
    pub fn copy(
        &self,
        id: impl ToString,
        source: Maildir,
        target: Maildir,
        target_subdir: Option<MaildirSubdir>,
    ) -> Result<(), MaildirClientError> {
        self.run(MaildirEntryCopy::new(id, source, target, target_subdir))
    }

    /// Runs [`MaildirEntryMove`] from `source` to `target`.
    pub fn r#move(
        &self,
        id: impl ToString,
        source: Maildir,
        target: Maildir,
        target_subdir: Option<MaildirSubdir>,
    ) -> Result<(), MaildirClientError> {
        self.run(MaildirEntryMove::new(id, source, target, target_subdir))
    }

    /// Drains every keyword out of `flags`, allocating dovecot slots when
    /// [`Self::dovecot_keywords`] is set; drops them otherwise.
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
            match allocate_keyword_slot(&mut table, keyword) {
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
}
