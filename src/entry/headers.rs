//! RFC 5322 header helpers: extract keyword headers, strip arbitrary
//! headers, inject a header after `Date:`.

use core::{iter, str};

use alloc::{
    string::{String, ToString},
    vec::Vec,
};

use crate::flag::types::KeywordHeader;

/// Extracts values from `header`, splitting on `header.separator()`.
///
/// Case-insensitive lookup; respects RFC 5322 line folding.
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

        let Ok(value) = str::from_utf8(value) else {
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

/// Removes every header whose name (case-insensitively) matches one
/// of `names`. Preserves CRLF / LF terminators.
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

/// Inserts `name: value` after the existing `Date:` header, or at the
/// top of the header block otherwise. Matches the message's existing
/// EOL style (CRLF / LF).
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

        // NOTE: empty line (or CR-only) terminates the header block.
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
    str::from_utf8(name).ok().map(|s| s.trim())
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
    iter::from_fn(move || {
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
