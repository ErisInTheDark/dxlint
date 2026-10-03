use super::source::Span;

/// A problem found while loading a configuration: lexing, parsing, macro
/// expansion, imports, or compiling rules. Loading collects as many of these
/// as it can instead of stopping at the first one.
#[derive(Debug, Clone)]
pub struct ConfigError {
    pub span: Option<Span>,
    pub message: String,
    pub notes: Vec<String>,
    /// Other places the error is about ("first defined here", "still used
    /// here"), possibly in other files.
    pub related: Vec<(Span, String)>,
}

impl ConfigError {
    pub fn new(span: impl Into<Option<Span>>, message: impl Into<String>) -> Self {
        ConfigError { span: span.into(), message: message.into(), notes: Vec::new(), related: Vec::new() }
    }

    pub fn note(mut self, note: impl Into<String>) -> Self {
        self.notes.push(note.into());
        self
    }

    pub fn related(mut self, span: impl Into<Option<Span>>, label: impl Into<String>) -> Self {
        if let Some(span) = span.into() {
            self.related.push((span, label.into()));
        }
        self
    }

    /// Adds a "did you mean" note when a close candidate exists.
    pub fn suggest<'a>(self, wanted: &str, candidates: impl IntoIterator<Item = &'a str>) -> Self {
        match closest(wanted, candidates) {
            Some(best) => self.note(format!("did you mean `{best}`?")),
            None => self,
        }
    }
}

pub fn closest<'a>(wanted: &str, candidates: impl IntoIterator<Item = &'a str>) -> Option<&'a str> {
    let limit = (wanted.chars().count() / 3).max(1);
    candidates
        .into_iter()
        .map(|c| (edit_distance(wanted, c), c))
        .filter(|(d, _)| *d <= limit)
        .min_by_key(|(d, _)| *d)
        .map(|(_, c)| c)
}

fn edit_distance(a: &str, b: &str) -> usize {
    let b: Vec<char> = b.chars().collect();
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.chars().enumerate() {
        let mut cur = vec![i + 1; b.len() + 1];
        for (j, cb) in b.iter().enumerate() {
            let sub = prev[j] + usize::from(ca != *cb);
            cur[j + 1] = sub.min(prev[j + 1] + 1).min(cur[j] + 1);
        }
        prev = cur;
    }
    prev[b.len()]
}
