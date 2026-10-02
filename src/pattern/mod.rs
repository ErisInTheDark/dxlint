//! Matcher patterns: a linear-time regex subset plus two layer-aware items,
//! references to other matchers and the `*` skip. Patterns compile into flat
//! programs run by [`crate::grammar`].

pub mod class;
pub mod compile;
pub mod dfa;
pub mod prefilter;
pub mod regex;

use class::CharClass;

/// Zero-width assertions.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
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

#[derive(Clone, Debug)]
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
}

impl Pat {
    pub fn lit(s: &str) -> Pat {
        Pat::Lit { bytes: s.as_bytes().to_vec(), fold: false }
    }

    /// Can this pattern match without consuming anything? Matcher
    /// references never can (that is checked per matcher).
    pub fn nullable(&self) -> bool {
        match self {
            Pat::Empty | Pat::Look(_) | Pat::Skip => true,
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
}
