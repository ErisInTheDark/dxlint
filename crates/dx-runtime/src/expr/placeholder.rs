//! `placeholder(name)`: a name the user is expected to choose. Fix text
//! carries it between two Unicode noncharacters (reserved for exactly this
//! kind of internal use); the engine's `Fix::from_regions` strips them and
//! records where each placeholder lands.

use std::ops::Range;

const OPEN: char = '\u{FDD0}';
const CLOSE: char = '\u{FDD1}';

pub fn mark(name: &str) -> String {
    format!("{OPEN}{name}{CLOSE}")
}

/// The text without marks, and each placeholder's range in it.
pub fn strip(text: &str) -> (String, Vec<Range<usize>>) {
    if !text.contains(OPEN) {
        return (text.to_string(), Vec::new());
    }
    let mut out = String::with_capacity(text.len());
    let mut marks = Vec::new();
    let mut open = None;
    for c in text.chars() {
        match c {
            OPEN => open = Some(out.len()),
            CLOSE => {
                if let Some(start) = open.take() {
                    marks.push(start..out.len());
                }
            }
            c => out.push(c),
        }
    }
    (out, marks)
}
