use core::hash::{Hash, Hasher};

use alloc::vec::Vec;

use crate::path::MaildirPath;

#[cfg(unix)]
pub static INFORMATIONAL_SUFFIX_SEPARATOR: char = ':';
#[cfg(windows)]
pub static INFORMATIONAL_SUFFIX_SEPARATOR: char = ';';

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct Message {
    pub(crate) path: MaildirPath,
    pub(crate) contents: Vec<u8>,
}

impl Message {
    pub fn path(&self) -> &MaildirPath {
        &self.path
    }

    pub fn id(&self) -> Option<&str> {
        let file_name = self.path.file_name()?;

        let id = match file_name.rsplit_once(INFORMATIONAL_SUFFIX_SEPARATOR) {
            Some((id, _)) => id,
            None => file_name,
        };

        Some(id)
    }

    pub fn contents(&self) -> &[u8] {
        &self.contents
    }

    #[cfg(feature = "parser")]
    pub fn parsed(&self) -> Option<mail_parser::Message<'_>> {
        mail_parser::MessageParser::new().parse(&self.contents)
    }

    #[cfg(feature = "parser")]
    pub fn headers(&self) -> Option<mail_parser::Message<'_>> {
        mail_parser::MessageParser::new()
            .with_minimal_headers()
            .parse(&self.contents)
    }
}

impl From<Message> for Vec<u8> {
    fn from(msg: Message) -> Self {
        msg.contents
    }
}

impl From<(MaildirPath, Vec<u8>)> for Message {
    fn from((path, contents): (MaildirPath, Vec<u8>)) -> Self {
        Self { path, contents }
    }
}

impl Hash for Message {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.path.hash(state);
    }
}
