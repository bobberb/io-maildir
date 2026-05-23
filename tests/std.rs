use std::{
    collections::{BTreeMap, BTreeSet},
    fs, process,
    time::{SystemTime, UNIX_EPOCH},
};

use io_maildir::{
    coroutines::{
        flags_add::{MaildirFlagsAdd, MaildirFlagsAddArg, MaildirFlagsAddResult},
        flags_remove::{MaildirFlagsRemove, MaildirFlagsRemoveArg, MaildirFlagsRemoveResult},
        flags_set::{MaildirFlagsSet, MaildirFlagsSetArg, MaildirFlagsSetResult},
        maildir_create::{MaildirCreate, MaildirCreateArg, MaildirCreateResult},
        maildir_delete::{MaildirDelete, MaildirDeleteArg, MaildirDeleteResult},
        maildir_list::{MaildirList, MaildirListArg, MaildirListResult},
        maildir_rename::{MaildirRename, MaildirRenameArg, MaildirRenameResult},
        message_copy::{MaildirMessageCopy, MaildirMessageCopyArg, MaildirMessageCopyResult},
        message_get::{MaildirMessageGet, MaildirMessageGetArg, MaildirMessageGetResult},
        message_list::{MaildirMessagesList, MaildirMessagesListArg, MaildirMessagesListResult},
        message_move::{MaildirMessageMove, MaildirMessageMoveArg, MaildirMessageMoveResult},
        message_store::{MaildirMessageStore, MaildirMessageStoreArg, MaildirMessageStoreResult},
    },
    flag::{Flag, Flags},
    maildir::{Maildir, MaildirSubdir},
    path::MaildirPath,
};
use tempfile::tempdir;

fn into_path(p: impl AsRef<std::path::Path>) -> MaildirPath {
    MaildirPath::new(p.as_ref().to_string_lossy().into_owned())
}

fn dir_read(paths: BTreeSet<MaildirPath>) -> BTreeMap<MaildirPath, BTreeSet<MaildirPath>> {
    let mut entries = BTreeMap::new();

    for path in paths {
        let mut children = BTreeSet::new();

        for entry in fs::read_dir(path.as_str()).unwrap() {
            let entry = entry.unwrap();
            children.insert(into_path(entry.path()));
        }

        entries.insert(path, children);
    }

    entries
}

fn file_read(paths: BTreeSet<MaildirPath>) -> BTreeMap<MaildirPath, Vec<u8>> {
    let mut contents = BTreeMap::new();
    for path in paths {
        let data = fs::read(path.as_str()).unwrap();
        contents.insert(path, data);
    }
    contents
}

fn file_exists(paths: BTreeSet<MaildirPath>) -> BTreeMap<MaildirPath, bool> {
    paths
        .into_iter()
        .map(|p| {
            let exists = fs::metadata(p.as_str())
                .map(|m| m.is_file())
                .unwrap_or(false);
            (p, exists)
        })
        .collect()
}

fn dir_exists(paths: BTreeSet<MaildirPath>) -> BTreeMap<MaildirPath, bool> {
    paths
        .into_iter()
        .map(|p| {
            let exists = fs::metadata(p.as_str())
                .map(|m| m.is_dir())
                .unwrap_or(false);
            (p, exists)
        })
        .collect()
}

fn rename(pairs: Vec<(MaildirPath, MaildirPath)>) {
    for (from, to) in pairs {
        fs::rename(from.as_str(), to.as_str()).unwrap();
    }
}

fn copy(pairs: Vec<(MaildirPath, MaildirPath)>) {
    for (from, to) in pairs {
        fs::copy(from.as_str(), to.as_str()).unwrap();
    }
}

fn create_maildir(root: MaildirPath) -> Maildir {
    let mut arg: Option<MaildirCreateArg> = None;
    let mut coroutine = MaildirCreate::new(root.clone());

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

    Maildir::from_path(root)
}

fn message_count(maildir: Maildir) -> usize {
    let mut arg: Option<MaildirMessagesListArg> = None;
    let mut c = MaildirMessagesList::new(maildir);

    loop {
        match c.resume(arg.take()) {
            MaildirMessagesListResult::Ok(m) => return m.len(),
            MaildirMessagesListResult::WantsDirRead(paths) => {
                arg = Some(MaildirMessagesListArg::DirRead(dir_read(paths)));
            }
            MaildirMessagesListResult::WantsFileExists(paths) => {
                arg = Some(MaildirMessagesListArg::FileExists(file_exists(paths)));
            }
            MaildirMessagesListResult::WantsFileRead(paths) => {
                arg = Some(MaildirMessagesListArg::FileRead(file_read(paths)));
            }
            MaildirMessagesListResult::Err(err) => panic!("{err}"),
        }
    }
}

fn run_store(
    inbox: Maildir,
    subdir: MaildirSubdir,
    flags: Flags,
    contents: Vec<u8>,
) -> (String, MaildirPath) {
    let mut arg: Option<MaildirMessageStoreArg> = None;
    let mut coroutine = MaildirMessageStore::new(inbox, subdir, flags, contents);

    loop {
        match coroutine.resume(arg.take()) {
            MaildirMessageStoreResult::Ok { id, path } => return (id, path),
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
                arg = Some(MaildirMessageStoreArg::Hostname("localhost".into()));
            }
            MaildirMessageStoreResult::WantsFileCreate(files) => {
                for (path, contents) in files {
                    fs::write(path.as_str(), &contents).unwrap();
                }
                arg = Some(MaildirMessageStoreArg::FileCreate);
            }
            MaildirMessageStoreResult::WantsRename(pairs) => {
                rename(pairs);
                arg = Some(MaildirMessageStoreArg::Rename);
            }
            MaildirMessageStoreResult::Err(err) => panic!("{err}"),
        }
    }
}

#[test]
fn std() {
    let _ = env_logger::try_init();

    let workdir = tempdir().unwrap();
    let root = into_path(workdir.path());

    // should list zero maildirs in empty root

    let mut arg: Option<MaildirListArg> = None;
    let mut coroutine = MaildirList::new(root.clone());

    let maildirs = loop {
        match coroutine.resume(arg.take()) {
            MaildirListResult::Ok(m) => break m,
            MaildirListResult::WantsDirRead(paths) => {
                arg = Some(MaildirListArg::DirRead(dir_read(paths)));
            }
            MaildirListResult::WantsDirExists(paths) => {
                arg = Some(MaildirListArg::DirExists(dir_exists(paths)));
            }
            MaildirListResult::Err(err) => panic!("{err}"),
        }
    };

    assert!(maildirs.is_empty());

    // should create maildirs

    let inbox = create_maildir(root.join("inbox"));
    let drafts = create_maildir(root.join("drafts"));

    assert!(fs::metadata(inbox.cur().as_str()).unwrap().is_dir());
    assert!(fs::metadata(inbox.new().as_str()).unwrap().is_dir());
    assert!(fs::metadata(inbox.tmp().as_str()).unwrap().is_dir());

    // should list two maildirs

    let mut arg: Option<MaildirListArg> = None;
    let mut coroutine = MaildirList::new(root.clone());

    let maildirs = loop {
        match coroutine.resume(arg.take()) {
            MaildirListResult::Ok(m) => break m,
            MaildirListResult::WantsDirRead(paths) => {
                arg = Some(MaildirListArg::DirRead(dir_read(paths)));
            }
            MaildirListResult::WantsDirExists(paths) => {
                arg = Some(MaildirListArg::DirExists(dir_exists(paths)));
            }
            MaildirListResult::Err(err) => panic!("{err}"),
        }
    };

    assert_eq!(maildirs.len(), 2);

    // should store a message in /new

    let msg = b"From: alice@example.com\r\nSubject: Test\r\n\r\nBody\r\n".to_vec();
    let (id, msg_path) = run_store(inbox.clone(), MaildirSubdir::New, Flags::default(), msg);

    assert!(fs::metadata(msg_path.as_str()).unwrap().is_file());
    assert!(msg_path.starts_with(&inbox.new()));

    // should list messages

    let mut arg: Option<MaildirMessagesListArg> = None;
    let mut coroutine = MaildirMessagesList::new(inbox.clone());

    let messages = loop {
        match coroutine.resume(arg.take()) {
            MaildirMessagesListResult::Ok(m) => break m,
            MaildirMessagesListResult::WantsDirRead(paths) => {
                arg = Some(MaildirMessagesListArg::DirRead(dir_read(paths)));
            }
            MaildirMessagesListResult::WantsFileExists(paths) => {
                arg = Some(MaildirMessagesListArg::FileExists(file_exists(paths)));
            }
            MaildirMessagesListResult::WantsFileRead(paths) => {
                arg = Some(MaildirMessagesListArg::FileRead(file_read(paths)));
            }
            MaildirMessagesListResult::Err(err) => panic!("{err}"),
        }
    };

    assert_eq!(messages.len(), 1);

    // should get the message

    let mut arg: Option<MaildirMessageGetArg> = None;
    let mut coroutine = MaildirMessageGet::new(inbox.clone(), &id);

    let message = loop {
        match coroutine.resume(arg.take()) {
            MaildirMessageGetResult::Ok(m) => break m,
            MaildirMessageGetResult::WantsFileExists(paths) => {
                arg = Some(MaildirMessageGetArg::FileExists(file_exists(paths)));
            }
            MaildirMessageGetResult::WantsDirRead(paths) => {
                arg = Some(MaildirMessageGetArg::DirRead(dir_read(paths)));
            }
            MaildirMessageGetResult::WantsFileRead(paths) => {
                arg = Some(MaildirMessageGetArg::FileRead(file_read(paths)));
            }
            MaildirMessageGetResult::Err(err) => panic!("{err}"),
        }
    };

    assert_eq!(message.id(), Some(id.as_str()));

    // should set flags (message now lives in /new, flags are a no-op)

    let mut arg: Option<MaildirFlagsSetArg> = None;
    let flags_seen = Flags::from_iter([Flag::Seen]);
    let mut coroutine = MaildirFlagsSet::new(inbox.clone(), &id, flags_seen);

    loop {
        match coroutine.resume(arg.take()) {
            MaildirFlagsSetResult::Ok => break,
            MaildirFlagsSetResult::WantsFileExists(paths) => {
                arg = Some(MaildirFlagsSetArg::FileExists(file_exists(paths)));
            }
            MaildirFlagsSetResult::WantsDirRead(paths) => {
                arg = Some(MaildirFlagsSetArg::DirRead(dir_read(paths)));
            }
            MaildirFlagsSetResult::WantsRename(pairs) => {
                rename(pairs);
                arg = Some(MaildirFlagsSetArg::Rename);
            }
            MaildirFlagsSetResult::Err(err) => panic!("{err}"),
        }
    }

    // should add flags (no-op for /new messages)

    let mut arg: Option<MaildirFlagsAddArg> = None;
    let flags_flagged = Flags::from_iter([Flag::Flagged]);
    let mut coroutine = MaildirFlagsAdd::new(inbox.clone(), &id, flags_flagged);

    loop {
        match coroutine.resume(arg.take()) {
            MaildirFlagsAddResult::Ok => break,
            MaildirFlagsAddResult::WantsFileExists(paths) => {
                arg = Some(MaildirFlagsAddArg::FileExists(file_exists(paths)));
            }
            MaildirFlagsAddResult::WantsDirRead(paths) => {
                arg = Some(MaildirFlagsAddArg::DirRead(dir_read(paths)));
            }
            MaildirFlagsAddResult::WantsRename(pairs) => {
                rename(pairs);
                arg = Some(MaildirFlagsAddArg::Rename);
            }
            MaildirFlagsAddResult::Err(err) => panic!("{err}"),
        }
    }

    // should remove flags (no-op for /new messages)

    let mut arg: Option<MaildirFlagsRemoveArg> = None;
    let flags_seen2 = Flags::from_iter([Flag::Seen]);
    let mut coroutine = MaildirFlagsRemove::new(inbox.clone(), &id, flags_seen2);

    loop {
        match coroutine.resume(arg.take()) {
            MaildirFlagsRemoveResult::Ok => break,
            MaildirFlagsRemoveResult::WantsFileExists(paths) => {
                arg = Some(MaildirFlagsRemoveArg::FileExists(file_exists(paths)));
            }
            MaildirFlagsRemoveResult::WantsDirRead(paths) => {
                arg = Some(MaildirFlagsRemoveArg::DirRead(dir_read(paths)));
            }
            MaildirFlagsRemoveResult::WantsRename(pairs) => {
                rename(pairs);
                arg = Some(MaildirFlagsRemoveArg::Rename);
            }
            MaildirFlagsRemoveResult::Err(err) => panic!("{err}"),
        }
    }

    // should copy message to drafts

    let mut arg: Option<MaildirMessageCopyArg> = None;
    let mut coroutine =
        MaildirMessageCopy::new(&id, inbox.clone(), drafts.clone(), Some(MaildirSubdir::New));

    loop {
        match coroutine.resume(arg.take()) {
            MaildirMessageCopyResult::Ok => break,
            MaildirMessageCopyResult::WantsFileExists(paths) => {
                arg = Some(MaildirMessageCopyArg::FileExists(file_exists(paths)));
            }
            MaildirMessageCopyResult::WantsDirRead(paths) => {
                arg = Some(MaildirMessageCopyArg::DirRead(dir_read(paths)));
            }
            MaildirMessageCopyResult::WantsCopy(pairs) => {
                copy(pairs);
                arg = Some(MaildirMessageCopyArg::Copy);
            }
            MaildirMessageCopyResult::Err(err) => panic!("{err}"),
        }
    }

    // inbox still has one message after copy
    assert_eq!(message_count(inbox.clone()), 1);

    // drafts now has one message
    assert_eq!(message_count(drafts.clone()), 1);

    // should move message from inbox to drafts

    let mut arg: Option<MaildirMessageMoveArg> = None;
    let mut coroutine =
        MaildirMessageMove::new(&id, inbox.clone(), drafts.clone(), Some(MaildirSubdir::New));

    loop {
        match coroutine.resume(arg.take()) {
            MaildirMessageMoveResult::Ok => break,
            MaildirMessageMoveResult::WantsFileExists(paths) => {
                arg = Some(MaildirMessageMoveArg::FileExists(file_exists(paths)));
            }
            MaildirMessageMoveResult::WantsDirRead(paths) => {
                arg = Some(MaildirMessageMoveArg::DirRead(dir_read(paths)));
            }
            MaildirMessageMoveResult::WantsRename(pairs) => {
                rename(pairs);
                arg = Some(MaildirMessageMoveArg::Rename);
            }
            MaildirMessageMoveResult::Err(err) => panic!("{err}"),
        }
    }

    // inbox should now be empty
    assert_eq!(message_count(inbox.clone()), 0);

    // should rename maildir

    let mut arg: Option<MaildirRenameArg> = None;
    let mut coroutine = MaildirRename::new(drafts.path().clone(), "archive");

    loop {
        match coroutine.resume(arg.take()) {
            MaildirRenameResult::Ok => break,
            MaildirRenameResult::WantsRename(pairs) => {
                rename(pairs);
                arg = Some(MaildirRenameArg::Rename);
            }
            MaildirRenameResult::Err(err) => panic!("{err}"),
        }
    }

    assert!(
        fs::metadata(root.join("drafts").as_str())
            .map(|m| m.is_dir())
            .unwrap_or(false)
            == false
    );
    assert!(
        fs::metadata(root.join("archive").as_str())
            .unwrap()
            .is_dir()
    );

    // should delete maildirs

    for name in ["inbox", "archive"] {
        let mut arg: Option<MaildirDeleteArg> = None;
        let mut coroutine = MaildirDelete::new(root.join(name));

        loop {
            match coroutine.resume(arg.take()) {
                MaildirDeleteResult::Ok => break,
                MaildirDeleteResult::WantsDirRemove(paths) => {
                    for path in paths {
                        fs::remove_dir_all(path.as_str()).unwrap();
                    }
                    arg = Some(MaildirDeleteArg::DirRemove);
                }
                MaildirDeleteResult::Err(err) => panic!("{err}"),
            }
        }
    }

    let mut arg: Option<MaildirListArg> = None;
    let mut coroutine = MaildirList::new(root);

    let maildirs = loop {
        match coroutine.resume(arg.take()) {
            MaildirListResult::Ok(m) => break m,
            MaildirListResult::WantsDirRead(paths) => {
                arg = Some(MaildirListArg::DirRead(dir_read(paths)));
            }
            MaildirListResult::WantsDirExists(paths) => {
                arg = Some(MaildirListArg::DirExists(dir_exists(paths)));
            }
            MaildirListResult::Err(err) => panic!("{err}"),
        }
    };

    assert!(maildirs.is_empty());
}
