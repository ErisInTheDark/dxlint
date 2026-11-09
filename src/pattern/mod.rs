//! Matcher patterns: a linear-time regex subset plus two layer-aware items,
//! references to other matchers and the `*` skip. Patterns compile into flat
//! programs run by [`crate::grammar`].

pub mod class;
pub mod compile;
pub mod dfa;
pub mod prefilter;
pub mod regex;
pub mod syntax;

use class::CharClass;

/// Zero-width assertions.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Look {
    /// `^`: start of a line.
    Bol,
    /// `$`: end of a line, before `\r\n` / `\n`.
    Eol,
    /// `\A`: start of the text.
    Bof,
    /// `\z`: end of the text.
    Eof,
    WordBoundary,
    NotWordBoundary,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Pat {
    Empty,
    Lit { bytes: Vec<u8>, fold: bool },
    Class(CharClass),
    Any { newline: bool },
    Seq(Vec<Pat>),
    Alt(Vec<Pat>),
    Repeat { inner: Box<Pat>, min: u32, max: Option<u32>, greedy: bool },
    /// A labelled piece of text (`name:/\w+/`, `(?<name>...)`).
    Capture { name: String, inner: Box<Pat> },
    Look(Look),
    /// Another matcher, matched as one unit and kept as a child layer.
    Ref { name: String, label: Option<String> },
    /// `*`: lazily skip ahead, letting the current layer's children eat
    /// whole chunks (strings, nested brackets, ...) as they go.
    Skip,
    /// `(?=...)`, `(?!...)`, `(?<=...)`, `(?<!...)`: does a pure regex match
    /// right after (or right before) this position? Runs as a lazy DFA.
    Around { behind: bool, negate: bool, inner: Box<Pat> },
}

impl Pat {
    pub fn lit(s: &str) -> Pat {
        Pat::Lit { bytes: s.as_bytes().to_vec(), fold: false }
    }

    /// Can this pattern match without consuming anything? Matcher
    /// references never can (that is checked per matcher).
    pub fn nullable(&self) -> bool {
        match self {
            Pat::Empty | Pat::Look(_) | Pat::Skip | Pat::Around { .. } => true,
            Pat::Lit { bytes, .. } => bytes.is_empty(),
            Pat::Class(_) | Pat::Any { .. } | Pat::Ref { .. } => false,
            Pat::Seq(items) => items.iter().all(Pat::nullable),
            Pat::Alt(items) => items.iter().any(Pat::nullable),
            Pat::Repeat { inner, min, .. } => *min == 0 || inner.nullable(),
            Pat::Capture { inner, .. } => inner.nullable(),
        }
    }

    /// Visits every sub-pattern, depth first.
    pub fn walk<'a>(&'a self, f: &mut impl FnMut(&'a Pat)) {
        f(self);
        match self {
            Pat::Seq(items) | Pat::Alt(items) => items.iter().for_each(|p| p.walk(f)),
            Pat::Repeat { inner, .. } | Pat::Capture { inner, .. } => inner.walk(f),
            _ => {}
        }
    }

    /// Field labels this pattern produces, in order of first appearance.
    pub fn labels(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        self.walk(&mut |p| {
            let label = match p {
                Pat::Capture { name, .. } => Some(name),
                Pat::Ref { label: Some(l), .. } => Some(l),
                _ => None,
            };
            if let Some(l) = label {
                if !out.contains(l) {
                    out.push(l.clone());
                }
            }
        });
        out
    }

    /// Which matcher does label `label` refer to, if it labels a reference?
    pub fn label_target(&self, label: &str) -> Option<&str> {
        let mut found = None;
        self.walk(&mut |p| {
            if let Pat::Ref { name, label: Some(l) } = p {
                if l == label && found.is_none() {
                    found = Some(name.as_str());
                }
            }
        });
        found
    }

    /// Every text this pattern matches whole, when that is a finite set of
    /// at most `limit` case-sensitive literals.
    pub fn literals(&self, limit: usize) -> Option<Vec<Vec<u8>>> {
        let mut out = match self {
            Pat::Empty => vec![Vec::new()],
            Pat::Lit { bytes, fold: false } => vec![bytes.clone()],
            Pat::Class(class) => {
                let parts = class.parts();
                if parts.negated || !parts.named.is_empty() || !parts.not_named.is_empty() { return None; }
                let count: usize = parts.ranges.iter().map(|(low, high)| (*high as usize).saturating_sub(*low as usize) + 1).sum();
                if count > limit { return None; }
                parts.ranges.iter().flat_map(|(low, high)| *low..=*high).map(|c| c.to_string().into_bytes()).collect()
            }
            Pat::Capture { inner, .. } => inner.literals(limit)?,
            Pat::Repeat { inner, min: 1, max: Some(1), .. } => inner.literals(limit)?,
            Pat::Repeat { inner, min: 0, max: Some(1), .. } => {
                let mut out = inner.literals(limit)?;
                out.push(Vec::new());
                out
            }
            Pat::Alt(items) => {
                let mut out = Vec::new();
                for item in items {
                    out.extend(item.literals(limit)?);
                    if out.len() > limit { return None; }
                }
                out
            }
            Pat::Seq(items) => {
                let mut out = vec![Vec::new()];
                for item in items {
                    let next = item.literals(limit)?;
                    if out.len() * next.len() > limit { return None; }
                    out = out.iter().flat_map(|left| next.iter().map(move |right| [left.as_slice(), right].concat())).collect();
                }
                out
            }
            _ => return None,
        };
        out.sort_unstable();
        out.dedup();
        (out.len() <= limit).then_some(out)
    }
}
