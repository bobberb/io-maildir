//! I/O-free coroutine listing every Maildir reachable from a store's
//! root.

use core::{fmt, mem};

use alloc::collections::{BTreeMap, BTreeSet};

use log::trace;
use thiserror::Error;

use crate::{
    coroutine::*,
    maildir::types::{CUR, Maildir, NEW, TMP},
    path::FsPath,
    store::MaildirStore,
};

/// Failure causes during a [`MaildirList`] step.
#[derive(Clone, Debug, Error)]
pub enum MaildirListError {
    #[error("Maildir list failed: unexpected arg {0:?}")]
    UnexpectedArg(Option<MaildirReply>),
}

/// Lists every Maildir reachable from a store's root.
///
/// Default layout (`store.maildirpp == false`) is fs: subfolders are
/// real nested directories, each with its own cur/new/tmp, and the
/// coroutine recurses through the whole tree (plain Maildir is the
/// no-subfolders degenerate case). With `store.maildirpp == true` it
/// switches to the flat layout where the root is itself INBOX and
/// subfolders are dot-prefixed siblings of cur/new/tmp at the root (no
/// recursion).
#[derive(Debug)]
pub struct MaildirList {
    state: State,
    maildirpp: bool,
}

impl MaildirList {
    pub fn new(store: &MaildirStore) -> Self {
        Self {
            state: State::Start {
                root: store.root.clone(),
            },
            maildirpp: store.maildirpp,
        }
    }
}

impl MaildirCoroutine for MaildirList {
    type Yield = MaildirYield;
    type Return = Result<BTreeSet<Maildir>, MaildirListError>;

    fn resume(
        &mut self,
        arg: Option<MaildirReply>,
    ) -> MaildirCoroutineState<Self::Yield, Self::Return> {
        trace!("maildir list: {}", self.state);

        match (&mut self.state, arg) {
            (State::Start { root }, None) => {
                let pending = BTreeSet::from_iter([mem::take(root)]);
                self.state = State::AwaitRead {
                    probe_pending: true,
                    found: BTreeSet::new(),
                };
                MaildirCoroutineState::Yielded(MaildirYield::WantsDirRead(pending))
            }
            (
                State::AwaitRead {
                    probe_pending,
                    found,
                },
                Some(MaildirReply::DirRead(entries)),
            ) => {
                let probe_pending = *probe_pending;
                let found = mem::take(found);
                let maildirpp = self.maildirpp;

                let mut candidates = BTreeSet::new();
                let mut next_pending = BTreeSet::new();

                if probe_pending {
                    candidates.extend(entries.keys().cloned());
                }

                for (_dir, names) in entries {
                    for path in names {
                        let Some(name) = path.file_name() else {
                            continue;
                        };

                        if name == CUR || name == NEW || name == TMP {
                            continue;
                        }

                        // Maildir++ keeps only dot-prefixed children
                        // (subfolders); fs skips dot-prefixed children
                        // (hidden).
                        let dotted = name.starts_with('.');
                        if maildirpp != dotted {
                            continue;
                        }

                        candidates.insert(path.clone());
                        if !maildirpp {
                            next_pending.insert(path);
                        }
                    }
                }

                if candidates.is_empty() {
                    return MaildirCoroutineState::Complete(Ok(found));
                }

                let mut markers = BTreeMap::new();
                for cand in &candidates {
                    markers.insert(cand.join(CUR), cand.clone());
                    markers.insert(cand.join(NEW), cand.clone());
                    markers.insert(cand.join(TMP), cand.clone());
                }
                let probes: BTreeSet<FsPath> = markers.keys().cloned().collect();

                self.state = State::AwaitProbe {
                    markers,
                    next_pending,
                    found,
                };
                MaildirCoroutineState::Yielded(MaildirYield::WantsDirExists(probes))
            }
            (
                State::AwaitProbe {
                    markers,
                    next_pending,
                    found,
                },
                Some(MaildirReply::DirExists(probes)),
            ) => {
                let markers = mem::take(markers);
                let next_pending = mem::take(next_pending);
                let mut found = mem::take(found);

                let mut hits: BTreeMap<FsPath, u8> = BTreeMap::new();
                for (marker, candidate) in markers {
                    if probes.get(&marker).copied().unwrap_or(false) {
                        *hits.entry(candidate).or_insert(0) += 1;
                    }
                }

                for (candidate, count) in hits {
                    if count == 3 {
                        found.insert(Maildir::from_path(candidate));
                    }
                }

                if next_pending.is_empty() {
                    return MaildirCoroutineState::Complete(Ok(found));
                }

                self.state = State::AwaitRead {
                    probe_pending: false,
                    found,
                };
                MaildirCoroutineState::Yielded(MaildirYield::WantsDirRead(next_pending))
            }
            (_, arg) => {
                let err = MaildirListError::UnexpectedArg(arg);
                MaildirCoroutineState::Complete(Err(err))
            }
        }
    }
}

#[derive(Debug)]
enum State {
    Start {
        root: FsPath,
    },
    AwaitRead {
        probe_pending: bool,
        found: BTreeSet<Maildir>,
    },
    AwaitProbe {
        markers: BTreeMap<FsPath, FsPath>,
        next_pending: BTreeSet<FsPath>,
        found: BTreeSet<Maildir>,
    },
}

impl fmt::Display for State {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Start { .. } => f.write_str("start"),
            Self::AwaitRead { .. } => f.write_str("await read reply"),
            Self::AwaitProbe { .. } => f.write_str("await probe reply"),
        }
    }
}

#[cfg(test)]
mod tests {
    use alloc::vec::Vec;

    use crate::maildir::list::*;

    fn fs_store() -> MaildirStore {
        MaildirStore {
            root: FsPath::from("root"),
            maildirpp: false,
        }
    }

    fn maildirpp_store() -> MaildirStore {
        MaildirStore {
            root: FsPath::from("root"),
            maildirpp: true,
        }
    }

    #[test]
    fn empty_root_returns_empty() {
        let mut cor = MaildirList::new(&fs_store());
        expect_wants_dir_read(&mut cor, None);

        let mut entries = BTreeMap::new();
        entries.insert(FsPath::from("root"), BTreeSet::new());

        let probes = expect_wants_dir_exists(&mut cor, Some(MaildirReply::DirRead(entries)));
        let reply = probes.into_iter().map(|p| (p, false)).collect();

        let out = expect_complete_ok(&mut cor, Some(MaildirReply::DirExists(reply)));
        assert!(out.is_empty());
    }

    #[test]
    fn root_is_a_maildir_returns_root() {
        let mut cor = MaildirList::new(&fs_store());
        expect_wants_dir_read(&mut cor, None);

        let mut entries = BTreeMap::new();
        entries.insert(FsPath::from("root"), BTreeSet::new());

        let probes = expect_wants_dir_exists(&mut cor, Some(MaildirReply::DirRead(entries)));
        let reply = probes.into_iter().map(|p| (p, true)).collect();

        let out = expect_complete_ok(&mut cor, Some(MaildirReply::DirExists(reply)));
        assert_eq!(out.len(), 1);
        assert!(out.contains(&Maildir::from_path("root")));
    }

    #[test]
    fn fs_recurses_into_nested_subfolders() {
        let mut cor = MaildirList::new(&fs_store());
        expect_wants_dir_read(&mut cor, None);

        let mut entries = BTreeMap::new();
        entries.insert(
            FsPath::from("root"),
            BTreeSet::from_iter([FsPath::from("root/Foo")]),
        );

        let probes = expect_wants_dir_exists(&mut cor, Some(MaildirReply::DirRead(entries)));
        let reply = probes.into_iter().map(|p| (p, true)).collect();

        let next = expect_wants_dir_read(&mut cor, Some(MaildirReply::DirExists(reply)));
        assert!(next.contains(&FsPath::from("root/Foo")));

        let mut entries = BTreeMap::new();
        entries.insert(
            FsPath::from("root/Foo"),
            BTreeSet::from_iter([FsPath::from("root/Foo/Bar")]),
        );

        let probes = expect_wants_dir_exists(&mut cor, Some(MaildirReply::DirRead(entries)));
        let reply = probes.into_iter().map(|p| (p, true)).collect();

        let next = expect_wants_dir_read(&mut cor, Some(MaildirReply::DirExists(reply)));
        assert!(next.contains(&FsPath::from("root/Foo/Bar")));

        let mut entries = BTreeMap::new();
        entries.insert(FsPath::from("root/Foo/Bar"), BTreeSet::new());

        let out = expect_complete_ok(&mut cor, Some(MaildirReply::DirRead(entries)));
        let names: Vec<_> = out.iter().map(|m| m.path().as_str()).collect();
        assert!(names.contains(&"root"));
        assert!(names.contains(&"root/Foo"));
        assert!(names.contains(&"root/Foo/Bar"));
        assert_eq!(out.len(), 3);
    }

    #[test]
    fn fs_skips_dotted_children() {
        let mut cor = MaildirList::new(&fs_store());
        expect_wants_dir_read(&mut cor, None);

        let mut entries = BTreeMap::new();
        entries.insert(
            FsPath::from("root"),
            BTreeSet::from_iter([FsPath::from("root/.Hidden"), FsPath::from("root/Sent")]),
        );

        let probes = expect_wants_dir_exists(&mut cor, Some(MaildirReply::DirRead(entries)));
        assert!(!probes.iter().any(|p| p.as_str().contains(".Hidden")));
        assert!(probes.iter().any(|p| p.as_str().contains("Sent")));
    }

    #[test]
    fn maildirpp_keeps_only_dotted_children_no_recursion() {
        let mut cor = MaildirList::new(&maildirpp_store());
        expect_wants_dir_read(&mut cor, None);

        let mut entries = BTreeMap::new();
        entries.insert(
            FsPath::from("root"),
            BTreeSet::from_iter([FsPath::from("root/.Sent"), FsPath::from("root/Other")]),
        );

        let probes = expect_wants_dir_exists(&mut cor, Some(MaildirReply::DirRead(entries)));
        assert!(probes.iter().any(|p| p.as_str() == "root/cur"));
        assert!(probes.iter().any(|p| p.as_str() == "root/.Sent/cur"));
        assert!(!probes.iter().any(|p| p.as_str().contains("Other")));

        let reply = probes.into_iter().map(|p| (p, true)).collect();
        let out = expect_complete_ok(&mut cor, Some(MaildirReply::DirExists(reply)));
        assert_eq!(out.len(), 2);
        assert!(out.contains(&Maildir::from_path("root")));
        assert!(out.contains(&Maildir::from_path("root/.Sent")));
    }

    #[test]
    fn cur_new_tmp_are_not_candidates() {
        let mut cor = MaildirList::new(&fs_store());
        expect_wants_dir_read(&mut cor, None);

        let mut entries = BTreeMap::new();
        entries.insert(
            FsPath::from("root"),
            BTreeSet::from_iter([
                FsPath::from("root/cur"),
                FsPath::from("root/new"),
                FsPath::from("root/tmp"),
            ]),
        );

        let probes = expect_wants_dir_exists(&mut cor, Some(MaildirReply::DirRead(entries)));
        assert_eq!(probes.len(), 3);
        assert!(probes.iter().all(|p| p.parent() == Some("root")));
    }

    #[test]
    fn unexpected_reply_returns_error() {
        let mut cor = MaildirList::new(&fs_store());
        expect_wants_dir_read(&mut cor, None);

        let err = expect_complete_err(&mut cor, Some(MaildirReply::DirCreate));
        assert!(matches!(err, MaildirListError::UnexpectedArg(_)));
    }

    // --- utils

    fn expect_wants_dir_read(cor: &mut MaildirList, arg: Option<MaildirReply>) -> BTreeSet<FsPath> {
        match cor.resume(arg) {
            MaildirCoroutineState::Yielded(MaildirYield::WantsDirRead(paths)) => paths,
            state => panic!("expected WantsDirRead, got {state:?}"),
        }
    }

    fn expect_wants_dir_exists(
        cor: &mut MaildirList,
        arg: Option<MaildirReply>,
    ) -> BTreeSet<FsPath> {
        match cor.resume(arg) {
            MaildirCoroutineState::Yielded(MaildirYield::WantsDirExists(paths)) => paths,
            state => panic!("expected WantsDirExists, got {state:?}"),
        }
    }

    fn expect_complete_ok(cor: &mut MaildirList, arg: Option<MaildirReply>) -> BTreeSet<Maildir> {
        match cor.resume(arg) {
            MaildirCoroutineState::Complete(Ok(found)) => found,
            state => panic!("expected Complete(Ok), got {state:?}"),
        }
    }

    fn expect_complete_err(cor: &mut MaildirList, arg: Option<MaildirReply>) -> MaildirListError {
        match cor.resume(arg) {
            MaildirCoroutineState::Complete(Err(err)) => err,
            state => panic!("expected Complete(Err), got {state:?}"),
        }
    }
}
