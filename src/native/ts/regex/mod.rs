//! JavaScript regular expressions as eslint's rules see them: syntax errors
//! (with regexpp's messages), and what is questionable inside valid ones,
//! each with where it is in the pattern.

mod parse;
mod sources;

pub use parse::is_mark;
use parse::{parse, Ast, Kind};

/// Something a rule may report, at characters `start..end` of the pattern.
#[derive(Debug, Clone, PartialEq)]
pub struct Finding {
    /// `empty_class`, `control`, `spaces`, `backref`, `misleading`, or
    /// `misleading_escaped` (what `allowEscape` still reports).
    pub check: &'static str,
    /// The message id or detail (`forward`, `zwj`, `3` spaces).
    pub kind: String,
    /// More detail: control characters (`\x1f, \x1e`), or a backreference
    /// and its group (`\1`, `(a)`, ` and another group`).
    pub data: Vec<String>,
    pub start: usize,
    pub end: usize,
}

/// regexpp's message for an invalid pattern under these flags.
pub fn pattern_error(pattern: &str, unicode: bool, sets: bool) -> Option<String> {
    let flags = format!("{}{}", if unicode { "u" } else { "" }, if sets { "v" } else { "" });
    if unicode && sets {
        return Some("Regex 'u' and 'v' flags cannot be used together".into());
    }
    match parse(pattern, unicode, sets) {
        Ok(_) => None,
        Err((message, _)) => Some(format!("Invalid regular expression: /{pattern}/{flags}: {message}")),
    }
}

/// no-invalid-regexp's flags check: `u` with `v`, duplicates, then
/// unknown flags (`allowed` extra ones pass).
pub fn flags_error(flags: &str, allowed: &str) -> Option<String> {
    // Standard flags are not "allowed" twice.
    let mut extra: Vec<char> = allowed.chars().filter(|c| !"dgimsuvy".contains(*c)).collect();
    extra.dedup();
    let all: Vec<char> = "dgimsuvy".chars().chain(extra).collect();
    let mut rest = flags.to_string();
    for f in &all {
        if let Some(i) = rest.find(*f) {
            rest.remove(i);
        }
    }
    if flags.contains('u') && flags.contains('v') {
        return Some("Regex 'u' and 'v' flags cannot be used together".into());
    }
    let dupes: String = rest.chars().filter(|c| all.contains(c)).collect();
    if !dupes.is_empty() {
        return Some(format!("Duplicate flags ('{dupes}') supplied to RegExp constructor"));
    }
    (!rest.is_empty()).then(|| format!("Invalid flags supplied to RegExp constructor '{rest}'"))
}

/// What the checks find in a pattern. `raw(start, end)` gives the source
/// text of characters `start..end` (through string escapes, for patterns
/// written as strings). Patterns with syntax errors find nothing but
/// control characters (eslint collects those as it parses).
pub fn findings(pattern: &str, flags: &str, literal: bool, raw: &dyn Fn(usize, usize) -> String) -> Vec<Finding> {
    let (unicode, sets) = (flags.contains('u'), flags.contains('v'));
    let (ast, valid) = match parse(pattern, unicode, sets) {
        Ok(ast) => (ast, true),
        Err((_, partial)) => (partial, false),
    };
    let mut out = Vec::new();
    control(&ast, raw, &mut out);
    if !valid {
        return out;
    }
    if literal {
        for (i, n) in ast.nodes.iter().enumerate() {
            if matches!(n.kind, Kind::Class { negate: false, expression: false }) && ast.children(i).next().is_none() {
                out.push(Finding { check: "empty_class", kind: String::new(), data: Vec::new(), start: n.start, end: n.end });
            }
        }
    }
    spaces(&ast, raw, &mut out);
    backrefs(&ast, &mut out);
    misleading(&ast, raw, false, &mut out);
    misleading(&ast, raw, true, &mut out);
    out
}

/// Control characters written as themselves, `\x..` or `\u....`.
fn control(ast: &Ast, raw: &dyn Fn(usize, usize) -> String, out: &mut Vec<Finding>) {
    let mut found = Vec::new();
    let (mut start, mut end) = (usize::MAX, 0);
    for n in &ast.nodes {
        let Kind::Character(v) = n.kind else { continue };
        if v > 0x1F {
            continue;
        }
        let text = raw(n.start, n.end);
        let pattern_text: String = ast.chars[n.start..n.end].iter().collect();
        if pattern_text.chars().next() == char::from_u32(v) || text.starts_with("\\x") || text.starts_with("\\u") || pattern_text.starts_with("\\x") || pattern_text.starts_with("\\u") {
            found.push(format!("\\x{v:02x}"));
            start = start.min(n.start);
            end = end.max(n.end);
        }
    }
    if !found.is_empty() {
        out.push(Finding { check: "control", kind: String::new(), data: vec![found.join(", ")], start, end });
    }
}

/// eslint's no-regex-spaces: the first run of two or more spaces in the
/// pattern's value outside character classes (a last space that a
/// quantifier repeats is not part of it), when the source has two spaces
/// in a row somewhere.
fn spaces(ast: &Ast, raw: &dyn Fn(usize, usize) -> String, out: &mut Vec<Finding>) {
    if !raw(0, ast.chars.len()).contains("  ") {
        return;
    }
    let classes: Vec<(usize, usize)> = ast.nodes.iter().filter(|n| matches!(n.kind, Kind::Class { .. })).map(|n| (n.start, n.end)).collect();
    let c = &ast.chars;
    let mut i = 0;
    while i < c.len() {
        if c[i] != ' ' {
            i += 1;
            continue;
        }
        let start = i;
        while i < c.len() && c[i] == ' ' {
            i += 1;
        }
        let mut len = i - start;
        if c.get(i).is_some_and(|q| "+*{?".contains(*q)) {
            len -= 1;
        }
        if len >= 2 && classes.iter().all(|&(s, e)| start < s || e <= start) {
            out.push(Finding { check: "spaces", kind: len.to_string(), data: Vec::new(), start, end: start + len });
            return;
        }
    }
}

/// eslint's no-useless-backreference.
fn backrefs(ast: &Ast, out: &mut Vec<Finding>) {
    let lookaround = |n: usize| matches!(ast.nodes[n].kind, Kind::Lookaround { .. });
    for (b, n) in ast.nodes.iter().enumerate() {
        let Kind::Backreference(groups) = &n.kind else { continue };
        let bref_path = ast.path(b);
        let mut problems: Vec<(&'static str, usize)> = Vec::new();
        let mut fine = false;
        for &g in groups {
            let group_path = ast.path(g);
            if bref_path.contains(&g) {
                problems.push(("nested", g));
                continue;
            }
            // Paths from the root: the first node where they part.
            let (mut i, mut j) = (bref_path.len() - 1, group_path.len() - 1);
            loop {
                if i == 0 || j == 0 {
                    break;
                }
                i -= 1;
                j -= 1;
                if bref_path[i] != group_path[j] {
                    break;
                }
            }
            let lca = j + 1;
            let group_cut = &group_path[..lca];
            let common = &group_path[lca..];
            let backward = common.iter().find(|&&a| lookaround(a)).is_some_and(|&a| matches!(ast.nodes[a].kind, Kind::Lookaround { behind: true, .. }));
            let problem = if group_cut.last().is_some_and(|&x| matches!(ast.nodes[x].kind, Kind::Alternative)) {
                Some("disjunctive")
            } else if !backward && n.end <= ast.nodes[g].start {
                Some("forward")
            } else if backward && ast.nodes[g].end <= n.start {
                Some("backward")
            } else if group_cut.iter().any(|&x| matches!(ast.nodes[x].kind, Kind::Lookaround { negative: true, .. })) {
                Some("intoNegativeLookaround")
            } else {
                None
            };
            match problem {
                Some(p) => problems.push((p, g)),
                None => fine = true,
            }
        }
        if problems.is_empty() || fine {
            continue;
        }
        let same: Vec<(&str, usize)> = problems.iter().filter(|(p, _)| *p != "disjunctive").copied().collect();
        let report = if same.is_empty() { problems } else { same };
        let (kind, group) = report[0];
        let other = match report.len() - 1 {
            0 => String::new(),
            1 => " and another group".into(),
            k => format!(" and other {k} groups"),
        };
        out.push(Finding { check: "backref", kind: kind.into(), data: vec![ast.raw(b), ast.raw(group), other], start: n.start, end: n.end });
    }
}

/// eslint's no-misleading-character-class, once without and once with
/// `allowEscape`.
fn misleading(ast: &Ast, raw: &dyn Fn(usize, usize) -> String, allow_escape: bool, out: &mut Vec<Finding>) {
    let check = if allow_escape { "misleading_escaped" } else { "misleading" };
    let mut found: Vec<(&'static str, usize, usize)> = Vec::new();
    for (c, n) in ast.nodes.iter().enumerate() {
        if !matches!(n.kind, Kind::Class { expression: false, .. }) {
            continue;
        }
        // Character sequences: ranges and sets interrupt them.
        let mut seqs: Vec<Vec<usize>> = vec![Vec::new()];
        for e in ast.children(c) {
            match ast.nodes[e].kind {
                Kind::Character(_) => seqs.last_mut().expect("one").push(e),
                Kind::ClassRange => {
                    let ends: Vec<usize> = ast.children(e).collect();
                    if let (Some(&min), Some(&max)) = (ends.first(), ends.last()) {
                        seqs.last_mut().expect("one").push(min);
                        seqs.push(vec![max]);
                    }
                }
                _ => seqs.push(Vec::new()),
            }
        }
        for unfiltered in seqs.into_iter().filter(|s| !s.is_empty()) {
            let value = |e: usize| match ast.nodes[e].kind {
                Kind::Character(v) => v,
                _ => 0,
            };
            // Written as an escape other than a backslash before the
            // character itself (eslint's `checkForAcceptableEscape`).
            let escaped = |e: usize| {
                let text = raw(ast.nodes[e].start, ast.nodes[e].end);
                let after = text.trim_start_matches('\\');
                let mut it = after.chars();
                let (Some(only), None) = (it.next(), it.next()) else { return text.starts_with('\\') };
                // `\👍` without `u` escapes the character itself (both halves).
                let itself = only as u32 == value(e) || only as u32 > 0xFFFF && {
                    let v = only as u32 - 0x10000;
                    value(e) == 0xD800 + (v >> 10) || value(e) == 0xDC00 + (v & 0x3FF)
                };
                text.starts_with('\\') && !itself
            };
            let chars: Vec<Option<usize>> = unfiltered.iter().map(|&e| if allow_escape && escaped(e) { None } else { Some(e) }).collect();
            let code_point_escape = |e: usize| {
                let t: String = ast.chars[ast.nodes[e].start..ast.nodes[e].end].iter().collect();
                t.starts_with("\\u{")
            };
            let surrogate = |a: u32, b: u32| (0xD800..=0xDBFF).contains(&a) && (0xDC00..=0xDFFF).contains(&b);
            let is_combining = |v: u32| char::from_u32(v).is_some_and(is_mark);
            let modifier = |v: u32| (0x1F3FB..=0x1F3FF).contains(&v);
            let regional = |v: u32| (0x1F1E6..=0x1F1FF).contains(&v);
            for i in 1..chars.len() {
                let (Some(prev), Some(cur)) = (chars[i - 1], chars[i]) else {
                    // combiningClass looks at the unfiltered previous one.
                    if let Some(cur) = chars[i] {
                        let prev = unfiltered[i - 1];
                        if is_combining(value(cur)) && !is_combining(value(prev)) {
                            found.push(("combiningClass", prev, cur));
                        }
                    }
                    continue;
                };
                let (pv, cv) = (value(prev), value(cur));
                if surrogate(pv, cv) {
                    let escaped_point = code_point_escape(prev) || code_point_escape(cur);
                    found.push((if escaped_point { "surrogatePair" } else { "surrogatePairWithoutUFlag" }, prev, cur));
                }
                if is_combining(cv) && !is_combining(value(unfiltered[i - 1])) {
                    found.push(("combiningClass", unfiltered[i - 1], cur));
                }
                if modifier(cv) && !modifier(pv) {
                    found.push(("emojiModifier", prev, cur));
                }
                if regional(cv) && regional(pv) {
                    found.push(("regionalIndicatorSymbol", prev, cur));
                }
            }
            // Joined sequences: `a ZWJ b ZWJ c` is one.
            let mut seq: Option<(usize, usize)> = None;
            for i in 1..chars.len().saturating_sub(1) {
                let (Some(prev), Some(cur), Some(next)) = (chars[i - 1], chars[i], chars[i + 1]) else { continue };
                if value(cur) == 0x200D && value(prev) != 0x200D && value(next) != 0x200D {
                    match seq {
                        Some((s, last)) if last == prev => seq = Some((s, next)),
                        Some((s, last)) => {
                            found.push(("zwj", s, last));
                            seq = Some((prev, next));
                        }
                        None => seq = Some((prev, next)),
                    }
                }
            }
            if let Some((s, last)) = seq {
                found.push(("zwj", s, last));
            }
        }
    }
    // Grouped by kind, in the order kinds are checked (as eslint reports).
    for kind in ["surrogatePairWithoutUFlag", "surrogatePair", "combiningClass", "emojiModifier", "regionalIndicatorSymbol", "zwj"] {
        for &(k, first, last) in &found {
            if k == kind {
                out.push(Finding { check, kind: kind.into(), data: Vec::new(), start: ast.nodes[first].start, end: ast.nodes[last].end });
            }
        }
    }
}

/// A string literal's value (`"a\nb"` -> `a`, newline, `b`), or `None`
/// for templates with substitutions.
pub fn cook(literal: &str) -> Option<String> {
    cook_spans(literal).map(|(s, _)| s)
}

/// A string literal's value, and for each of its characters the byte span
/// of the source (within `literal`) that wrote it. A lone surrogate escape
/// stays an escape (`\uD83D`): regexes read it the same way.
pub fn cook_spans(literal: &str) -> Option<(String, Vec<(usize, usize)>)> {
    let quote = literal.chars().next()?;
    if !matches!(quote, '"' | '\'' | '`') || literal.len() < 2 {
        return None;
    }
    let body = &literal[1..literal.len() - 1];
    if quote == '`' && body.contains("${") {
        return None;
    }
    let mut out = String::new();
    let mut spans = Vec::new();
    let chars: Vec<(usize, char)> = body.char_indices().map(|(i, c)| (i + 1, c)).collect();
    let mut k = 0;
    let end_of = |k: usize| chars.get(k).map_or(literal.len() - 1, |&(i, _)| i);
    while k < chars.len() {
        let (at, c) = chars[k];
        if c != '\\' {
            out.push(c);
            spans.push((at, end_of(k + 1)));
            k += 1;
            continue;
        }
        let Some(&(_, e)) = chars.get(k + 1) else { break };
        let mut next = k + 2;
        let push = |s: &str, next: usize, out: &mut String, spans: &mut Vec<(usize, usize)>| {
            for ch in s.chars() {
                out.push(ch);
                spans.push((at, end_of(next)));
            }
        };
        match e {
            'n' => push("\n", next, &mut out, &mut spans),
            't' => push("\t", next, &mut out, &mut spans),
            'r' => push("\r", next, &mut out, &mut spans),
            'b' => push("\u{8}", next, &mut out, &mut spans),
            'f' => push("\u{c}", next, &mut out, &mut spans),
            'v' => push("\u{b}", next, &mut out, &mut spans),
            '0' if !chars.get(next).is_some_and(|&(_, d)| d.is_ascii_digit()) => push("\0", next, &mut out, &mut spans),
            // Legacy octal escapes (`\60` is `0`); `\8` and `\9` are digits.
            '0'..='7' => {
                let mut v = e.to_digit(8).unwrap_or(0);
                let mut digits = 1;
                while let Some(d) = chars.get(next).and_then(|&(_, d)| d.to_digit(8)) {
                    if digits == 3 || v * 8 + d > 0o377 {
                        break;
                    }
                    v = v * 8 + d;
                    next += 1;
                    digits += 1;
                }
                push(&char::from_u32(v)?.to_string(), next, &mut out, &mut spans);
            }
            '\r' => {
                if chars.get(next).is_some_and(|&(_, c)| c == '\n') {
                    next += 1;
                }
            }
            '\n' | '\u{2028}' | '\u{2029}' => {}
            'x' => {
                let s: String = chars.get(next..next + 2)?.iter().map(|&(_, c)| c).collect();
                next += 2;
                let ch = char::from_u32(u32::from_str_radix(&s, 16).ok()?)?;
                push(&ch.to_string(), next, &mut out, &mut spans);
            }
            'u' => {
                let v = if chars.get(next).is_some_and(|&(_, c)| c == '{') {
                    let close = chars[next..].iter().position(|&(_, c)| c == '}')?;
                    let s: String = chars[next + 1..next + close].iter().map(|&(_, c)| c).collect();
                    next += close + 1;
                    u32::from_str_radix(&s, 16).ok()?
                } else {
                    let s: String = chars.get(next..next + 4)?.iter().map(|&(_, c)| c).collect();
                    next += 4;
                    u32::from_str_radix(&s, 16).ok()?
                };
                // A surrogate pair written as two escapes is one character.
                let pair = if (0xD800..=0xDBFF).contains(&v) && chars.get(next).is_some_and(|&(_, c)| c == '\\') && chars.get(next + 1).is_some_and(|&(_, c)| c == 'u') {
                    let s: String = chars.get(next + 2..next + 6).map(|w| w.iter().map(|&(_, c)| c).collect()).unwrap_or_default();
                    u32::from_str_radix(&s, 16).ok().filter(|lo| (0xDC00..=0xDFFF).contains(lo)).map(|lo| 0x10000 + ((v - 0xD800) << 10) + (lo - 0xDC00))
                } else {
                    None
                };
                match pair.or(Some(v)).and_then(char::from_u32) {
                    Some(ch) => {
                        if pair.is_some() {
                            next += 6;
                        }
                        push(&ch.to_string(), next, &mut out, &mut spans);
                    }
                    // Lone surrogates have no `char`: keep the escape.
                    None => push(&format!("\\u{v:04X}"), next, &mut out, &mut spans),
                }
            }
            e => push(&e.to_string(), next, &mut out, &mut spans),
        }
        k = next;
    }
    Some((out, spans))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn found(pattern: &str, flags: &str) -> Vec<(String, String, Vec<String>)> {
        let chars: Vec<char> = pattern.chars().collect();
        let raw = |s: usize, e: usize| chars[s..e].iter().collect::<String>();
        findings(pattern, flags, true, &raw).into_iter().map(|f| (f.check.to_string(), f.kind, f.data)).collect()
    }

    fn kinds(pattern: &str, flags: &str) -> Vec<String> {
        found(pattern, flags).into_iter().filter(|(c, _, _)| c != "misleading_escaped").map(|(c, k, _)| if k.is_empty() { c } else { format!("{c}:{k}") }).collect()
    }

    #[test]
    fn errors_name_what_is_wrong() {
        assert!(pattern_error("a(b)c", false, false).is_none());
        assert_eq!(pattern_error("(", false, false).as_deref(), Some("Invalid regular expression: /(/: Unterminated group"));
        assert_eq!(pattern_error("a)", false, false).as_deref(), Some("Invalid regular expression: /a)/: Unmatched ')'"));
        assert_eq!(pattern_error("*a", false, false).as_deref(), Some("Invalid regular expression: /*a/: Nothing to repeat"));
        assert_eq!(pattern_error("[b-a]", true, false).as_deref(), Some("Invalid regular expression: /[b-a]/u: Range out of order in character class"));
        assert_eq!(pattern_error("[a", false, false).as_deref(), Some("Invalid regular expression: /[a/: Unterminated character class"));
        assert!(pattern_error("a{", false, false).is_none());
        assert!(pattern_error("a{", true, false).is_some());
        assert!(pattern_error("\\1(a)", false, false).is_none());
        // ES2025: one name in different alternatives.
        assert!(pattern_error("(?<k>a)|(?<k>b)", false, false).is_none());
        assert!(pattern_error("(?<k>a)(?<k>b)", false, false).is_some());
        assert!(pattern_error("(?<\\u{1d49c}>.)", false, false).is_none());
        // `v`: set operations, string disjunctions, nested classes.
        assert!(pattern_error("[A--B]", false, true).is_none());
        assert!(pattern_error("[A--[0-9]]", false, true).is_none());
        assert!(pattern_error("[\\p{Basic_Emoji}--\\q{a|bc|def}]", false, true).is_none());
        assert!(pattern_error("[^\\q{ab}]", false, true).unwrap().contains("Negated character class may contain strings"));
        assert!(pattern_error("[a-z--b]", false, true).unwrap().contains("Invalid set operation"));
        assert!(pattern_error("[(]", false, true).unwrap().contains("Invalid character in character class"));
        assert!(pattern_error("\\p{Foo}", true, false).unwrap().contains("Invalid property name"));
        assert!(pattern_error("\\p{Script=Greek}", true, false).is_none());
    }

    #[test]
    fn flags_are_checked_like_the_constructor() {
        assert_eq!(flags_error("gg", "").as_deref(), Some("Duplicate flags ('g') supplied to RegExp constructor"));
        assert_eq!(flags_error("z", "").as_deref(), Some("Invalid flags supplied to RegExp constructor 'z'"));
        assert_eq!(flags_error("uv", "").as_deref(), Some("Regex 'u' and 'v' flags cannot be used together"));
        assert!(flags_error("gz", "z").is_none());
    }

    #[test]
    fn findings_follow_eslint() {
        assert_eq!(kinds("a[]b", ""), vec!["empty_class"]);
        assert!(kinds("a[^]b", "").is_empty());
        assert_eq!(kinds("[a--[]]", "v"), vec!["empty_class"]);
        assert_eq!(kinds("a   b", ""), vec!["spaces:3"]);
        assert_eq!(kinds("a   +b", ""), vec!["spaces:2"]);
        assert_eq!(found("\\1(a)", ""), vec![("backref".into(), "forward".into(), vec!["\\1".into(), "(a)".into(), String::new()])]);
        assert_eq!(kinds("(a\\1)", ""), vec!["backref:nested"]);
        assert_eq!(kinds("(a)|\\1", ""), vec!["backref:disjunctive"]);
        assert_eq!(kinds("(?<=(a)\\1)", ""), vec!["backref:backward"]);
        assert_eq!(kinds("(?!(a))\\1", ""), vec!["backref:intoNegativeLookaround"]);
        // Different lookbehinds: matching forward between them.
        assert_eq!(kinds("(?<=\\1)(?<=(a))", ""), vec!["backref:forward"]);
        assert_eq!(kinds("\\1(a)\\2(b)", ""), vec!["backref:forward", "backref:forward"]);
        assert!(kinds("(a)\\1", "").is_empty());
        assert_eq!(kinds("[👍]", ""), vec!["misleading:surrogatePairWithoutUFlag"]);
        assert!(kinds("[👍]", "u").is_empty());
        assert_eq!(kinds("[\u{1F1EF}\u{1F1F5}]", "u"), vec!["misleading:regionalIndicatorSymbol"]);
        assert!(kinds("[\u{1F1EF}[A]\u{1F1F5}]", "v").is_empty());
        assert_eq!(found("\\x1f\\x1e", "")[0].2, vec!["\\x1f, \\x1e".to_string()]);
    }

    #[test]
    fn strings_cook_escapes_with_their_spans() {
        assert_eq!(cook(r#""a\nb\\c""#).as_deref(), Some("a\nb\\c"));
        assert_eq!(cook(r#"'A\x42'"#).as_deref(), Some("AB"));
        assert_eq!(cook("`a${b}`"), None);
        let (s, spans) = cook_spans(r#"'a\x42c'"#).unwrap();
        assert_eq!(s, "aBc");
        assert_eq!(spans, vec![(1, 2), (2, 6), (6, 7)]);
        let (s, _) = cook_spans(r#"'👍'"#).unwrap();
        assert_eq!(s, "👍");
    }
}
