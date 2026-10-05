//! JavaScript regex syntax stays at the selector input boundary.

use regex_automata::meta;

pub(super) struct Regex {
    inner: meta::Regex,
    multiline: bool,
    legacy_case: bool,
}

impl Regex {
    pub(super) fn compile(pattern: &str, flags: &str) -> Result<Self, String> {
        let mut seen = String::new();
        for flag in flags.chars() {
            if !matches!(flag, 'i' | 'm' | 's' | 'u') || seen.contains(flag) {
                return Err(format!("unsupported or repeated selector regex flag: {flag}"));
            }
            seen.push(flag);
        }
        let mut multiline = false;
        let legacy_case = flags.contains('i') && !flags.contains('u');
        if legacy_case && !pattern.is_ascii() { return Err("non-ASCII legacy selector regex case folding is unsupported; use the u flag".into()); }
        let mut out = String::new();
        let mut chars = pattern.chars().peekable();
        let mut class = false;
        while let Some(c) = chars.next() {
            match c {
                '(' if chars.peek() == Some(&'?') => {
                    let tail: String = chars.clone().take(3).collect();
                    if !tail.starts_with("?:") { return Err("selector regex lookarounds and named groups are unsupported".into()); }
                    out.push(c);
                }
                '[' if class => out.push_str(r"\["),
                '[' => { class = true; out.push(c); }
                ']' => { class = false; out.push(c); }
                '.' if !class => out.push_str(if flags.contains('s') { r"(?s:.)" } else { r"[^\n\r\u{2028}\u{2029}]" }),
                '^' if !class => { multiline |= flags.contains('m'); out.push_str(if flags.contains('m') { "(?m:^)" } else { r"\A" }); }
                '$' if !class => { multiline |= flags.contains('m'); out.push_str(if flags.contains('m') { "(?m:$)" } else { r"\z" }); }
                '\\' => {
                    let escaped = chars.next().ok_or("unterminated selector regex escape")?;
                    match escaped {
                        '1'..='9' | 'k' => return Err("selector regex backreferences are unsupported".into()),
                        'd' => out.push_str(if class { "0-9" } else { "[0-9]" }),
                        'w' => out.push_str(if class { "A-Za-z0-9_" } else { "[A-Za-z0-9_]" }),
                        's' => out.push_str(if class { r"\t\n\r\x0B\x0C \u{a0}\u{1680}\u{2000}-\u{200a}\u{2028}\u{2029}\u{202f}\u{205f}\u{3000}\u{feff}" } else { r"[\t\n\r\x0B\x0C \u{a0}\u{1680}\u{2000}-\u{200a}\u{2028}\u{2029}\u{202f}\u{205f}\u{3000}\u{feff}]" }),
                        'D' => out.push_str("[^0-9]"),
                        'W' => out.push_str("[^A-Za-z0-9_]"),
                        'S' => out.push_str(r"[^\t\n\r\x0B\x0C \u{a0}\u{1680}\u{2000}-\u{200a}\u{2028}\u{2029}\u{202f}\u{205f}\u{3000}\u{feff}]"),
                        'b' | 'B' if !class => {
                            if flags.contains('i') && flags.contains('u') { return Err("Unicode case-insensitive selector regex word boundaries are unsupported".into()); }
                            out.push_str("(?-u:\\"); out.push(escaped); out.push(')');
                        }
                        'b' => out.push_str(r"\x08"),
                        'u' => {
                            let mut hex = String::new();
                            if flags.contains('u') && chars.peek() == Some(&'{') {
                                chars.next();
                                for h in chars.by_ref() { if h == '}' { break; } hex.push(h); }
                            } else {
                                for _ in 0..4 { hex.push(chars.next().ok_or("incomplete Unicode regex escape")?); }
                            }
                            let value = u32::from_str_radix(&hex, 16).map_err(|_| "invalid Unicode regex escape")?;
                            if char::from_u32(value).is_none() { return Err("selector regex surrogate escapes are unsupported".into()); }
                            if legacy_case && value > 127 { return Err("non-ASCII legacy selector regex case folding is unsupported; use the u flag".into()); }
                            out.push_str(&format!("\\u{{{hex}}}"));
                        }
                        '/' => out.push('/'),
                        c if c.is_ascii_alphabetic() && !matches!(c, 'n' | 'r' | 't' | 'f' | 'v' | 'x') => {
                            if flags.contains('u') { return Err(format!("invalid Unicode regex escape: \\{c}")); }
                            out.push(c);
                        }
                        c => { out.push('\\'); out.push(c); }
                    }
                }
                c => out.push(c),
            }
        }
        let inner = meta::Regex::builder()
            .syntax(regex_automata::util::syntax::Config::new().case_insensitive(flags.contains('i')))
            .build(&out).map_err(|e| format!("invalid selector regex: {e}"))?;
        Ok(Self { inner, multiline, legacy_case })
    }
    pub(super) fn matches(&self, text: &str) -> Result<bool, String> {
        if self.multiline && text.contains(['\r', '\u{2028}', '\u{2029}']) {
            return Err("selector regex multiline anchors on CR or Unicode line separators are unsupported".into());
        }
        if self.legacy_case && !text.is_ascii() {
            return Err("non-ASCII legacy selector regex case folding is unsupported; use the u flag".into());
        }
        Ok(self.inner.is_match(text))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn javascript_character_classes_and_line_endings() {
        assert!(!Regex::compile(r"\w", "").unwrap().matches("é").unwrap());
        assert!(!Regex::compile(r"\d", "").unwrap().matches("١").unwrap());
        let whitespace = Regex::compile(r"^\s$", "").unwrap();
        assert!(whitespace.matches("\u{feff}").unwrap());
        assert!(!whitespace.matches("\u{85}").unwrap());
        let dot = Regex::compile(r"^.$", "").unwrap();
        assert!(!dot.matches("\r").unwrap());
        assert!(!dot.matches("\u{2028}").unwrap());
        assert!(!Regex::compile("end$", "").unwrap().matches("end\r\n").unwrap());
    }

    #[test]
    fn supported_multiline_is_not_silently_approximated() {
        let multiline = Regex::compile("^word$", "m").unwrap();
        assert!(multiline.matches("before\nword\nafter").unwrap());
        assert!(multiline.matches("before\rword\rafter").is_err());
    }

    #[test]
    fn legacy_case_folding_is_not_unicode_case_folding() {
        assert!(Regex::compile("k", "iu").unwrap().matches("K").unwrap());
        assert!(Regex::compile("k", "i").unwrap().matches("K").is_err());
    }
}
