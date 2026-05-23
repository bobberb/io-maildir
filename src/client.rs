//! Standard, blocking Maildir client.
//!
//! Holds a single filesystem root and exposes one method per
//! coroutine. Every method runs its coroutine to completion by
//! performing the requested filesystem operations via [`std::fs`]
//! in a resume loop.

use alloc::{
    collections::{BTreeMap, BTreeSet},
    string::{String, ToString},
    vec::Vec,
};
use std::{
    fs, io, process,
    time::{SystemTime, UNIX_EPOCH},
};

use gethostname::gethostname;
use log::trace;
use thiserror::Error;

use crate::{
    coroutines::{
        flags_add::*, flags_remove::*, flags_set::*, maildir_create::*, maildir_delete::*,
        maildir_list::*, maildir_rename::*, message_copy::*, message_get::*, message_list::*,
        message_locate::*, message_move::*, message_store::*,
    },
    flag::Flags,
    maildir::{CUR, Maildir, MaildirSubdir, NEW, TMP},
    message::Message,
    path::MaildirPath,
};

/// Errors returned by the [`MaildirClient`] helpers.
#[derive(Debug, Error)]
pub enum MaildirClientError {
    #[error(transparent)]
    LoadMaildir(#[from] LoadMaildirError),

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
#[derive(Debug)]
pub struct MaildirClient {
    root: MaildirPath,
}

impl MaildirClient {
    /// Builds a client rooted at `root`. No filesystem check is
    /// performed at construction time.
    pub fn new(root: impl Into<MaildirPath>) -> Self {
        Self { root: root.into() }
    }

    /// Returns the filesystem root this client operates on.
    pub fn root(&self) -> &MaildirPath {
        &self.root
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
        let mut coroutine = MaildirCreate::new(path);
        let mut arg: Option<MaildirCreateArg> = None;

        loop {
            match coroutine.resume(arg.take()) {
                MaildirCreateResult::Ok => return Ok(()),
                MaildirCreateResult::WantsDirCreate(paths) => {
                    create_dirs(paths)?;
                    arg = Some(MaildirCreateArg::DirCreate);
                }
                MaildirCreateResult::Err(err) => return Err(err.into()),
            }
        }
    }

    /// Runs [`MaildirDelete`]: recursively removes the Maildir
    /// rooted at `path`.
    pub fn delete_maildir(&self, path: impl Into<MaildirPath>) -> Result<(), MaildirClientError> {
        let mut coroutine = MaildirDelete::new(path);
        let mut arg: Option<MaildirDeleteArg> = None;

        loop {
            match coroutine.resume(arg.take()) {
                MaildirDeleteResult::Ok => return Ok(()),
                MaildirDeleteResult::WantsDirRemove(paths) => {
                    remove_dirs(paths)?;
                    arg = Some(MaildirDeleteArg::DirRemove);
                }
                MaildirDeleteResult::Err(err) => return Err(err.into()),
            }
        }
    }

    /// Runs [`MaildirList`]: lists every valid Maildir directly
    /// under [`self.root`](Self::root).
    pub fn list_maildirs(&self) -> Result<BTreeSet<Maildir>, MaildirClientError> {
        let mut coroutine = MaildirList::new(self.root.clone());
        let mut arg: Option<MaildirListArg> = None;

        loop {
            match coroutine.resume(arg.take()) {
                MaildirListResult::Ok(maildirs) => return Ok(maildirs),
                MaildirListResult::WantsDirRead(paths) => {
                    arg = Some(MaildirListArg::DirRead(read_dirs(paths)?));
                }
                MaildirListResult::WantsDirExists(paths) => {
                    arg = Some(MaildirListArg::DirExists(dir_exists(paths)));
                }
                MaildirListResult::Err(err) => return Err(err.into()),
            }
        }
    }

    /// Runs [`MaildirRename`]: renames the Maildir at `path` to
    /// `name` (keeping the same parent directory).
    pub fn rename_maildir(
        &self,
        path: impl Into<MaildirPath>,
        name: impl ToString,
    ) -> Result<(), MaildirClientError> {
        let mut coroutine = MaildirRename::new(path, name);
        let mut arg: Option<MaildirRenameArg> = None;

        loop {
            match coroutine.resume(arg.take()) {
                MaildirRenameResult::Ok => return Ok(()),
                MaildirRenameResult::WantsRename(pairs) => {
                    rename_paths(pairs)?;
                    arg = Some(MaildirRenameArg::Rename);
                }
                MaildirRenameResult::Err(err) => return Err(err.into()),
            }
        }
    }

    // ---- Flags --------------------------------------------------

    /// Runs [`MaildirFlagsAdd`]: adds `flags` to message `id` in
    /// `maildir`. Messages in `/new` or `/tmp` are left unchanged.
    pub fn add_flags(
        &self,
        maildir: Maildir,
        id: impl ToString,
        flags: Flags,
    ) -> Result<(), MaildirClientError> {
        let mut coroutine = MaildirFlagsAdd::new(maildir, id, flags);
        let mut arg: Option<MaildirFlagsAddArg> = None;

        loop {
            match coroutine.resume(arg.take()) {
                MaildirFlagsAddResult::Ok => return Ok(()),
                MaildirFlagsAddResult::WantsFileExists(paths) => {
                    arg = Some(MaildirFlagsAddArg::FileExists(file_exists(paths)));
                }
                MaildirFlagsAddResult::WantsDirRead(paths) => {
                    arg = Some(MaildirFlagsAddArg::DirRead(read_dirs(paths)?));
                }
                MaildirFlagsAddResult::WantsRename(pairs) => {
                    rename_paths(pairs)?;
                    arg = Some(MaildirFlagsAddArg::Rename);
                }
                MaildirFlagsAddResult::Err(err) => return Err(err.into()),
            }
        }
    }

    /// Runs [`MaildirFlagsRemove`]: removes `flags` from message
    /// `id` in `maildir`. Messages in `/new` or `/tmp` are left
    /// unchanged.
    pub fn remove_flags(
        &self,
        maildir: Maildir,
        id: impl ToString,
        flags: Flags,
    ) -> Result<(), MaildirClientError> {
        let mut coroutine = MaildirFlagsRemove::new(maildir, id, flags);
        let mut arg: Option<MaildirFlagsRemoveArg> = None;

        loop {
            match coroutine.resume(arg.take()) {
                MaildirFlagsRemoveResult::Ok => return Ok(()),
                MaildirFlagsRemoveResult::WantsFileExists(paths) => {
                    arg = Some(MaildirFlagsRemoveArg::FileExists(file_exists(paths)));
                }
                MaildirFlagsRemoveResult::WantsDirRead(paths) => {
                    arg = Some(MaildirFlagsRemoveArg::DirRead(read_dirs(paths)?));
                }
                MaildirFlagsRemoveResult::WantsRename(pairs) => {
                    rename_paths(pairs)?;
                    arg = Some(MaildirFlagsRemoveArg::Rename);
                }
                MaildirFlagsRemoveResult::Err(err) => return Err(err.into()),
            }
        }
    }

    /// Runs [`MaildirFlagsSet`]: replaces the flags of message `id`
    /// in `maildir` with `flags`. Messages in `/new` or `/tmp` are
    /// left unchanged.
    pub fn set_flags(
        &self,
        maildir: Maildir,
        id: impl ToString,
        flags: Flags,
    ) -> Result<(), MaildirClientError> {
        let mut coroutine = MaildirFlagsSet::new(maildir, id, flags);
        let mut arg: Option<MaildirFlagsSetArg> = None;

        loop {
            match coroutine.resume(arg.take()) {
                MaildirFlagsSetResult::Ok => return Ok(()),
                MaildirFlagsSetResult::WantsFileExists(paths) => {
                    arg = Some(MaildirFlagsSetArg::FileExists(file_exists(paths)));
                }
                MaildirFlagsSetResult::WantsDirRead(paths) => {
                    arg = Some(MaildirFlagsSetArg::DirRead(read_dirs(paths)?));
                }
                MaildirFlagsSetResult::WantsRename(pairs) => {
                    rename_paths(pairs)?;
                    arg = Some(MaildirFlagsSetArg::Rename);
                }
                MaildirFlagsSetResult::Err(err) => return Err(err.into()),
            }
        }
    }

    // ---- Messages -----------------------------------------------

    /// Runs [`MaildirMessageLocate`]: finds the on-disk path of
    /// message `id` inside `maildir` and returns its subdir and
    /// flags.
    pub fn locate(
        &self,
        maildir: Maildir,
        id: impl ToString,
    ) -> Result<(MaildirPath, MaildirSubdir, Flags), MaildirClientError> {
        let mut coroutine = MaildirMessageLocate::new(maildir, id);
        let mut arg: Option<MaildirMessageLocateArg> = None;

        loop {
            match coroutine.resume(arg.take()) {
                MaildirMessageLocateResult::Ok {
                    path,
                    subdir,
                    flags,
                } => return Ok((path, subdir, flags)),
                MaildirMessageLocateResult::WantsFileExists(paths) => {
                    arg = Some(MaildirMessageLocateArg::FileExists(file_exists(paths)));
                }
                MaildirMessageLocateResult::WantsDirRead(paths) => {
                    arg = Some(MaildirMessageLocateArg::DirRead(read_dirs(paths)?));
                }
                MaildirMessageLocateResult::Err(err) => return Err(err.into()),
            }
        }
    }

    /// Runs [`MaildirMessageGet`]: locates message `id` in
    /// `maildir` and reads its contents from disk.
    pub fn get(&self, maildir: Maildir, id: impl ToString) -> Result<Message, MaildirClientError> {
        let mut coroutine = MaildirMessageGet::new(maildir, id);
        let mut arg: Option<MaildirMessageGetArg> = None;

        loop {
            match coroutine.resume(arg.take()) {
                MaildirMessageGetResult::Ok(message) => return Ok(message),
                MaildirMessageGetResult::WantsFileExists(paths) => {
                    arg = Some(MaildirMessageGetArg::FileExists(file_exists(paths)));
                }
                MaildirMessageGetResult::WantsDirRead(paths) => {
                    arg = Some(MaildirMessageGetArg::DirRead(read_dirs(paths)?));
                }
                MaildirMessageGetResult::WantsFileRead(paths) => {
                    arg = Some(MaildirMessageGetArg::FileRead(read_files(paths)?));
                }
                MaildirMessageGetResult::Err(err) => return Err(err.into()),
            }
        }
    }

    /// Runs [`MaildirMessagesList`]: scans both `/new` and `/cur`
    /// of `maildir` and returns every message it finds.
    pub fn list_messages(&self, maildir: Maildir) -> Result<BTreeSet<Message>, MaildirClientError> {
        let mut coroutine = MaildirMessagesList::new(maildir);
        let mut arg: Option<MaildirMessagesListArg> = None;

        loop {
            match coroutine.resume(arg.take()) {
                MaildirMessagesListResult::Ok(messages) => return Ok(messages),
                MaildirMessagesListResult::WantsDirRead(paths) => {
                    arg = Some(MaildirMessagesListArg::DirRead(read_dirs(paths)?));
                }
                MaildirMessagesListResult::WantsFileExists(paths) => {
                    arg = Some(MaildirMessagesListArg::FileExists(file_exists(paths)));
                }
                MaildirMessagesListResult::WantsFileRead(paths) => {
                    arg = Some(MaildirMessagesListArg::FileRead(read_files(paths)?));
                }
                MaildirMessagesListResult::Err(err) => return Err(err.into()),
            }
        }
    }

    /// Runs [`MaildirMessageStore`]: writes `contents` to `tmp`
    /// then atomically renames it under `subdir` of `maildir` with
    /// the given `flags`. Returns the generated message id and
    /// final path.
    pub fn store(
        &self,
        maildir: Maildir,
        subdir: MaildirSubdir,
        flags: Flags,
        contents: Vec<u8>,
    ) -> Result<(String, MaildirPath), MaildirClientError> {
        let mut coroutine = MaildirMessageStore::new(maildir, subdir, flags, contents);
        let mut arg: Option<MaildirMessageStoreArg> = None;

        loop {
            match coroutine.resume(arg.take()) {
                MaildirMessageStoreResult::Ok { id, path } => return Ok((id, path)),
                MaildirMessageStoreResult::WantsTime => {
                    let ts = SystemTime::now().duration_since(UNIX_EPOCH).unwrap();
                    arg = Some(MaildirMessageStoreArg::Time {
                        secs: ts.as_secs(),
                        nanos: ts.subsec_nanos(),
                    });
                }
                MaildirMessageStoreResult::WantsPid => {
                    arg = Some(MaildirMessageStoreArg::Pid(process::id()));
                }
                MaildirMessageStoreResult::WantsHostname => {
                    let hostname = gethostname().into_string().unwrap_or_default();
                    arg = Some(MaildirMessageStoreArg::Hostname(hostname));
                }
                MaildirMessageStoreResult::WantsFileCreate(files) => {
                    write_files(files)?;
                    arg = Some(MaildirMessageStoreArg::FileCreate);
                }
                MaildirMessageStoreResult::WantsRename(pairs) => {
                    rename_paths(pairs)?;
                    arg = Some(MaildirMessageStoreArg::Rename);
                }
                MaildirMessageStoreResult::Err(err) => return Err(err.into()),
            }
        }
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
        let mut coroutine = MaildirMessageCopy::new(id, source, target, target_subdir);
        let mut arg: Option<MaildirMessageCopyArg> = None;

        loop {
            match coroutine.resume(arg.take()) {
                MaildirMessageCopyResult::Ok => return Ok(()),
                MaildirMessageCopyResult::WantsFileExists(paths) => {
                    arg = Some(MaildirMessageCopyArg::FileExists(file_exists(paths)));
                }
                MaildirMessageCopyResult::WantsDirRead(paths) => {
                    arg = Some(MaildirMessageCopyArg::DirRead(read_dirs(paths)?));
                }
                MaildirMessageCopyResult::WantsCopy(pairs) => {
                    copy_paths(pairs)?;
                    arg = Some(MaildirMessageCopyArg::Copy);
                }
                MaildirMessageCopyResult::Err(err) => return Err(err.into()),
            }
        }
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
        let mut coroutine = MaildirMessageMove::new(id, source, target, target_subdir);
        let mut arg: Option<MaildirMessageMoveArg> = None;

        loop {
            match coroutine.resume(arg.take()) {
                MaildirMessageMoveResult::Ok => return Ok(()),
                MaildirMessageMoveResult::WantsFileExists(paths) => {
                    arg = Some(MaildirMessageMoveArg::FileExists(file_exists(paths)));
                }
                MaildirMessageMoveResult::WantsDirRead(paths) => {
                    arg = Some(MaildirMessageMoveArg::DirRead(read_dirs(paths)?));
                }
                MaildirMessageMoveResult::WantsRename(pairs) => {
                    rename_paths(pairs)?;
                    arg = Some(MaildirMessageMoveArg::Rename);
                }
                MaildirMessageMoveResult::Err(err) => return Err(err.into()),
            }
        }
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
