//! Pure parsers and serialisers for the `dovecot-keywords` sidecar
//! file dropped at the root of a Maildir by Dovecot / mbsync, mapping
//! single lowercase letters `a..z` to keyword names.

use core::fmt::Write as _;

use alloc::{
    collections::{BTreeMap, btree_map::Entry},
    string::{String, ToString},
};

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

        let _ = writeln!(out, "{n} {name}");
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
        if let Entry::Vacant(slot) = table.entry(letter) {
            slot.insert(keyword.to_string());
            return Some(letter);
        }
    }

    None
}

fn letter_to_slot(c: char) -> Option<u8> {
    let b = c as u32;
    if b >= SLOT_MIN as u32 && b < (SLOT_MIN + SLOT_COUNT) as u32 {
        Some((b as u8) - SLOT_MIN)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use crate::dovecot::utils::*;

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
        assert!(!table.contains_key(&'b'));
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
}
