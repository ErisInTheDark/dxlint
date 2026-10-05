//! JavaScript syntax translation; unsupported constructs fail at this boundary.

pub(super) struct Pattern {
    pub source: String,
    pub multiline: bool,
    pub assertions: Vec<(Pattern, bool)>,
}

pub(super) fn pattern(source: &str, flags: &str, allow_assertions: bool) -> Result<Pattern, String> {
    let mut result = Pattern { source: String::new(), multiline: false, assertions: vec![] };
    let out = &mut result.source;
    let mut chars = source.chars().peekable();
    let mut class = false;
    let mut repetition = 0;
    let mut atom = false;
    while let Some(c) = chars.next() {
        let previous_repetition = repetition;
        let previous_atom = atom;
        if !class {
            if matches!(c, '*' | '+' | '?') {
                if !atom || repetition == 2 || repetition == 1 && c != '?' { return Err("invalid JavaScript regex repetition".into()); }
                repetition = if repetition == 1 { 2 } else { 1 };
            } else {
                repetition = 0;
                atom = !matches!(c, '(' | '|' | '^' | '$');
            }
        }
        match c {
            '(' if !class && chars.peek() == Some(&'?') => {
                let tail: String = chars.clone().take(2).collect();
                if matches!(tail.as_str(), "?=" | "?!") {
                    if !allow_assertions { return Err("nested JavaScript regex assertions are unsupported".into()); }
                    chars.next();
                    let positive = chars.next() == Some('=');
                    let body = assertion_body(&mut chars)?;
                    atom = true;
                    if flags.contains('u') && chars.peek().is_some_and(|c| matches!(c, '*' | '+' | '?' | '{')) {
                        return Err("quantified JavaScript regex assertions are invalid in Unicode mode".into());
                    }
                    if result.assertions.len() >= 64 { return Err("JavaScript regex exceeds 64 assertions".into()); }
                    out.push_str(&format!("(?P<zlint_assertion_{}>)", result.assertions.len()));
                    result.assertions.push((pattern(&body, flags, false)?, positive));
                } else if tail.starts_with("?:") {
                    chars.next();
                    chars.next();
                    out.push_str("(?:");
                } else {
                    return Err("JavaScript regex lookbehind and named groups are unsupported".into());
                }
            }
            '[' if class => out.push_str(r"\["),
            '[' => { class = true; out.push(c); }
            ']' => { class = false; out.push(c); }
            '.' if !class => out.push_str(if flags.contains('s') { r"(?s:.)" } else { r"[^\n\r\u{2028}\u{2029}]" }),
            '^' if !class => { result.multiline |= flags.contains('m'); out.push_str(if flags.contains('m') { "(?m:^)" } else { r"\A" }); }
            '$' if !class => { result.multiline |= flags.contains('m'); out.push_str(if flags.contains('m') { "(?m:$)" } else { r"\z" }); }
            '{' if !class => {
                if let Some(body) = quantifier_body(&mut chars) {
                    if previous_repetition != 0 || !previous_atom { return Err("invalid JavaScript regex repetition".into()); }
                    repetition = 1;
                    out.push('{');
                    out.push_str(&body);
                } else { out.push(c); }
            }
            '\\' => {
                let escaped = chars.next().ok_or("unterminated JavaScript regex escape")?;
                match escaped {
                    '1'..='9' | 'k' => return Err("JavaScript regex backreferences are unsupported".into()),
                    'c' => return Err("JavaScript regex control escapes are unsupported".into()),
                    'd' => out.push_str(if class { "0-9" } else { "[0-9]" }),
                    'w' => out.push_str(if class { "A-Za-z0-9_" } else { "[A-Za-z0-9_]" }),
                    's' => out.push_str(if class { r"\t\n\r\x0B\x0C \u{a0}\u{1680}\u{2000}-\u{200a}\u{2028}\u{2029}\u{202f}\u{205f}\u{3000}\u{feff}" } else { r"[\t\n\r\x0B\x0C \u{a0}\u{1680}\u{2000}-\u{200a}\u{2028}\u{2029}\u{202f}\u{205f}\u{3000}\u{feff}]" }),
                    'D' => out.push_str("[^0-9]"),
                    'W' => out.push_str("[^A-Za-z0-9_]"),
                    'S' => out.push_str(r"[^\t\n\r\x0B\x0C \u{a0}\u{1680}\u{2000}-\u{200a}\u{2028}\u{2029}\u{202f}\u{205f}\u{3000}\u{feff}]"),
                    'b' | 'B' if !class => {
                        atom = false;
                        if flags.contains('i') && flags.contains('u') { return Err("Unicode case-insensitive JavaScript regex word boundaries are unsupported".into()); }
                        out.push_str("(?-u:\\"); out.push(escaped); out.push(')');
                    }
                    'b' => out.push_str(r"\x08"),
                    'u' => {
                        let mut hex = String::new();
                        if flags.contains('u') && chars.peek() == Some(&'{') {
                            chars.next();
                            loop {
                                match chars.next() {
                                    Some('}') => break,
                                    Some(h) => hex.push(h),
                                    None => return Err("unterminated Unicode regex escape".into()),
                                }
                            }
                        } else {
                            for _ in 0..4 { hex.push(chars.next().ok_or("incomplete Unicode regex escape")?); }
                        }
                        let value = u32::from_str_radix(&hex, 16).map_err(|_| "invalid Unicode regex escape")?;
                        let c = char::from_u32(value).ok_or("JavaScript regex surrogate escapes are unsupported")?;
                        if flags.contains('i') && !flags.contains('u') && super::legacy_gap(c) { return Err("unsupported legacy JavaScript regex case-fold group; use u".into()); }
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
    Ok(result)
}

fn quantifier_body(chars: &mut std::iter::Peekable<std::str::Chars<'_>>) -> Option<String> {
    let mut probe = chars.clone();
    let mut body = String::new();
    while probe.peek().is_some_and(char::is_ascii_digit) { body.push(probe.next()?); }
    if body.is_empty() { return None; }
    if probe.peek() == Some(&',') {
        body.push(probe.next()?);
        while probe.peek().is_some_and(char::is_ascii_digit) { body.push(probe.next()?); }
    }
    if probe.next()? != '}' { return None; }
    body.push('}');
    *chars = probe;
    Some(body)
}

fn assertion_body(chars: &mut std::iter::Peekable<std::str::Chars<'_>>) -> Result<String, String> {
    let mut body = String::new();
    let mut depth = 1;
    let mut class = false;
    while let Some(c) = chars.next() {
        match c {
            '\\' => {
                body.push(c);
                body.push(chars.next().ok_or("unterminated JavaScript regex assertion escape")?);
                continue;
            }
            '[' => class = true,
            ']' => class = false,
            '(' if !class => { depth += 1; if depth > 128 { return Err("JavaScript regex nesting exceeds 128".into()); } }
            ')' if !class => { depth -= 1; if depth == 0 { return Ok(body); } }
            _ => {}
        }
        body.push(c);
    }
    Err("unterminated JavaScript regex assertion".into())
}
