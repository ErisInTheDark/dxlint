//! Where a file's regexes are: literals, and calls of the global `RegExp`
//! (also through aliases) with patterns known statically. Findings get
//! file offsets: exact where the pattern is written in a literal, the
//! pattern argument's otherwise.

use super::super::facts::NativeFile;
use super::super::kinds::{Field as F, Kind as K};
use super::{findings, Finding};

/// A regex as one rule site sees it.
pub struct Site {
    pub pattern: String,
    /// Static flags (`None` when they cannot be known).
    pub flags: Option<String>,
    /// The node that writes the pattern.
    pub pattern_node: u32,
    /// `regex`, `string` (a plain string literal), `template` or `computed`.
    pub pattern_kind: &'static str,
    /// Pattern character -> file byte span, when the pattern is written
    /// out in a literal.
    pub spans: Option<Vec<(usize, usize)>>,
}

/// A finding with file offsets.
pub struct Located {
    pub finding: Finding,
    pub offset: usize,
    pub end: usize,
}

impl NativeFile<'_> {
    /// A regex literal, or a `RegExp(...)` / `new RegExp(...)` call of the
    /// global with a static pattern.
    pub fn regex_site(&self, node: u32) -> Option<Site> {
        let s = &self.syntax;
        match s.kind(node)? {
            K::Regex => {
                let text = s.text(node);
                let close = text.rfind('/')?;
                let start = s.tree.node(node).start as usize + 1;
                let pattern = text[1..close].to_string();
                let spans = pattern.char_indices().map(|(i, c)| (start + i, start + i + c.len_utf8())).collect();
                Some(Site { flags: Some(text[close + 1..].to_string()), pattern, pattern_node: node, pattern_kind: "regex", spans: Some(spans) })
            }
            K::Call | K::New if self.global_value(s.get(node, F::Callee)?).as_deref() == Some("RegExp") => {
                let mut args = s.all(node, F::Arg);
                let mut arg = args.next()?;
                let flags_arg = args.next();
                while s.is(arg, K::Paren) {
                    arg = s.get(arg, F::Expr)?;
                }
                let flags = match flags_arg {
                    Some(f) => self.static_string(f),
                    None => None,
                };
                let at = s.tree.node(arg).start as usize;
                let text = s.text(arg);
                let (pattern, kind, spans) = match s.kind(arg)? {
                    K::Regex => {
                        let close = text.rfind('/')?;
                        let pattern = text[1..close].to_string();
                        let spans = pattern.char_indices().map(|(i, c)| (at + 1 + i, at + 1 + i + c.len_utf8())).collect();
                        // Without new flags, the literal is its own regex.
                        let flags = match flags_arg {
                            Some(_) => flags,
                            None => Some(text[close + 1..].to_string()),
                        };
                        return Some(Site { pattern, flags, pattern_node: arg, pattern_kind: "regex", spans: Some(spans) }).filter(|_| flags_arg.is_some());
                    }
                    K::String | K::Template if !(s.is(arg, K::Template) && s.all(arg, F::Expr).next().is_some()) => {
                        let (pattern, spans) = super::cook_spans(text)?;
                        let spans = spans.into_iter().map(|(a, b)| (at + a, at + b)).collect();
                        (pattern, if s.is(arg, K::String) { "string" } else { "template" }, Some(spans))
                    }
                    _ => (self.static_string(arg)?, "computed", None),
                };
                let flags = if flags_arg.is_none() { Some(String::new()) } else { flags };
                Some(Site { pattern, flags, pattern_node: arg, pattern_kind: kind, spans })
            }
            _ => None,
        }
    }

    /// The regex findings at a site, located in the file.
    pub fn regex_findings(&self, node: u32) -> Option<Vec<Located>> {
        let site = self.regex_site(node)?;
        let s = &self.syntax;
        // A literal passed to `RegExp` with flags is checked as the call.
        let checked_by_call = s.is(node, K::Regex)
            && s.parent(node).is_some_and(|p| {
                matches!(s.kind(p), Some(K::Call | K::New)) && s.all(p, F::Arg).next() == Some(node) && s.all(p, F::Arg).nth(1).is_some() && self.regex_site(p).is_some()
            });
        let src = s.src;
        let chars: Vec<char> = site.pattern.chars().collect();
        let raw = |a: usize, b: usize| -> String {
            match &site.spans {
                Some(spans) if a < spans.len() && b > a => src[spans[a].0..spans[b - 1].1.max(spans[a].0)].to_string(),
                _ => chars[a.min(chars.len())..b.min(chars.len())].iter().collect(),
            }
        };
        let flags = site.flags.clone().unwrap_or_default();
        let whole = s.tree.node(site.pattern_node);
        let mut out: Vec<Located> = findings(&site.pattern, &flags, site.pattern_kind == "regex" && s.is(node, K::Regex), &raw)
            .into_iter()
            .filter(|f| !(checked_by_call && f.check.starts_with("misleading")))
            .map(|f| {
                let (offset, end) = match &site.spans {
                    Some(spans) if f.start < spans.len() && f.end > f.start => (spans[f.start].0, spans[(f.end - 1).min(spans.len() - 1)].1),
                    _ => (whole.start as usize, whole.end as usize),
                };
                Located { finding: f, offset, end }
            })
            .collect();
        out.sort_by_key(|l| l.finding.check);
        Some(out)
    }

    /// no-invalid-regexp on a `RegExp` call: the pattern's syntax error
    /// (only plain string patterns; with unknown flags, only when it fails
    /// in every mode).
    pub fn regex_pattern_error(&self, node: u32) -> Option<String> {
        let s = &self.syntax;
        if !matches!(s.kind(node)?, K::Call | K::New) || self.global_value(s.get(node, F::Callee)?).as_deref() != Some("RegExp") {
            return None;
        }
        let arg = s.all(node, F::Arg).next().filter(|&a| s.is(a, K::String))?;
        let pattern = super::cook(s.text(arg))?;
        match self.regex_flags(node) {
            Some(flags) => super::pattern_error(&pattern, flags.contains('u'), flags.contains('v')),
            None => {
                super::pattern_error(&pattern, true, false)?;
                super::pattern_error(&pattern, false, true)?;
                super::pattern_error(&pattern, false, false)
            }
        }
    }

    /// The flags of a `RegExp` call as no-invalid-regexp reads them: `""`
    /// without, the string literal's value, `None` otherwise.
    pub fn regex_flags(&self, node: u32) -> Option<String> {
        let s = &self.syntax;
        match s.all(node, F::Arg).nth(1) {
            None => Some(String::new()),
            Some(f) if s.is(f, K::String) => super::cook(s.text(f)),
            Some(_) => None,
        }
    }
}
