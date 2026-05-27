//! Pure parsers and serialisers for the two sidecars used to encode
//! custom Maildir keywords:
//!
//! - the `dovecot-keywords` file dropped at the root of a Maildir by
//!   Dovecot / mbsync, mapping single lowercase letters `a..z` to
//!   keyword names; and
//! - the optional body header (`X-Keywords` or `X-Label`) carrying the
//!   keywords inline with the message bytes.
//!
//! Both formats are no_std-friendly: byte slices in, owned types out.

use alloc::{
    collections::BTreeMap,
    string::{String, ToString},
    vec::Vec,
};

use crate::flag::KeywordHeader;

const SLOT_MIN: u8 = b'a';
const SLOT_COUNT: u8 = 26;

/// Parses a `dovecot-keywords` file content into a slot table.
///
/// Each non-empty line is expected to start with a slot number (decimal
/// integer, 0-based) followed by whitespace and the keyword name. Slot
/// `N` maps to letter `'a' + N` while `N < 26`. Gaps and trailing
/// whitespace are tolerated; malformed lines are skipped.
pub fn parse_dovecot_keywords(text: &str) -> BTreeMap<char, String> {
    let mut table = BTreeMap::new();

    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        let Some((idx, name)) = line.split_once(char::is_whitespace) else {
            continue;
        };

        let Ok(n) = idx.trim().parse::<u8>() else {
            continue;
        };

        if n >= SLOT_COUNT {
            continue;
        }

        let letter = (SLOT_MIN + n) as char;
        table.insert(letter, name.trim().to_string());
    }

    table
}

/// Serialises a slot table back into the `dovecot-keywords` line
/// format. Slot indices are written in ascending order regardless of
/// the input ordering, so the output is deterministic.
pub fn serialize_dovecot_keywords(table: &BTreeMap<char, String>) -> String {
    let mut out = String::new();

    for (letter, name) in table {
        let Some(n) = letter_to_slot(*letter) else {
            continue;
        };

        let _ = core::fmt::Write::write_fmt(&mut out, format_args!("{n} {name}\n"));
    }

    out
}

/// Returns the lowest free slot if `keyword` is not yet known, or the
/// existing letter if it is. Returns `None` when every slot is taken.
pub fn allocate_keyword_slot(table: &mut BTreeMap<char, String>, keyword: &str) -> Option<char> {
    for (letter, name) in table.iter() {
        if name == keyword {
            return Some(*letter);
        }
    }

    for n in 0..SLOT_COUNT {
        let letter = (SLOT_MIN + n) as char;
        if !table.contains_key(&letter) {
            table.insert(letter, keyword.to_string());
            return Some(letter);
        }
    }

    None
}

/// Returns the values of the named header from the RFC 5322 message
/// `bytes`, split on `header.separator()`. Header lookup is
/// case-insensitive and respects line folding (continuation lines
/// start with WSP). Bytes outside ASCII are passed through as UTF-8;
/// invalid sequences are dropped.
pub fn extract_keywords_header(bytes: &[u8], header: KeywordHeader) -> Vec<String> {
    let mut out = Vec::new();
    let name = header.header_name();

    for line in iter_header_lines(bytes) {
        if !header_matches(&line, name) {
            continue;
        }

        let value = match line.iter().position(|&b| b == b':') {
            Some(idx) => &line[idx + 1..],
            None => continue,
        };

        let Ok(value) = core::str::from_utf8(value) else {
            continue;
        };

        for part in value.split(header.separator()) {
            let trimmed = part.trim();
            if !trimmed.is_empty() {
                out.push(trimmed.to_string());
            }
        }
    }

    out
}

/// Removes any header whose name (case-insensitively) matches one of
/// `names`. Returns the modified byte slice. Preserves CRLF / LF
/// terminators exactly as observed.
pub fn strip_headers(bytes: &[u8], names: &[&str]) -> Vec<u8> {
    let mut out = Vec::with_capacity(bytes.len());
    let mut cursor = 0;
    let header_end = find_header_end(bytes);

    while cursor < header_end {
        let line_end = find_line_end(bytes, cursor);
        let (logical, next) = read_unfolded(bytes, cursor, line_end, header_end);

        if let Some(name) = header_name(&logical) {
            if names.iter().any(|n| n.eq_ignore_ascii_case(name)) {
                cursor = next;
                continue;
            }
        }

        out.extend_from_slice(&bytes[cursor..next]);
        cursor = next;
    }

    out.extend_from_slice(&bytes[cursor..]);
    out
}

/// Inserts `name: value` immediately after the existing `Date:`
/// header. Falls back to the top of the header block when `Date:` is
/// missing. The injected line uses CRLF terminators when the existing
/// message uses CRLF, LF otherwise.
pub fn inject_header(bytes: &[u8], name: &str, value: &str) -> Vec<u8> {
    let crlf = uses_crlf(bytes);
    let eol: &[u8] = if crlf { b"\r\n" } else { b"\n" };

    let mut payload = Vec::with_capacity(name.len() + value.len() + 4);
    payload.extend_from_slice(name.as_bytes());
    payload.extend_from_slice(b": ");
    payload.extend_from_slice(value.as_bytes());
    payload.extend_from_slice(eol);

    let header_end = find_header_end(bytes);
    let mut cursor = 0;

    while cursor < header_end {
        let line_end = find_line_end(bytes, cursor);
        let (logical, next) = read_unfolded(bytes, cursor, line_end, header_end);

        if let Some(hname) = header_name(&logical) {
            if hname.eq_ignore_ascii_case("Date") {
                let mut out = Vec::with_capacity(bytes.len() + payload.len());
                out.extend_from_slice(&bytes[..next]);
                out.extend_from_slice(&payload);
                out.extend_from_slice(&bytes[next..]);
                return out;
            }
        }

        cursor = next;
    }

    let mut out = Vec::with_capacity(bytes.len() + payload.len());
    out.extend_from_slice(&payload);
    out.extend_from_slice(bytes);
    out
}

// ---- private helpers ---------------------------------------------------

fn letter_to_slot(c: char) -> Option<u8> {
    let b = c as u32;
    if b >= SLOT_MIN as u32 && b < (SLOT_MIN + SLOT_COUNT) as u32 {
        Some((b as u8) - SLOT_MIN)
    } else {
        None
    }
}

fn find_line_end(bytes: &[u8], start: usize) -> usize {
    let mut i = start;
    while i < bytes.len() && bytes[i] != b'\n' {
        i += 1;
    }
    i
}

fn find_header_end(bytes: &[u8]) -> usize {
    let mut i = 0;
    while i < bytes.len() {
        let line_end = find_line_end(bytes, i);

        // Empty line (or CR-only) terminates the header block.
        let content_end = if line_end > i && bytes[line_end - 1] == b'\r' {
            line_end - 1
        } else {
            line_end
        };

        if content_end == i {
            return if line_end < bytes.len() {
                line_end + 1
            } else {
                line_end
            };
        }

        i = if line_end < bytes.len() {
            line_end + 1
        } else {
            line_end
        };
    }
    bytes.len()
}

fn read_unfolded(
    bytes: &[u8],
    start: usize,
    mut line_end: usize,
    header_end: usize,
) -> (Vec<u8>, usize) {
    let mut logical = Vec::new();
    logical.extend_from_slice(&bytes[start..line_end]);
    let mut next = if line_end < bytes.len() {
        line_end + 1
    } else {
        line_end
    };

    while next < header_end {
        let peek = bytes[next];
        if peek == b' ' || peek == b'\t' {
            line_end = find_line_end(bytes, next);
            logical.extend_from_slice(&bytes[next..line_end]);
            next = if line_end < bytes.len() {
                line_end + 1
            } else {
                line_end
            };
        } else {
            break;
        }
    }

    (logical, next)
}

fn header_name(line: &[u8]) -> Option<&str> {
    let colon = line.iter().position(|&b| b == b':')?;
    let name = &line[..colon];
    if name.is_empty() {
        return None;
    }
    core::str::from_utf8(name).ok().map(|s| s.trim())
}

fn header_matches(line: &[u8], name: &str) -> bool {
    match header_name(line) {
        Some(actual) => actual.eq_ignore_ascii_case(name),
        None => false,
    }
}

fn iter_header_lines(bytes: &[u8]) -> impl Iterator<Item = Vec<u8>> + '_ {
    let header_end = find_header_end(bytes);
    let mut cursor = 0;
    core::iter::from_fn(move || {
        if cursor >= header_end {
            return None;
        }
        let line_end = find_line_end(bytes, cursor);
        let (logical, next) = read_unfolded(bytes, cursor, line_end, header_end);
        cursor = next;
        Some(logical)
    })
}

fn uses_crlf(bytes: &[u8]) -> bool {
    for i in 0..bytes.len() {
        if bytes[i] == b'\n' {
            return i > 0 && bytes[i - 1] == b'\r';
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_dovecot_table_basic() {
        let text = "0 Important\n1 Work\n2 Personal\n";
        let table = parse_dovecot_keywords(text);
        assert_eq!(table.get(&'a'), Some(&"Important".to_string()));
        assert_eq!(table.get(&'b'), Some(&"Work".to_string()));
        assert_eq!(table.get(&'c'), Some(&"Personal".to_string()));
    }

    #[test]
    fn parse_dovecot_table_with_gaps() {
        let text = "0 Important\n\n3 Work\n";
        let table = parse_dovecot_keywords(text);
        assert_eq!(table.get(&'a'), Some(&"Important".to_string()));
        assert_eq!(table.get(&'d'), Some(&"Work".to_string()));
        assert!(table.get(&'b').is_none());
    }

    #[test]
    fn parse_dovecot_table_drops_overflow() {
        let text = "26 ShouldBeIgnored\n0 Ok\n";
        let table = parse_dovecot_keywords(text);
        assert_eq!(table.get(&'a'), Some(&"Ok".to_string()));
        assert_eq!(table.len(), 1);
    }

    #[test]
    fn serialize_dovecot_table_deterministic() {
        let mut table = BTreeMap::new();
        table.insert('b', "Work".to_string());
        table.insert('a', "Important".to_string());
        assert_eq!(serialize_dovecot_keywords(&table), "0 Important\n1 Work\n");
    }

    #[test]
    fn allocate_returns_existing_slot() {
        let mut table = BTreeMap::new();
        table.insert('a', "Work".to_string());
        assert_eq!(allocate_keyword_slot(&mut table, "Work"), Some('a'));
        assert_eq!(table.len(), 1);
    }

    #[test]
    fn allocate_picks_lowest_free_slot() {
        let mut table = BTreeMap::new();
        table.insert('a', "Work".to_string());
        table.insert('c', "Personal".to_string());
        assert_eq!(allocate_keyword_slot(&mut table, "New"), Some('b'));
    }

    #[test]
    fn allocate_returns_none_when_full() {
        let mut table = BTreeMap::new();
        for n in 0..26 {
            table.insert((b'a' + n) as char, format!("k{n}"));
        }
        assert_eq!(allocate_keyword_slot(&mut table, "extra"), None);
    }

    #[test]
    fn extract_x_keywords_comma_separated() {
        let msg = b"From: a@b\r\nX-Keywords: Work, Personal,Urgent\r\n\r\nbody";
        let kws = extract_keywords_header(msg, KeywordHeader::XKeywords);
        assert_eq!(kws, vec!["Work", "Personal", "Urgent"]);
    }

    #[test]
    fn extract_x_label_space_separated() {
        let msg = b"X-Label: work personal urgent\n\nbody";
        let kws = extract_keywords_header(msg, KeywordHeader::XLabel);
        assert_eq!(kws, vec!["work", "personal", "urgent"]);
    }

    #[test]
    fn extract_handles_line_folding() {
        let msg = b"X-Keywords: Work,\r\n    Personal,\r\n\tUrgent\r\n\r\nbody";
        let kws = extract_keywords_header(msg, KeywordHeader::XKeywords);
        assert_eq!(kws, vec!["Work", "Personal", "Urgent"]);
    }

    #[test]
    fn extract_missing_header() {
        let msg = b"From: a@b\r\n\r\nbody";
        let kws = extract_keywords_header(msg, KeywordHeader::XKeywords);
        assert!(kws.is_empty());
    }

    #[test]
    fn strip_removes_named_headers() {
        let msg = b"From: a@b\r\nX-Mozilla-Status: 0001\r\nDate: now\r\n\r\nbody";
        let out = strip_headers(msg, &["X-Mozilla-Status"]);
        assert_eq!(out, b"From: a@b\r\nDate: now\r\n\r\nbody");
    }

    #[test]
    fn strip_removes_folded_continuations() {
        let msg = b"From: a@b\r\nX-Keywords: Work,\r\n    Personal\r\nDate: now\r\n\r\nbody";
        let out = strip_headers(msg, &["X-Keywords"]);
        assert_eq!(out, b"From: a@b\r\nDate: now\r\n\r\nbody");
    }

    #[test]
    fn strip_case_insensitive() {
        let msg = b"x-keywords: Work\r\n\r\nbody";
        let out = strip_headers(msg, &["X-Keywords"]);
        assert_eq!(out, b"\r\nbody");
    }

    #[test]
    fn inject_after_date_header() {
        let msg = b"From: a@b\r\nDate: now\r\nSubject: hi\r\n\r\nbody";
        let out = inject_header(msg, "X-Keywords", "Work, Personal");
        assert_eq!(
            out,
            b"From: a@b\r\nDate: now\r\nX-Keywords: Work, Personal\r\nSubject: hi\r\n\r\nbody"
        );
    }

    #[test]
    fn inject_without_date_prepends() {
        let msg = b"From: a@b\r\nSubject: hi\r\n\r\nbody";
        let out = inject_header(msg, "X-Keywords", "Work");
        assert_eq!(
            out,
            b"X-Keywords: Work\r\nFrom: a@b\r\nSubject: hi\r\n\r\nbody"
        );
    }

    #[test]
    fn inject_lf_message_keeps_lf() {
        let msg = b"Date: now\n\nbody";
        let out = inject_header(msg, "X-Keywords", "Work");
        assert_eq!(out, b"Date: now\nX-Keywords: Work\n\nbody");
    }
}
