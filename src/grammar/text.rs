//! Byte-level text helpers shared by the interpreter. Zero-width assertions
//! use exactly regex-syntax's definitions, so a pattern means the same thing
//! whether the DFA or the interpreter runs it.

use crate::pattern::Look;

#[inline]
pub fn utf8_len(b: u8) -> usize {
    match b {
        0..=0x7F => 1,
        0xC0..=0xDF => 2,
        0xE0..=0xEF => 3,
        _ => 4,
    }
}

#[inline]
pub fn decode(text: &[u8], pos: usize) -> (char, usize) {
    let b = text[pos];
    if b < 0x80 {
        return (b as char, 1);
    }
    let len = utf8_len(b).min(text.len() - pos);
    match std::str::from_utf8(&text[pos..pos + len]).ok().and_then(|s| s.chars().next()) {
        Some(c) => (c, len),
        None => ('\u{FFFD}', 1),
    }
}

fn is_word(c: char) -> bool {
    regex_syntax::is_word_character(c)
}

fn word_before(text: &[u8], pos: usize) -> bool {
    if pos == 0 {
        return false;
    }
    let mut start = pos - 1;
    while start > 0 && (0x80..0xC0).contains(&text[start]) {
        start -= 1;
    }
    is_word(decode(text, start).0)
}

fn word_after(text: &[u8], pos: usize, limit: usize) -> bool {
    pos < limit && is_word(decode(text, pos).0)
}

/// Does `look` hold at `pos`, with `limit` as the end of the text?
pub fn look(text: &[u8], pos: usize, limit: usize, look: Look) -> bool {
    match look {
        Look::Bol => pos == 0 || text[pos - 1] == b'\n',
        // Like regex's `(?mR)$`: before `\r` or `\n`, never inside `\r\n`.
        Look::Eol => pos >= limit || text[pos] == b'\r' || (text[pos] == b'\n' && (pos == 0 || text[pos - 1] != b'\r')),
        Look::Bof => pos == 0,
        Look::Eof => pos >= limit,
        Look::WordBoundary => word_before(text, pos) != word_after(text, pos, limit),
        Look::NotWordBoundary => word_before(text, pos) == word_after(text, pos, limit),
    }
}
