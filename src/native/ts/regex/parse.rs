//! JavaScript regular expression patterns, parsed as `@eslint-community/
//! regexpp` does: an AST with positions (in characters of the pattern) and
//! its syntax error messages.

use std::sync::OnceLock;

#[derive(Debug, Clone, PartialEq)]
pub enum Kind {
    Pattern,
    Alternative,
    /// `(?:...)`, `(?i:...)`
    Group,
    CapturingGroup(Option<String>),
    Lookaround { behind: bool, negative: bool },
    Quantifier,
    /// One character (or, without the `u` flag, one UTF-16 code unit of an
    /// astral character: those come in two nodes over the same text).
    Character(u32),
    /// `.`, `\d`, `\p{...}`
    CharacterSet,
    /// `[...]`; `expression` for `[a--b]` / `[a&&b]` (its children are the
    /// operands, not elements).
    Class { negate: bool, expression: bool },
    /// `a-z` in a class: its two characters are its children.
    ClassRange,
    /// `\q{a|bc}`
    StringDisjunction,
    /// `\1`, `\k<name>`: the groups it can refer to.
    Backreference(Vec<usize>),
    /// `^`, `$`, `\b`, `\B`
    Assertion,
}

#[derive(Debug, Clone)]
pub struct Node {
    pub kind: Kind,
    pub parent: Option<usize>,
    pub start: usize,
    pub end: usize,
}

#[derive(Debug, Default)]
pub struct Ast {
    pub chars: Vec<char>,
    pub nodes: Vec<Node>,
}

impl Ast {
    pub fn raw(&self, n: usize) -> String {
        self.chars[self.nodes[n].start..self.nodes[n].end].iter().collect()
    }

    pub fn children(&self, n: usize) -> impl Iterator<Item = usize> + '_ {
        self.nodes.iter().enumerate().filter(move |(_, c)| c.parent == Some(n)).map(|(i, _)| i)
    }

    /// `n` and its ancestors up to the pattern, nearest first.
    pub fn path(&self, mut n: usize) -> Vec<usize> {
        let mut out = vec![n];
        while let Some(p) = self.nodes[n].parent {
            out.push(p);
            n = p;
        }
        out
    }
}

/// Parses `pattern`; on a syntax error, the message and what was parsed
/// up to it.
pub fn parse(pattern: &str, unicode: bool, sets: bool) -> Result<Ast, (String, Ast)> {
    let chars: Vec<char> = pattern.chars().collect();
    let named = has_group_names(&chars);
    let mut p = Parser { c: chars, i: 0, unicode: unicode || sets, sets, named, nodes: Vec::new(), groups: Vec::new(), backrefs: Vec::new() };
    match p.pattern() {
        Ok(()) => Ok(Ast { chars: p.c, nodes: p.nodes }),
        Err(e) => Err((e, Ast { chars: p.c, nodes: p.nodes })),
    }
}

/// Without the `u` flag, `\k` is a named reference only when the pattern has
/// a named group.
fn has_group_names(c: &[char]) -> bool {
    let mut in_class = false;
    let mut i = 0;
    while i < c.len() {
        match c[i] {
            '\\' => i += 1,
            '[' => in_class = true,
            ']' => in_class = false,
            '(' if !in_class && c.get(i + 1) == Some(&'?') && c.get(i + 2) == Some(&'<') && !matches!(c.get(i + 3), Some('=' | '!')) => return true,
            _ => {}
        }
        i += 1;
    }
    false
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
}

type R<T> = Result<T, String>;

fn err<T>(m: &str) -> R<T> {
    Err(m.to_string())
}

/// A character escape's value and whether it is an identity escape (`\.`).
type Escaped = Option<u32>;

impl Parser {
    fn peek(&self) -> Option<char> {
        self.c.get(self.i).copied()
    }

    fn peek_at(&self, k: usize) -> Option<char> {
        self.c.get(self.i + k).copied()
    }

    fn at(&self, s: &str) -> bool {
        s.chars().enumerate().all(|(k, ch)| self.c.get(self.i + k) == Some(&ch))
    }

    fn eat(&mut self, s: &str) -> bool {
        if self.at(s) {
            self.i += s.chars().count();
            true
        } else {
            false
        }
    }

    fn push(&mut self, kind: Kind, parent: Option<usize>, start: usize) -> usize {
        self.nodes.push(Node { kind, parent, start, end: start });
        self.nodes.len() - 1
    }

    fn finish(&mut self, n: usize) {
        self.nodes[n].end = self.i;
    }

    fn pattern(&mut self) -> R<()> {
        let root = self.push(Kind::Pattern, None, 0);
        self.disjunction(root)?;
        if self.i < self.c.len() {
            return if self.peek() == Some(')') { err("Unmatched ')'") } else { err("Unexpected character") };
        }
        self.finish(root);
        let count = self.groups.len() as u32;
        for (node, target) in std::mem::take(&mut self.backrefs) {
            let refs: Vec<usize> = match target {
                Ok(n) if n >= 1 && n <= count => vec![self.groups[n as usize - 1].0],
                Ok(_) => return err("Invalid escape"),
                Err(name) => {
                    let found: Vec<usize> = self.groups.iter().filter(|(_, g)| g.as_deref() == Some(&*name)).map(|(g, _)| *g).collect();
                    if found.is_empty() {
                        return err("Invalid named capture referenced");
                    }
                    found
                }
            };
            self.nodes[node].kind = Kind::Backreference(refs);
        }
        // Names repeat only in different alternatives.
        for (a, (ga, na)) in self.groups.iter().enumerate() {
            let Some(na) = na else { continue };
            for (gb, nb) in &self.groups[..a] {
                if nb.as_deref() == Some(na.as_str()) && !self.in_other_alternatives(*ga, *gb) {
                    return err("Duplicate capture group name");
                }
            }
        }
        Ok(())
    }

    /// `a` and `b` are in different alternatives of some disjunction.
    fn in_other_alternatives(&self, a: usize, b: usize) -> bool {
        let path = |mut n: usize| {
            let mut out = vec![n];
            while let Some(p) = self.nodes[n].parent {
                out.push(p);
                n = p;
            }
            out
        };
        let (pa, pb) = (path(a), path(b));
        let common = pa.iter().position(|x| pb.contains(x));
        let Some(ia) = common else { return false };
        let ib = pb.iter().position(|x| *x == pa[ia]).unwrap_or(0);
        ia > 0 && ib > 0 && matches!(self.nodes[pa[ia - 1]].kind, Kind::Alternative) && matches!(self.nodes[pb[ib - 1]].kind, Kind::Alternative)
    }

    fn disjunction(&mut self, parent: usize) -> R<()> {
        loop {
            let alt = self.push(Kind::Alternative, Some(parent), self.i);
            self.alternative(alt)?;
            self.finish(alt);
            if !self.eat("|") {
                return Ok(());
            }
        }
    }

    fn alternative(&mut self, parent: usize) -> R<()> {
        while let Some(ch) = self.peek() {
            if ch == '|' || ch == ')' {
                break;
            }
            self.term(parent)?;
        }
        Ok(())
    }

    /// A term and its quantifier.
    fn term(&mut self, parent: usize) -> R<()> {
        let start = self.i;
        let before = self.nodes.len();
        let quantifiable = self.atom(parent)?;
        let q = self.quantifier_len()?;
        if q == 0 {
            return Ok(());
        }
        if !quantifiable {
            return err("Nothing to repeat");
        }
        self.i += q;
        self.eat("?");
        // The quantifier wraps what it repeats.
        let quant = self.push(Kind::Quantifier, Some(parent), start);
        for n in &mut self.nodes[before..quant] {
            if n.parent == Some(parent) {
                n.parent = Some(quant);
            }
        }
        self.finish(quant);
        Ok(())
    }

    /// One atom; whether a quantifier may follow it.
    fn atom(&mut self, parent: usize) -> R<bool> {
        let start = self.i;
        let ch = self.peek().expect("not at end");
        match ch {
            '^' | '$' => {
                self.i += 1;
                let a = self.push(Kind::Assertion, Some(parent), start);
                self.finish(a);
                Ok(false)
            }
            '\\' => self.atom_escape(parent),
            '[' => {
                self.class(parent)?;
                Ok(true)
            }
            '.' => {
                self.i += 1;
                let s = self.push(Kind::CharacterSet, Some(parent), start);
                self.finish(s);
                Ok(true)
            }
            '(' => self.group(parent),
            ')' => err("Unmatched ')'"),
            '*' | '+' | '?' => err("Nothing to repeat"),
            '{' => {
                if self.braces().is_some() {
                    return err("Nothing to repeat");
                }
                if self.unicode {
                    return err("Lone quantifier brackets");
                }
                self.literal(parent, start)
            }
            '}' | ']' if self.unicode => err("Lone quantifier brackets"),
            _ => self.literal(parent, start),
        }
    }

    /// A literal character at the cursor (two nodes for an astral one
    /// without `u`).
    fn literal(&mut self, parent: usize, start: usize) -> R<bool> {
        let ch = self.peek().expect("at a character");
        self.i += 1;
        self.characters(parent, start, ch as u32);
        Ok(true)
    }

    fn characters(&mut self, parent: usize, start: usize, value: u32) {
        if !self.unicode && value > 0xFFFF {
            let v = value - 0x10000;
            for unit in [0xD800 + (v >> 10), 0xDC00 + (v & 0x3FF)] {
                let n = self.push(Kind::Character(unit), Some(parent), start);
                self.finish(n);
            }
        } else {
            let n = self.push(Kind::Character(value), Some(parent), start);
            self.finish(n);
        }
    }

    fn group(&mut self, parent: usize) -> R<bool> {
        let start = self.i;
        self.i += 1;
        let (kind, quantifiable) = if self.eat("?:") {
            (Kind::Group, true)
        } else if self.at("?=") || self.at("?!") {
            let negative = self.peek_at(1) == Some('!');
            self.i += 2;
            (Kind::Lookaround { behind: false, negative }, !self.unicode)
        } else if self.at("?<=") || self.at("?<!") {
            let negative = self.peek_at(2) == Some('!');
            self.i += 3;
            (Kind::Lookaround { behind: true, negative }, false)
        } else if self.eat("?<") {
            let name = self.group_name()?;
            let g = self.push(Kind::CapturingGroup(Some(name.clone())), Some(parent), start);
            self.groups.push((g, Some(name)));
            self.group_body(g)?;
            return Ok(true);
        } else if self.eat("?") {
            // Modifiers: `(?i:...)`, `(?-i:...)`, `(?i-s:...)`.
            let mut seen = String::new();
            let mut minus = false;
            while let Some(c) = self.peek() {
                match c {
                    'i' | 'm' | 's' if seen.contains(c) => return Err(format!("Duplicated flag '{c}'")),
                    'i' | 'm' | 's' => seen.push(c),
                    '-' if !minus => minus = true,
                    _ => break,
                }
                self.i += 1;
            }
            if self.peek() != Some(':') {
                return err("Invalid group");
            }
            self.i += 1;
            if seen.is_empty() {
                return err("Invalid empty flags");
            }
            (Kind::Group, true)
        } else {
            let g = self.push(Kind::CapturingGroup(None), Some(parent), start);
            self.groups.push((g, None));
            self.group_body(g)?;
            return Ok(true);
        };
        let g = self.push(kind, Some(parent), start);
        self.group_body(g)?;
        Ok(quantifiable)
    }

    fn group_body(&mut self, g: usize) -> R<()> {
        self.disjunction(g)?;
        if !self.eat(")") {
            return err("Unterminated group");
        }
        self.finish(g);
        Ok(())
    }

    /// A group name up to `>`: identifier characters, also written as
    /// `\uXXXX` (pairs too) or `\u{...}`.
    fn group_name(&mut self) -> R<String> {
        let mut name = String::new();
        loop {
            let c = match self.peek() {
                None => return err("Invalid capture group name"),
                Some('>') => {
                    self.i += 1;
                    if name.is_empty() {
                        return err("Invalid capture group name");
                    }
                    return Ok(name);
                }
                Some('\\') => {
                    self.i += 1;
                    if !self.eat("u") {
                        return err("Invalid capture group name");
                    }
                    let v = self.unicode_escape_value(true).ok_or("Invalid capture group name")?;
                    char::from_u32(v).ok_or("Invalid capture group name")?
                }
                Some(c) => {
                    self.i += 1;
                    c
                }
            };
            let ok = if name.is_empty() { is_id_start(c) } else { is_id_continue(c) };
            if !ok {
                return err("Invalid capture group name");
            }
            name.push(c);
        }
    }

    /// After `\u`: `XXXX` (a surrogate pair joins with a following `\uXXXX`
    /// when `pairs`), or `{...}` (in `u` mode, or for group names).
    fn unicode_escape_value(&mut self, braces_ok: bool) -> Option<u32> {
        if self.peek() == Some('{') && (self.unicode || braces_ok) {
            let close = self.c[self.i..].iter().position(|&c| c == '}')?;
            let s: String = self.c[self.i + 1..self.i + close].iter().collect();
            let v = u32::from_str_radix(&s, 16).ok().filter(|&v| v <= 0x10FFFF && !s.is_empty())?;
            self.i += close + 1;
            return Some(v);
        }
        let hi = self.hex(4)?;
        if (0xD800..=0xDBFF).contains(&hi) && (self.unicode || braces_ok) && self.at("\\u") {
            let save = self.i;
            self.i += 2;
            match self.hex(4) {
                Some(lo @ 0xDC00..=0xDFFF) => return Some(0x10000 + ((hi - 0xD800) << 10) + (lo - 0xDC00)),
                _ => self.i = save,
            }
        }
        Some(hi)
    }

    fn hex(&mut self, n: usize) -> Option<u32> {
        let s: String = self.c.get(self.i..self.i + n)?.iter().collect();
        if s.chars().count() != n || !s.chars().all(|c| c.is_ascii_hexdigit()) {
            return None;
        }
        self.i += n;
        u32::from_str_radix(&s, 16).ok()
    }

    /// `{n}`, `{n,}`, `{n,m}` at the cursor: `(min, max)` and its length.
    fn braces(&self) -> Option<(u64, Option<u64>, usize)> {
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

    /// The length of a quantifier at the cursor (0 when there is none).
    fn quantifier_len(&self) -> R<usize> {
        Ok(match self.peek() {
            Some('*' | '+' | '?') => 1,
            Some('{') => match self.braces() {
                Some((min, max, len)) => {
                    if max.is_some_and(|m| m < min) {
                        return err("numbers out of order in {} quantifier");
                    }
                    len
                }
                None => 0,
            },
            _ => 0,
        })
    }

    /// After `\` outside a class.
    fn atom_escape(&mut self, parent: usize) -> R<bool> {
        let start = self.i;
        self.i += 1;
        let Some(c) = self.peek() else { return err("\\ at end of pattern") };
        match c {
            'b' | 'B' => {
                self.i += 1;
                let a = self.push(Kind::Assertion, Some(parent), start);
                self.finish(a);
                Ok(false)
            }
            '1'..='9' => {
                let digits_at = self.i;
                let mut n = 0u32;
                while let Some(d) = self.peek().and_then(|d| d.to_digit(10)) {
                    n = n.saturating_mul(10).saturating_add(d);
                    self.i += 1;
                }
                // Without `u`, a number past the groups is an octal escape.
                if !self.unicode && n as usize > self.count_groups() {
                    self.i = digits_at;
                    let v = self.legacy_octal();
                    self.characters(parent, start, v);
                    return Ok(true);
                }
                let b = self.push(Kind::Backreference(Vec::new()), Some(parent), start);
                self.finish(b);
                self.backrefs.push((b, Ok(n)));
                Ok(true)
            }
            'k' if self.unicode || self.named => {
                self.i += 1;
                if !self.eat("<") {
                    return err("Invalid named reference");
                }
                let name = self.group_name().map_err(|_| "Invalid named reference".to_string())?;
                let b = self.push(Kind::Backreference(Vec::new()), Some(parent), start);
                self.finish(b);
                self.backrefs.push((b, Err(name)));
                Ok(true)
            }
            _ => {
                match self.char_escape(false)? {
                    Some(v) => self.characters(parent, start, v),
                    None => {
                        let s = self.push(Kind::CharacterSet, Some(parent), start);
                        self.finish(s);
                    }
                }
                Ok(true)
            }
        }
    }

    /// Capturing groups in the whole pattern (for `\N` without `u`).
    fn count_groups(&self) -> usize {
        let c = &self.c;
        let mut n = 0;
        let mut in_class = false;
        let mut i = 0;
        while i < c.len() {
            match c[i] {
                '\\' => i += 1,
                '[' => in_class = true,
                ']' => in_class = false,
                '(' if !in_class => {
                    let named = c.get(i + 1) == Some(&'?') && c.get(i + 2) == Some(&'<') && !matches!(c.get(i + 3), Some('=' | '!'));
                    if c.get(i + 1) != Some(&'?') || named {
                        n += 1;
                    }
                }
                _ => {}
            }
            i += 1;
        }
        n
    }

    /// `\0`-`\377` without `u`: an octal character code (the cursor after `\`).
    fn legacy_octal(&mut self) -> u32 {
        let mut v = 0u32;
        let mut digits = 0;
        while let Some(d) = self.peek().and_then(|d| d.to_digit(8)) {
            if digits == 3 || v * 8 + d > 0o377 {
                break;
            }
            v = v * 8 + d;
            self.i += 1;
            digits += 1;
        }
        if digits == 0 {
            // `\8`, `\9`: the digit itself.
            let c = self.peek().map_or(0, |c| c as u32);
            self.i += 1;
            return c;
        }
        v
    }

    /// A character escape after `\` (the cursor on the letter): its value,
    /// or `None` for a character set (`\d`, `\p{...}`).
    fn char_escape(&mut self, in_class: bool) -> R<Escaped> {
        let c = self.peek().ok_or("\\ at end of pattern")?;
        self.i += 1;
        Ok(match c {
            'd' | 'D' | 's' | 'S' | 'w' | 'W' => None,
            'p' | 'P' if self.unicode => {
                self.property(c == 'P', in_class)?;
                None
            }
            'n' => Some(10),
            't' => Some(9),
            'r' => Some(13),
            'f' => Some(12),
            'v' => Some(11),
            '0' if !self.peek().is_some_and(|d| d.is_ascii_digit()) => Some(0),
            '0'..='7' if !self.unicode => {
                self.i -= 1;
                Some(self.legacy_octal())
            }
            '0'..='9' if self.unicode => return err(if in_class { "Invalid class escape" } else { "Invalid escape" }),
            'c' => match self.peek() {
                Some(l) if l.is_ascii_alphabetic() => {
                    self.i += 1;
                    Some(l as u32 % 32)
                }
                Some(l) if in_class && !self.unicode && (l.is_ascii_digit() || l == '_') => {
                    self.i += 1;
                    Some(l as u32 % 32)
                }
                _ if self.unicode => return err(if in_class { "Invalid class escape" } else { "Invalid unicode escape" }),
                // `\c` alone is a backslash and a `c`.
                _ => {
                    self.i -= 1;
                    Some('\\' as u32)
                }
            },
            'x' => match self.hex(2) {
                Some(v) => Some(v),
                None if self.unicode => return err("Invalid escape"),
                None => Some('x' as u32),
            },
            'u' => match self.unicode_escape_value(false) {
                Some(v) => Some(v),
                None if self.unicode => return err("Invalid unicode escape"),
                None => Some('u' as u32),
            },
            '-' if in_class && self.unicode => Some('-' as u32),
            c if self.unicode => {
                let syntax = "^$\\.*+?()[]{}|/";
                let reserved = self.sets && in_class && "&-!#%,:;<=>@`~".contains(c);
                if syntax.contains(c) || reserved {
                    Some(c as u32)
                } else {
                    return err(if in_class { "Invalid class escape" } else { "Invalid escape" });
                }
            }
            // `\k` without named groups: a `k`.
            c => Some(c as u32),
        })
    }

    /// After `\p` / `\P`: `{Name}`, `{Name=Value}`.
    fn property(&mut self, negated: bool, in_class: bool) -> R<()> {
        let bad = if in_class { "Invalid property name in character class" } else { "Invalid property name" };
        if !self.eat("{") {
            return err(bad);
        }
        let close = self.c[self.i..].iter().position(|&c| c == '}').ok_or(bad)?;
        let body: String = self.c[self.i..self.i + close].iter().collect();
        self.i += close + 1;
        let valid = match body.split_once('=') {
            Some((name, value)) => match name {
                "General_Category" | "gc" => general_category(value),
                "Script" | "sc" | "Script_Extensions" | "scx" => script(value),
                _ => false,
            },
            None if self.sets && STRING_PROPERTIES.contains(&body.as_str()) => {
                if negated {
                    return err(bad);
                }
                true
            }
            None => general_category(&body) || binary_property(&body),
        };
        if valid {
            Ok(())
        } else {
            err(bad)
        }
    }

    fn class(&mut self, parent: usize) -> R<()> {
        let start = self.i;
        self.i += 1;
        let negate = self.eat("^");
        let node = self.push(Kind::Class { negate, expression: false }, Some(parent), start);
        if self.sets {
            self.class_set_contents(node, negate)?;
        } else {
            self.class_ranges(node)?;
        }
        self.finish(node);
        Ok(())
    }

    /// `[...]` without `v`: characters, ranges and escapes up to `]`.
    fn class_ranges(&mut self, node: usize) -> R<()> {
        loop {
            let Some(c) = self.peek() else { return err("Unterminated character class") };
            if c == ']' {
                self.i += 1;
                return Ok(());
            }
            let first = self.class_atom(node)?;
            if self.peek() == Some('-') && self.peek_at(1).is_some_and(|n| n != ']') {
                let dash = self.i;
                self.i += 1;
                let second = self.class_atom(node)?;
                match (first, second) {
                    (Some((a, an)), Some((b, bn))) => {
                        if b < a {
                            return err("Range out of order in character class");
                        }
                        let range = self.push(Kind::ClassRange, Some(node), self.nodes[an].start);
                        self.nodes[an].parent = Some(range);
                        self.nodes[bn].parent = Some(range);
                        self.finish(range);
                    }
                    _ if self.unicode => return err("Invalid character class"),
                    // `[\d-x]`: the dash is a character.
                    _ => {
                        let d = self.push(Kind::Character('-' as u32), Some(node), dash);
                        self.nodes[d].end = dash + 1;
                    }
                }
            }
        }
    }

    /// One class atom: `Some((value, node))` for a character.
    fn class_atom(&mut self, node: usize) -> R<Option<(u32, usize)>> {
        let start = self.i;
        let c = self.peek().ok_or("Unterminated character class")?;
        let value = if c == '\\' {
            self.i += 1;
            if self.eat("b") {
                Some(8)
            } else {
                self.char_escape(true)?
            }
        } else {
            self.i += 1;
            Some(c as u32)
        };
        match value {
            Some(v) => {
                self.characters(node, start, v);
                Ok(Some((v, self.nodes.len() - 1)))
            }
            None => {
                let s = self.push(Kind::CharacterSet, Some(node), start);
                self.finish(s);
                Ok(None)
            }
        }
    }

    /// `[...]` with `v`: a union, or operands joined by `&&` / `--`. Whether
    /// it may match strings (a negated one may not).
    fn class_set_contents(&mut self, node: usize, negate: bool) -> R<bool> {
        if self.eat("]") {
            return Ok(false);
        }
        let first = self.class_set_operand(node, true)?;
        let op = if self.at("&&") {
            Some("&&")
        } else if self.at("--") {
            Some("--")
        } else {
            None
        };
        let strings = if let Some(op) = op {
            if first.range {
                return err("Invalid set operation in character class");
            }
            self.nodes[node].kind = Kind::Class { negate, expression: true };
            let mut all = first.strings;
            while self.eat(op) {
                if op == "&&" && self.at("&") {
                    return err("Invalid character in character class");
                }
                let next = self.class_set_operand(node, false)?;
                all &= next.strings;
            }
            if !self.eat("]") {
                return err(if self.at("&&") || self.at("--") { "Invalid set operation in character class" } else { "Invalid character in character class" });
            }
            // An intersection has strings only if every operand does; a
            // difference only if its first one does.
            if op == "&&" {
                all
            } else {
                first.strings
            }
        } else {
            let mut any = first.strings;
            loop {
                if self.eat("]") {
                    break;
                }
                if self.at("&&") || self.at("--") {
                    return err("Invalid set operation in character class");
                }
                any |= self.class_set_operand(node, true)?.strings;
            }
            any
        };
        if negate && strings {
            return err("Negated character class may contain strings");
        }
        Ok(strings && !negate)
    }

    /// A class set operand (or, where `ranges`, a range): a nested class,
    /// `\q{...}`, an escape or a character.
    fn class_set_operand(&mut self, node: usize, ranges: bool) -> R<Operand> {
        let start = self.i;
        let Some(c) = self.peek() else { return err("Unterminated character class") };
        if c == '[' {
            self.i += 1;
            let negate = self.eat("^");
            let nested = self.push(Kind::Class { negate, expression: false }, Some(node), start);
            let strings = self.class_set_contents(nested, negate)?;
            self.finish(nested);
            return Ok(Operand { range: false, strings });
        }
        if self.at("\\q{") {
            self.i += 3;
            let d = self.push(Kind::StringDisjunction, Some(node), start);
            let mut strings = false;
            loop {
                let mut len = 0;
                loop {
                    match self.peek() {
                        None => return err("Unterminated character class"),
                        Some('}') | Some('|') => break,
                        Some('\\') => {
                            let at = self.i;
                            self.i += 1;
                            let v = self.char_escape(true)?.ok_or("Invalid escape")?;
                            self.characters(d, at, v);
                        }
                        Some(ch) => {
                            if is_class_set_syntax(ch) {
                                return err("Invalid character in character class");
                            }
                            let at = self.i;
                            self.i += 1;
                            self.characters(d, at, ch as u32);
                        }
                    }
                    len += 1;
                }
                strings |= len != 1;
                if self.eat("}") {
                    break;
                }
                self.i += 1;
            }
            self.finish(d);
            return Ok(Operand { range: false, strings });
        }
        let value = if c == '\\' {
            self.i += 1;
            if self.eat("b") {
                Some(8)
            } else {
                // `\p{RGI_Emoji}` and friends are sets of strings.
                let strings = matches!(self.peek(), Some('p')) && {
                    let rest: String = self.c[self.i..].iter().take(40).collect();
                    STRING_PROPERTIES.iter().any(|p| rest.starts_with(&format!("p{{{p}}}")))
                };
                match self.char_escape(true)? {
                    Some(v) => Some(v),
                    None => {
                        let s = self.push(Kind::CharacterSet, Some(node), start);
                        self.finish(s);
                        return Ok(Operand { range: false, strings });
                    }
                }
            }
        } else {
            if is_class_set_syntax(c) || is_double_punctuator(&self.c[self.i..]) {
                return err("Invalid character in character class");
            }
            self.i += 1;
            Some(c as u32)
        };
        let v = value.expect("a character");
        self.characters(node, start, v);
        let first = self.nodes.len() - 1;
        if ranges && self.peek() == Some('-') && !self.at("--") {
            self.i += 1;
            let at = self.i;
            let Some(c2) = self.peek() else { return err("Unterminated character class") };
            let v2 = if c2 == '\\' {
                self.i += 1;
                self.char_escape(true)?.ok_or("Invalid character class")?
            } else {
                if is_class_set_syntax(c2) {
                    return err("Invalid character in character class");
                }
                self.i += 1;
                c2 as u32
            };
            if v2 < v {
                return err("Range out of order in character class");
            }
            self.characters(node, at, v2);
            let second = self.nodes.len() - 1;
            let range = self.push(Kind::ClassRange, Some(node), self.nodes[first].start);
            self.nodes[first].parent = Some(range);
            self.nodes[second].parent = Some(range);
            self.finish(range);
            return Ok(Operand { range: true, strings: false });
        }
        Ok(Operand { range: false, strings: false })
    }

}

struct Operand {
    range: bool,
    strings: bool,
}

fn is_class_set_syntax(c: char) -> bool {
    "()[]{}/-\\|".contains(c)
}

fn is_double_punctuator(rest: &[char]) -> bool {
    matches!(rest, [a, b, ..] if a == b && "&!#$%*+,.:;<=>?@^`~".contains(*a))
}

fn is_id_start(c: char) -> bool {
    c == '$' || c == '_' || c.is_alphabetic()
}

fn is_id_continue(c: char) -> bool {
    is_id_start(c) || c.is_alphanumeric() || c == '\u{200C}' || c == '\u{200D}' || is_mark(c)
}

/// Unicode general category M (marks).
pub fn is_mark(c: char) -> bool {
    static RE: OnceLock<regex_automata::meta::Regex> = OnceLock::new();
    let re = RE.get_or_init(|| regex_automata::meta::Regex::new(r"^[\p{Mn}\p{Mc}\p{Me}]$").expect("valid"));
    let mut buf = [0u8; 4];
    re.is_match(c.encode_utf8(&mut buf).as_bytes())
}

/// Properties of strings (`v` flag only).
const STRING_PROPERTIES: &[&str] =
    &["Basic_Emoji", "Emoji_Keycap_Sequence", "RGI_Emoji_Modifier_Sequence", "RGI_Emoji_Flag_Sequence", "RGI_Emoji_Tag_Sequence", "RGI_Emoji_ZWJ_Sequence", "RGI_Emoji"];

const GENERAL_CATEGORIES: &[&str] = &[
    "C", "Other", "Cc", "Control", "cntrl", "Cf", "Format", "Cn", "Unassigned", "Co", "Private_Use", "Cs", "Surrogate", "L", "Letter", "LC",
    "Cased_Letter", "Ll", "Lowercase_Letter", "Lm", "Modifier_Letter", "Lo", "Other_Letter", "Lt", "Titlecase_Letter", "Lu", "Uppercase_Letter",
    "M", "Mark", "Combining_Mark", "Mc", "Spacing_Mark", "Me", "Enclosing_Mark", "Mn", "Nonspacing_Mark", "N", "Number", "Nd", "Decimal_Number",
    "digit", "Nl", "Letter_Number", "No", "Other_Number", "P", "Punctuation", "punct", "Pc", "Connector_Punctuation", "Pd", "Dash_Punctuation",
    "Pe", "Close_Punctuation", "Pf", "Final_Punctuation", "Pi", "Initial_Punctuation", "Po", "Other_Punctuation", "Ps", "Open_Punctuation", "S",
    "Symbol", "Sc", "Currency_Symbol", "Sk", "Modifier_Symbol", "Sm", "Math_Symbol", "So", "Other_Symbol", "Z", "Separator", "Zl",
    "Line_Separator", "Zp", "Paragraph_Separator", "Zs", "Space_Separator",
];

const BINARY_PROPERTIES: &[&str] = &[
    "ASCII", "ASCII_Hex_Digit", "AHex", "Alphabetic", "Alpha", "Any", "Assigned", "Bidi_Control", "Bidi_C", "Bidi_Mirrored", "Bidi_M", "Case_Ignorable",
    "CI", "Cased", "Changes_When_Casefolded", "CWCF", "Changes_When_Casemapped", "CWCM", "Changes_When_Lowercased", "CWL", "Changes_When_NFKC_Casefolded",
    "CWKCF", "Changes_When_Titlecased", "CWT", "Changes_When_Uppercased", "CWU", "Dash", "Default_Ignorable_Code_Point", "DI", "Deprecated", "Dep",
    "Diacritic", "Dia", "Emoji", "Emoji_Component", "EComp", "Emoji_Modifier", "EMod", "Emoji_Modifier_Base", "EBase", "Emoji_Presentation", "EPres",
    "Extended_Pictographic", "ExtPict", "Extender", "Ext", "Grapheme_Base", "Gr_Base", "Grapheme_Extend", "Gr_Ext", "Hex_Digit", "Hex",
    "IDS_Binary_Operator", "IDSB", "IDS_Trinary_Operator", "IDST", "ID_Continue", "IDC", "ID_Start", "IDS", "Ideographic", "Ideo", "Join_Control",
    "Join_C", "Logical_Order_Exception", "LOE", "Lowercase", "Lower", "Math", "Noncharacter_Code_Point", "NChar", "Pattern_Syntax", "Pat_Syn",
    "Pattern_White_Space", "Pat_WS", "Quotation_Mark", "QMark", "Radical", "Regional_Indicator", "RI", "Sentence_Terminal", "STerm", "Soft_Dotted",
    "SD", "Terminal_Punctuation", "Term", "Unified_Ideograph", "UIdeo", "Uppercase", "Upper", "Variation_Selector", "VS", "White_Space", "space",
    "XID_Continue", "XIDC", "XID_Start", "XIDS",
];

fn general_category(v: &str) -> bool {
    GENERAL_CATEGORIES.contains(&v)
}

fn binary_property(v: &str) -> bool {
    BINARY_PROPERTIES.contains(&v)
}

/// A script name or alias, as `regex-syntax` knows them (exactly cased:
/// JavaScript does not fold names).
fn script(v: &str) -> bool {
    if v.is_empty() || !v.chars().next().is_some_and(|c| c.is_ascii_uppercase()) || !v.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        return false;
    }
    regex_syntax::ParserBuilder::new().build().parse(&format!(r"\p{{Script={v}}}")).is_ok()
}
