//! I/O-free coroutine reading the `dovecot-keywords` slot table at the
//! root of a Maildir. Returns an empty table when the file is absent.
//!
//! # Example
//!
//! ```rust,no_run
//! use io_maildir::{client::MaildirClient, dovecot::load::MaildirDovecotLoad};
//!
//! let client = MaildirClient::new("/path/to/root");
//! let maildir = client.load_maildir("inbox").unwrap();
//!
//! let coroutine = MaildirDovecotLoad::new(&maildir);
//! let table = client.run(coroutine).unwrap();
//!
//! for (letter, keyword) in &table {
//!     println!("{letter} = {keyword}");
//! }
//! ```

use core::{fmt, str};

use alloc::{
    collections::{BTreeMap, BTreeSet},
    string::String,
};

use log::debug;
use thiserror::Error;

use crate::{
    coroutine::*, dovecot::utils::parse_dovecot_keywords, maildir::Maildir, path::MaildirFsPath,
};

const FILENAME: &str = "dovecot-keywords";

/// Failure causes during a [`MaildirDovecotLoad`] step.
#[derive(Clone, Debug, Error)]
pub enum MaildirDovecotLoadError {
    /// A reply arrived that does not match the awaited step.
    #[error("Maildir dovecot load failed: unexpected arg {0:?}")]
    UnexpectedArg(Option<MaildirReply>),
}

/// Loads the `dovecot-keywords` slot table for a Maildir.
#[derive(Debug)]
pub struct MaildirDovecotLoad {
    path: MaildirFsPath,
    state: State,
}

impl MaildirDovecotLoad {
    /// Builds a coroutine loading the `dovecot-keywords` slot table for
    /// a Maildir.
    pub fn new(maildir: &Maildir) -> Self {
        Self {
            path: maildir.path().join(FILENAME),
            state: State::Start,
        }
    }
}

impl MaildirCoroutine for MaildirDovecotLoad {
    type Yield = MaildirYield;
    type Return = Result<BTreeMap<char, String>, MaildirDovecotLoadError>;

    fn resume(
        &mut self,
        arg: Option<MaildirReply>,
    ) -> MaildirCoroutineState<Self::Yield, Self::Return> {
        match (&mut self.state, arg) {
            (State::Start, None) => {
                let paths = BTreeSet::from_iter([self.path.clone()]);
                self.state = State::AwaitProbe;
                MaildirCoroutineState::Yielded(MaildirYield::WantsFileExists(paths))
            }
            (State::AwaitProbe, Some(MaildirReply::FileExists(map))) => {
                let exists = map.get(&self.path).copied().unwrap_or(false);
                if !exists {
                    debug!("no dovecot-keywords file, empty table");
                    return MaildirCoroutineState::Complete(Ok(BTreeMap::new()));
                }
                let paths = BTreeSet::from_iter([self.path.clone()]);
                self.state = State::AwaitRead;
                MaildirCoroutineState::Yielded(MaildirYield::WantsFileRead(paths))
            }
            (State::AwaitRead, Some(MaildirReply::FileRead(mut map))) => {
                let bytes = map.remove(&self.path).unwrap_or_default();
                let text = str::from_utf8(&bytes).unwrap_or("");
                let table = parse_dovecot_keywords(text);
                debug!("loaded {} dovecot keywords", table.len());
                MaildirCoroutineState::Complete(Ok(table))
            }
            (_, arg) => {
                let err = MaildirDovecotLoadError::UnexpectedArg(arg);
                MaildirCoroutineState::Complete(Err(err))
            }
        }
    }
}

#[derive(Debug)]
enum State {
    Start,
    AwaitProbe,
    AwaitRead,
}

impl fmt::Display for State {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Start => f.write_str("start"),
            Self::AwaitProbe => f.write_str("await probe reply"),
            Self::AwaitRead => f.write_str("await read reply"),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::dovecot::load::*;

    fn maildir() -> Maildir {
        Maildir::from_path("root")
    }

    fn keywords_path() -> MaildirFsPath {
        MaildirFsPath::from("root/dovecot-keywords")
    }

    #[test]
    fn missing_file_returns_empty_table() {
        let m = maildir();
        let mut cor = MaildirDovecotLoad::new(&m);

        expect_wants_file_exists(&mut cor);

        let mut map = BTreeMap::new();
        map.insert(keywords_path(), false);
        let table = expect_complete_ok(&mut cor, Some(MaildirReply::FileExists(map)));
        assert!(table.is_empty());
    }

    #[test]
    fn present_file_returns_parsed_table() {
        let m = maildir();
        let mut cor = MaildirDovecotLoad::new(&m);

        expect_wants_file_exists(&mut cor);

        let mut probe = BTreeMap::new();
        probe.insert(keywords_path(), true);
        match cor.resume(Some(MaildirReply::FileExists(probe))) {
            MaildirCoroutineState::Yielded(MaildirYield::WantsFileRead(paths)) => {
                assert!(paths.contains(&keywords_path()));
            }
            state => panic!("expected WantsFileRead, got {state:?}"),
        }

        let mut contents = BTreeMap::new();
        contents.insert(keywords_path(), b"0 Project\n1 Work\n".to_vec());
        let table = expect_complete_ok(&mut cor, Some(MaildirReply::FileRead(contents)));
        assert_eq!(table.len(), 2);
    }

    #[test]
    fn unexpected_reply_returns_error() {
        let m = maildir();
        let mut cor = MaildirDovecotLoad::new(&m);

        expect_wants_file_exists(&mut cor);

        let err = expect_complete_err(&mut cor, Some(MaildirReply::DirCreate));
        assert!(matches!(err, MaildirDovecotLoadError::UnexpectedArg(_)));
    }

    fn expect_wants_file_exists(cor: &mut MaildirDovecotLoad) {
        match cor.resume(None) {
            MaildirCoroutineState::Yielded(MaildirYield::WantsFileExists(paths)) => {
                assert!(paths.contains(&keywords_path()));
            }
            state => panic!("expected WantsFileExists, got {state:?}"),
        }
    }

    fn expect_complete_ok(
        cor: &mut MaildirDovecotLoad,
        arg: Option<MaildirReply>,
    ) -> BTreeMap<char, String> {
        match cor.resume(arg) {
            MaildirCoroutineState::Complete(Ok(table)) => table,
            state => panic!("expected Complete(Ok), got {state:?}"),
        }
    }

    fn expect_complete_err(
        cor: &mut MaildirDovecotLoad,
        arg: Option<MaildirReply>,
    ) -> MaildirDovecotLoadError {
        match cor.resume(arg) {
            MaildirCoroutineState::Complete(Err(err)) => err,
            state => panic!("expected Complete(Err), got {state:?}"),
        }
    }
}
