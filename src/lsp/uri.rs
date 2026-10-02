//! `file://` URIs <-> paths, producing the same canonical form VS Code uses
//! (`file:///c%3A/dir/a%20b.rs`) so published URIs match open editors.

use std::path::{Path, PathBuf};

use zoomie_lint::config::source::normalize_path;

pub fn to_path(uri: &str) -> Option<PathBuf> {
    let rest = uri.strip_prefix("file://")?;
    let decoded = percent_decode(rest)?;
    // `/c:/x` on Windows; `//server/share` stays as is.
    let path = match decoded.as_bytes() {
        [b'/', d, b':', ..] if d.is_ascii_alphabetic() && cfg!(windows) => decoded[1..].to_string(),
        _ => decoded,
    };
    Some(normalize_path(Path::new(&path)))
}

pub fn from_path(path: &Path) -> String {
    let s = path.to_string_lossy().replace('\\', "/");
    let mut out = String::from("file://");
    let s = match s.as_bytes() {
        [d, b':', ..] if d.is_ascii_alphabetic() => {
            out.push('/');
            out.push((*d as char).to_ascii_lowercase());
            out.push_str("%3A");
            s[2..].to_string()
        }
        _ => s,
    };
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' | b'/' => out.push(b as char),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

fn percent_decode(s: &str) -> Option<String> {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = std::str::from_utf8(bytes.get(i + 1..i + 3)?).ok()?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg(windows)]
    fn windows_round_trip() {
        let p = to_path("file:///c%3A/git/a%20b.rs").unwrap();
        assert_eq!(p, PathBuf::from(r"C:\git\a b.rs"));
        assert_eq!(from_path(&p), "file:///c%3A/git/a%20b.rs");
    }

    #[test]
    #[cfg(not(windows))]
    fn unix_round_trip() {
        let p = to_path("file:///tmp/a%20b.rs").unwrap();
        assert_eq!(p, PathBuf::from("/tmp/a b.rs"));
        assert_eq!(from_path(&p), "file:///tmp/a%20b.rs");
    }
}
