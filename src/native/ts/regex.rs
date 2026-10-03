//! JavaScript regular expressions as rules see them: whether a pattern is
//! valid (with the messages ESLint's validator gives), and structural
//! problems inside it (empty classes, runs of spaces, useless
//! backreferences, misleading character classes).

/// Syntax error message, as `Invalid regular expression: /p/u: message`.
pub fn error(pattern: &str, flags: &str) -> Option<String> {
    if flags.chars().any(|c| !"dgimsuvy".contains(c)) {
        return Some(format!("Invalid flags supplied to RegExp constructor '{flags}'"));
    }
    let dupes: String = "dgimsuvy".chars().filter(|&c| flags.matches(c).count() > 1).collect();
    if !dupes.is_empty() {
        return Some(format!("Duplicate flags ('{dupes}') supplied to RegExp constructor"));
    }
    if flags.contains('u') && flags.contains('v') {
        return Some("Regex 'u' and 'v' flags cannot be used together".into());
    }
    let suffix = if flags.contains('v') {
        "v"
    } else if flags.contains('u') {
        "u"
    } else {
        ""
    };
    match Parser::new(pattern, flags).parse() {
        Ok(_) => None,
        Err(message) => Some(format!("Invalid regular expression: /{pattern}/{suffix}: {message}")),
    }
}

/// Problems in a valid pattern, each `kind` or `kind:detail`:
/// `empty_class`, `spaces:N`, `backref:<kind>:<backref>:<group>`,
/// `misleading:<kind>`.
pub fn problems(pattern: &str, flags: &str, literal: bool) -> Vec<String> {
    let Ok(tree) = Parser::new(pattern, flags).parse() else { return Vec::new() };
    let mut out = Vec::new();
    if literal && tree.nodes.iter().any(|n| matches!(n.kind, Nk::Class { empty: true })) {
        out.push("empty_class".into());
    }
    if let Some(run) = tree.space_run {
        out.push(format!("spaces:{run}"));
    }
    for (i, n) in tree.nodes.iter().enumerate() {
        if let Nk::Backref(target) = &n.kind {
            for &g in target {
                if let Some(kind) = tree.useless(i, g) {
                    out.push(format!("backref:{kind}:{}:{}", tree.text(i), tree.text(g)));
                    break;
                }
            }
        }
    }
    let unicode = flags.contains('u') || flags.contains('v');
    for kind in tree.misleading(unicode) {
        let item = format!("misleading:{kind}");
        if !out.contains(&item) {
            out.push(item);
        }
    }
    out
}

#[derive(Debug)]
enum Nk {
    Pattern,
    Alt,
    Group,
    Look { behind: bool, negative: bool },
    Class { empty: bool },
    /// Indices of the groups it can reference.
    Backref(Vec<usize>),
}

#[derive(Debug)]
struct Node {
    kind: Nk,
    parent: Option<usize>,
    start: usize,
    end: usize,
}

struct Tree {
    chars: Vec<char>,
    nodes: Vec<Node>,
    /// Characters directly inside each class, by class node.
    class_chars: Vec<(usize, Vec<char>)>,
    /// The first run of two or more literal spaces outside classes.
    space_run: Option<usize>,
}

impl Tree {
    fn text(&self, n: usize) -> String {
        self.chars[self.nodes[n].start..self.nodes[n].end].iter().collect()
    }

    fn ancestors(&self, mut n: usize) -> Vec<usize> {
        let mut out = vec![n];
        while let Some(p) = self.nodes[n].parent {
            out.push(p);
            n = p;
        }
        out
    }

    /// Why backreference `b` to group `g` can never match anything.
    fn useless(&self, b: usize, g: usize) -> Option<&'static str> {
        let bp = self.ancestors(b);
        let gp = self.ancestors(g);
        if bp.contains(&g) {
            return Some("nested");
        }
        let common = *gp.iter().find(|a| bp.contains(a))?;
        // Different alternatives below the common ancestor.
        let below = |path: &[usize]| path.iter().position(|&a| a == common).and_then(|i| i.checked_sub(1)).map(|i| path[i]);
        if let (Some(x), Some(y)) = (below(&bp), below(&gp)) {
            if matches!(self.nodes[x].kind, Nk::Alt) && matches!(self.nodes[y].kind, Nk::Alt) && x != y {
                return Some("disjunctive");
            }
        }
        let behind = bp.iter().find_map(|&a| match self.nodes[a].kind {
            Nk::Look { behind, .. } => Some(behind),
            _ => None,
        });
        let (bs, gs, ge) = (self.nodes[b].start, self.nodes[g].start, self.nodes[g].end);
        if behind == Some(true) {
            if ge <= bs {
                return Some("backward");
            }
        } else if gs > bs {
            return Some("forward");
        }
        let into_negative = gp.iter().take_while(|&&a| a != common).any(|&a| matches!(self.nodes[a].kind, Nk::Look { negative: true, .. }));
        into_negative.then_some("intoNegativeLookaround")
    }

    fn misleading(&self, unicode: bool) -> Vec<&'static str> {
        let mut out = Vec::new();
        for (_, chars) in &self.class_chars {
            for (i, &c) in chars.iter().enumerate() {
                let next = chars.get(i + 1).copied();
                if !unicode && (c as u32) > 0xFFFF {
                    out.push("surrogatePairWithoutUFlag");
                }
                if i > 0 && is_combining(c) && !is_combining(chars[i - 1]) {
                    out.push("combiningClass");
                }
                if i > 0 && (0x1F3FB..=0x1F3FF).contains(&(c as u32)) && is_emoji(chars[i - 1]) {
                    out.push("emojiModifier");
                }
                if is_regional(c) && next.is_some_and(is_regional) {
                    out.push("regionalIndicatorSymbol");
                }
                if c == '\u{200D}' && i > 0 && next.is_some() {
                    out.push("zwj");
                }
            }
        }
        out
    }
}

fn is_regional(c: char) -> bool {
    (0x1F1E6..=0x1F1FF).contains(&(c as u32))
}

fn is_emoji(c: char) -> bool {
    let c = c as u32;
    (0x1F300..=0x1FAFF).contains(&c) || (0x2600..=0x27BF).contains(&c)
}

/// Combining marks (the common blocks of Unicode categories Mn/Mc/Me).
fn is_combining(c: char) -> bool {
    let c = c as u32;
    matches!(c, 0x0300..=0x036F | 0x0483..=0x0489 | 0x0591..=0x05BD | 0x0610..=0x061A | 0x064B..=0x065F | 0x0900..=0x0903 | 0x093A..=0x094F
        | 0x0951..=0x0957 | 0x0962..=0x0963 | 0x0981..=0x0983 | 0x09BC..=0x09D7 | 0x0E31 | 0x0E34..=0x0E3A | 0x0E47..=0x0E4E
        | 0x1AB0..=0x1AFF | 0x1DC0..=0x1DFF | 0x20D0..=0x20FF | 0x302A..=0x302F | 0x3099..=0x309A | 0xFE00..=0xFE0F | 0xFE20..=0xFE2F
        | 0xE0100..=0xE01EF)
}

struct Parser {
    c: Vec<char>,
    i: usize,
    unicode: bool,
    sets: bool,
    named: bool,
    nodes: Vec<Node>,
    groups: Vec<(usize, Option<String>)>,
    backrefs: Vec<(usize, Result<u32, String>)>,
    class_chars: Vec<(usize, Vec<char>)>,
    space_run: Option<usize>,
}

type R<T> = Result<T, String>;

impl Parser {
    fn new(pattern: &str, flags: &str) -> Self {
        let c: Vec<char> = pattern.chars().collect();
        // `\k` is a named reference only when the pattern has named groups
        // (outside unicode mode).
        let named = c.windows(4).any(|w| w[..3] == ['(', '?', '<'] && w[3] != '=' && w[3] != '!');
        Parser {
            c,
            i: 0,
            unicode: flags.contains('u') || flags.contains('v'),
            sets: flags.contains('v'),
            named,
            nodes: Vec::new(),
            groups: Vec::new(),
            backrefs: Vec::new(),
            class_chars: Vec::new(),
            space_run: None,
        }
    }

    fn peek(&self) -> Option<char> {
        self.c.get(self.i).copied()
    }

    fn at(&self, s: &str) -> bool {
        s.chars().enumerate().all(|(k, ch)| self.c.get(self.i + k) == Some(&ch))
    }

    fn push(&mut self, kind: Nk, parent: Option<usize>, start: usize) -> usize {
        self.nodes.push(Node { kind, parent, start, end: start });
        self.nodes.len() - 1
    }

    fn parse(mut self) -> R<Tree> {
        let root = self.push(Nk::Pattern, None, 0);
        self.disjunction(root)?;
        if self.i < self.c.len() {
            return Err(if self.peek() == Some(')') { "Unmatched ')'".into() } else { "Unexpected character".into() });
        }
        self.nodes[root].end = self.c.len();
        // Resolve backreferences now that every group is known.
        let count = self.groups.len() as u32;
        for (node, target) in std::mem::take(&mut self.backrefs) {
            let groups: Vec<usize> = match target {
                Ok(n) if n >= 1 && n <= count => vec![self.groups[n as usize - 1].0],
                Ok(_) if self.unicode => return Err("Invalid escape".into()),
                Ok(_) => Vec::new(),
                Err(name) => {
                    let found: Vec<usize> = self.groups.iter().filter(|(_, n)| n.as_deref() == Some(&*name)).map(|(g, _)| *g).collect();
                    if found.is_empty() {
                        return Err("Invalid named capture referenced".into());
                    }
                    found
                }
            };
            self.nodes[node].kind = Nk::Backref(groups);
        }
        let mut names: Vec<&str> = Vec::new();
        for (_, n) in &self.groups {
            if let Some(n) = n {
                if names.contains(&n.as_str()) {
                    return Err("Duplicate capture group name".into());
                }
                names.push(n);
            }
        }
        Ok(Tree { chars: self.c, nodes: self.nodes, class_chars: self.class_chars, space_run: self.space_run })
    }

    fn disjunction(&mut self, parent: usize) -> R<()> {
        loop {
            let alt = self.push(Nk::Alt, Some(parent), self.i);
            self.alternative(alt)?;
            self.nodes[alt].end = self.i;
            if self.peek() == Some('|') {
                self.i += 1;
            } else {
                return Ok(());
            }
        }
    }

    fn alternative(&mut self, parent: usize) -> R<()> {
        let mut spaces = 0;
        while let Some(ch) = self.peek() {
            if ch == '|' || ch == ')' {
                break;
            }
            let start = self.i;
            let quantifiable = self.term(parent)?;
            let quantified = self.quantifier(quantifiable)?;
            // Runs of literal spaces, the last one not quantified.
            if self.c[start..self.i] == [' '] && !quantified {
                spaces += 1;
            } else {
                if spaces >= 2 && self.space_run.is_none() {
                    self.space_run = Some(spaces);
                }
                spaces = 0;
            }
        }
        if spaces >= 2 && self.space_run.is_none() {
            self.space_run = Some(spaces);
        }
        Ok(())
    }

    /// One term; whether a quantifier may follow it.
    fn term(&mut self, parent: usize) -> R<bool> {
        let start = self.i;
        let ch = self.peek().expect("not at end");
        match ch {
            '^' | '$' => {
                self.i += 1;
                Ok(false)
            }
            '\\' => self.escape(parent),
            '[' => {
                self.class(parent)?;
                Ok(true)
            }
            '(' => {
                self.i += 1;
                let (kind, quantifiable) = if self.at("?:") {
                    self.i += 2;
                    (Nk::Group, true)
                } else if self.at("?=") || self.at("?!") {
                    let negative = self.c[self.i + 1] == '!';
                    self.i += 2;
                    (Nk::Look { behind: false, negative }, !self.unicode)
                } else if self.at("?<=") || self.at("?<!") {
                    let negative = self.c[self.i + 2] == '!';
                    self.i += 3;
                    (Nk::Look { behind: true, negative }, false)
                } else if self.at("?<") {
                    self.i += 2;
                    let name = self.group_name()?;
                    let g = self.push(Nk::Group, Some(parent), start);
                    self.groups.push((g, Some(name)));
                    return self.close_group(g);
                } else if self.peek() == Some('?') {
                    // Modifiers: `(?i:...)`, `(?-i:...)`.
                    self.i += 1;
                    while self.peek().is_some_and(|c| "ims-".contains(c)) {
                        self.i += 1;
                    }
                    if self.peek() != Some(':') {
                        return Err("Invalid group".into());
                    }
                    self.i += 1;
                    (Nk::Group, true)
                } else {
                    let g = self.push(Nk::Group, Some(parent), start);
                    self.groups.push((g, None));
                    return self.close_group(g);
                };
                let g = self.push(kind, Some(parent), start);
                self.close_group(g)?;
                Ok(quantifiable)
            }
            ')' => Err("Unmatched ')'".into()),
            '*' | '+' | '?' => Err("Nothing to repeat".into()),
            '{' => {
                if self.quantifier_braces().is_some() {
                    return Err("Nothing to repeat".into());
                }
                if self.unicode {
                    return Err("Lone quantifier brackets".into());
                }
                self.i += 1;
                Ok(true)
            }
            '}' | ']' if self.unicode => Err("Lone quantifier brackets".into()),
            _ => {
                self.i += 1;
                Ok(true)
            }
        }
    }

    fn close_group(&mut self, g: usize) -> R<bool> {
        self.disjunction(g)?;
        if self.peek() != Some(')') {
            return Err("Unterminated group".into());
        }
        self.i += 1;
        self.nodes[g].end = self.i;
        Ok(true)
    }

    fn group_name(&mut self) -> R<String> {
        let mut name = String::new();
        while let Some(c) = self.peek() {
            if c == '>' {
                self.i += 1;
                if name.is_empty() {
                    return Err("Invalid capture group name".into());
                }
                return Ok(name);
            }
            if !(c.is_alphanumeric() || c == '_' || c == '$') || name.is_empty() && c.is_ascii_digit() {
                return Err("Invalid capture group name".into());
            }
            name.push(c);
            self.i += 1;
        }
        Err("Invalid capture group name".into())
    }

    /// `{n}`, `{n,}`, `{n,m}` at the cursor: `(min, max)` and its length.
    fn quantifier_braces(&self) -> Option<(u64, Option<u64>, usize)> {
        let rest: String = self.c[self.i..].iter().take(40).collect();
        let body = rest.strip_prefix('{')?;
        let close = body.find('}')?;
        let inner = &body[..close];
        let (a, b) = match inner.split_once(',') {
            Some((a, b)) => (a, Some(b)),
            None => (inner, None),
        };
        if a.is_empty() || !a.chars().all(|c| c.is_ascii_digit()) || b.is_some_and(|b| !b.chars().all(|c| c.is_ascii_digit())) {
            return None;
        }
        let min = a.parse().unwrap_or(u64::MAX);
        let max = match b {
            None => Some(min),
            Some("") => None,
            Some(b) => Some(b.parse().unwrap_or(u64::MAX)),
        };
        Some((min, max, close + 2))
    }

    fn quantifier(&mut self, quantifiable: bool) -> R<bool> {
        let q = match self.peek() {
            Some('*' | '+' | '?') => 1,
            Some('{') => match self.quantifier_braces() {
                Some((min, max, len)) => {
                    if max.is_some_and(|m| m < min) {
                        return Err("numbers out of order in {} quantifier".into());
                    }
                    len
                }
                None => return Ok(false),
            },
            _ => return Ok(false),
        };
        if !quantifiable {
            return Err("Nothing to repeat".into());
        }
        self.i += q;
        if self.peek() == Some('?') {
            self.i += 1;
        }
        Ok(true)
    }

    /// After `\` outside a class.
    fn escape(&mut self, parent: usize) -> R<bool> {
        let start = self.i;
        self.i += 1;
        let Some(c) = self.peek() else { return Err("\\ at end of pattern".into()) };
        match c {
            'b' | 'B' => {
                self.i += 1;
                Ok(false)
            }
            '1'..='9' => {
                let mut n = 0u32;
                while let Some(d) = self.peek().and_then(|d| d.to_digit(10)) {
                    n = n.saturating_mul(10).saturating_add(d);
                    self.i += 1;
                }
                let b = self.push(Nk::Backref(Vec::new()), Some(parent), start);
                self.nodes[b].end = self.i;
                self.backrefs.push((b, Ok(n)));
                Ok(true)
            }
            'k' if self.unicode || self.named => {
                self.i += 1;
                if self.peek() != Some('<') {
                    return Err("Invalid named reference".into());
                }
                self.i += 1;
                let name = self.group_name().map_err(|_| "Invalid named reference".to_string())?;
                let b = self.push(Nk::Backref(Vec::new()), Some(parent), start);
                self.nodes[b].end = self.i;
                self.backrefs.push((b, Err(name)));
                Ok(true)
            }
            _ => {
                self.char_escape(false)?;
                Ok(true)
            }
        }
    }

    /// A character escape after `\` (the cursor on the letter); the
    /// character it stands for when it is one.
    fn char_escape(&mut self, in_class: bool) -> R<Option<char>> {
        let c = self.peek().ok_or("\\ at end of pattern")?;
        self.i += 1;
        let hex = |p: &mut Self, n: usize| -> Option<u32> {
            let s: String = p.c.get(p.i..p.i + n)?.iter().collect();
            let v = u32::from_str_radix(&s, 16).ok()?;
            p.i += n;
            Some(v)
        };
        Ok(match c {
            'd' | 'D' | 's' | 'S' | 'w' | 'W' => None,
            'p' | 'P' if self.unicode => {
                if self.peek() != Some('{') {
                    return Err("Invalid property name".into());
                }
                while self.peek().is_some_and(|c| c != '}') {
                    self.i += 1;
                }
                if self.peek().is_none() {
                    return Err("Invalid property name".into());
                }
                self.i += 1;
                None
            }
            'n' => Some('\n'),
            't' => Some('\t'),
            'r' => Some('\r'),
            'f' => Some('\u{c}'),
            'v' => Some('\u{b}'),
            '0' if !self.peek().is_some_and(|d| d.is_ascii_digit()) => Some('\0'),
            'c' => match self.peek() {
                Some(l) if l.is_ascii_alphabetic() => {
                    self.i += 1;
                    char::from_u32(l as u32 % 32)
                }
                _ if self.unicode => return Err("Invalid unicode escape".into()),
                _ => Some('\\'),
            },
            'x' => match hex(self, 2) {
                Some(v) => char::from_u32(v),
                None if self.unicode => return Err("Invalid escape".into()),
                None => Some('x'),
            },
            'u' => {
                if self.unicode && self.peek() == Some('{') {
                    let close = self.c[self.i..].iter().position(|&c| c == '}').ok_or("Invalid unicode escape")?;
                    let s: String = self.c[self.i + 1..self.i + close].iter().collect();
                    let v = u32::from_str_radix(&s, 16).ok().filter(|&v| v <= 0x10FFFF).ok_or("Invalid unicode escape")?;
                    self.i += close + 1;
                    char::from_u32(v)
                } else {
                    match hex(self, 4) {
                        Some(hi @ 0xD800..=0xDBFF) => {
                            // A surrogate pair written as two escapes.
                            if self.at("\\u") {
                                let save = self.i;
                                self.i += 2;
                                match hex(self, 4) {
                                    Some(lo @ 0xDC00..=0xDFFF) => char::from_u32(0x10000 + ((hi - 0xD800) << 10) + (lo - 0xDC00)),
                                    _ => {
                                        self.i = save;
                                        None
                                    }
                                }
                            } else {
                                None
                            }
                        }
                        Some(v) => char::from_u32(v),
                        None if self.unicode => return Err("Invalid unicode escape".into()),
                        None => Some('u'),
                    }
                }
            }
            c if self.unicode => {
                if "^$\\.*+?()[]{}|/".contains(c) || in_class && c == '-' {
                    Some(c)
                } else {
                    return Err("Invalid escape".into());
                }
            }
            c => Some(c),
        })
    }

    fn class(&mut self, parent: usize) -> R<()> {
        let start = self.i;
        self.i += 1;
        let negated = self.peek() == Some('^');
        if negated {
            self.i += 1;
        }
        let empty = !negated && self.peek() == Some(']');
        let node = self.push(Nk::Class { empty }, Some(parent), start);
        let mut chars = Vec::new();
        let mut prev: Option<char> = None;
        let mut range_from: Option<char> = None;
        loop {
            let Some(c) = self.peek() else { return Err("Unterminated character class".into()) };
            if c == ']' {
                self.i += 1;
                break;
            }
            if self.sets && c == '[' {
                self.class(node)?;
                prev = None;
                continue;
            }
            let item = if c == '\\' {
                self.i += 1;
                if self.peek() == Some('b') {
                    self.i += 1;
                    Some('\u{8}')
                } else {
                    self.char_escape(true)?
                }
            } else {
                self.i += 1;
                Some(c)
            };
            if let Some(from) = range_from.take() {
                match item {
                    Some(to) if to < from => return Err("Range out of order in character class".into()),
                    None if self.unicode => return Err("Invalid character class".into()),
                    _ => {}
                }
                if let Some(to) = item {
                    chars.push(to);
                }
                prev = None;
                continue;
            }
            if let Some(ch) = item {
                chars.push(ch);
            }
            if self.peek() == Some('-') && self.c.get(self.i + 1).is_some_and(|&n| n != ']') {
                if let Some(ch) = item {
                    self.i += 1;
                    range_from = Some(ch);
                    continue;
                } else if self.unicode {
                    return Err("Invalid character class".into());
                }
            }
            prev = item;
        }
        let _ = prev;
        self.nodes[node].end = self.i;
        self.class_chars.push((node, chars));
        Ok(())
    }
}

/// A string literal's value (`"a\nb"` -> `a`, newline, `b`), or `None`
/// for templates with substitutions.
pub fn cook(literal: &str) -> Option<String> {
    let quote = literal.chars().next()?;
    if !matches!(quote, '"' | '\'' | '`') || literal.len() < 2 {
        return None;
    }
    let body = &literal[1..literal.len() - 1];
    if quote == '`' && body.contains("${") {
        return None;
    }
    let mut out = String::new();
    let mut it = body.chars().peekable();
    while let Some(c) = it.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        let Some(e) = it.next() else { break };
        match e {
            'n' => out.push('\n'),
            't' => out.push('\t'),
            'r' => out.push('\r'),
            'b' => out.push('\u{8}'),
            'f' => out.push('\u{c}'),
            'v' => out.push('\u{b}'),
            '0' if !it.peek().is_some_and(|d| d.is_ascii_digit()) => out.push('\0'),
            '\r' => {
                if it.peek() == Some(&'\n') {
                    it.next();
                }
            }
            '\n' | '\u{2028}' | '\u{2029}' => {}
            'x' => {
                let s: String = it.by_ref().take(2).collect();
                out.push(char::from_u32(u32::from_str_radix(&s, 16).ok()?)?);
            }
            'u' => {
                let v = if it.peek() == Some(&'{') {
                    it.next();
                    let s: String = it.by_ref().take_while(|&c| c != '}').collect();
                    u32::from_str_radix(&s, 16).ok()?
                } else {
                    let s: String = it.by_ref().take(4).collect();
                    u32::from_str_radix(&s, 16).ok()?
                };
                // Lone surrogates have no `char`: keep the escape.
                match char::from_u32(v) {
                    Some(ch) => out.push(ch),
                    None => out.push_str(&format!("\\u{v:04X}")),
                }
            }
            e => out.push(e),
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn errors_name_what_is_wrong() {
        assert!(error("a(b)c", "").is_none());
        assert_eq!(error("(", "").as_deref(), Some("Invalid regular expression: /(/: Unterminated group"));
        assert_eq!(error("a)", "").as_deref(), Some("Invalid regular expression: /a)/: Unmatched ')'"));
        assert_eq!(error("*a", "").as_deref(), Some("Invalid regular expression: /*a/: Nothing to repeat"));
        assert_eq!(error("[b-a]", "u").as_deref(), Some("Invalid regular expression: /[b-a]/u: Range out of order in character class"));
        assert_eq!(error("[a", "").as_deref(), Some("Invalid regular expression: /[a/: Unterminated character class"));
        assert!(error("a{", "").is_none());
        assert!(error("a{", "u").is_some());
        assert_eq!(error("a", "gg").as_deref(), Some("Duplicate flags ('g') supplied to RegExp constructor"));
        assert_eq!(error("a", "z").as_deref(), Some("Invalid flags supplied to RegExp constructor 'z'"));
        assert!(error("\\1(a)", "").is_none());
    }

    #[test]
    fn problems_find_structure() {
        assert_eq!(problems("a[]b", "", true), vec!["empty_class"]);
        assert!(problems("a[^]b", "", true).is_empty());
        assert_eq!(problems("a   b", "", true), vec!["spaces:3"]);
        // The last space belongs to the quantifier.
        assert!(problems("a  +b", "", true).is_empty());
        assert_eq!(problems("a   +b", "", true), vec!["spaces:2"]);
        assert_eq!(problems("(a\\1)", "", true), vec!["backref:nested:\\1:(a\\1)"]);
        assert_eq!(problems("\\1(a)", "", true), vec!["backref:forward:\\1:(a)"]);
        assert_eq!(problems("(a)|\\1", "", true), vec!["backref:disjunctive:\\1:(a)"]);
        assert_eq!(problems("(?<=(a)\\1)", "", true), vec!["backref:backward:\\1:(a)"]);
        assert_eq!(problems("(?!(a))\\1", "", true), vec!["backref:intoNegativeLookaround:\\1:(a)"]);
        assert!(problems("(a)\\1", "", true).is_empty());
        assert_eq!(problems("[👍]", "", true), vec!["misleading:surrogatePairWithoutUFlag"]);
        assert!(problems("[👍]", "u", true).is_empty());
        assert_eq!(problems("[\u{1F1EF}\u{1F1F5}]", "u", true), vec!["misleading:regionalIndicatorSymbol"]);
    }

    #[test]
    fn strings_cook_escapes() {
        assert_eq!(cook(r#""a\nb\\c""#).as_deref(), Some("a\nb\\c"));
        assert_eq!(cook(r#"'A\x42'"#).as_deref(), Some("AB"));
        assert_eq!(cook("`a${b}`"), None);
    }
}
