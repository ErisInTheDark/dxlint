//! Pure-regex patterns (no matcher calls, no `*`) compile to a
//! `regex-automata` meta regex: a lazy DFA with SIMD prefilters. The
//! interpreter in [`crate::grammar`] only handles what a DFA cannot: child
//! layers, `*`, and capturing every repetition of a label.

use std::sync::OnceLock;

use regex_automata::meta::{Cache, Regex};
use regex_automata::{Anchored, Input};
use regex_syntax::hir::{Class, ClassUnicode, ClassUnicodeRange, Hir, HirKind, Look as HLook, Repetition};

use super::class::{CharClass, Named};
use super::{Look, Pat};

/// Resolves a matcher name to its pattern when that matcher is pure, so
/// references can be inlined into DFAs that only decide match/no match.
pub type Resolve<'a> = &'a dyn Fn(&str) -> Option<&'a Pat>;

const MAX_INLINE_DEPTH: usize = 32;

/// Is `pat` free of matcher references and `*`?
pub fn is_pure(pat: &Pat) -> bool {
    let mut pure = true;
    pat.walk(&mut |p| pure &= !matches!(p, Pat::Ref { .. } | Pat::Skip));
    pure
}

/// Can `pat` be decided by a DFA once pure references are inlined?
pub fn is_decidable(pat: &Pat, resolve: Resolve) -> bool {
    decidable(pat, resolve, 0)
}

fn decidable(pat: &Pat, resolve: Resolve, depth: usize) -> bool {
    if depth > MAX_INLINE_DEPTH {
        return false;
    }
    let mut ok = true;
    pat.walk(&mut |p| match p {
        Pat::Skip => ok = false,
        Pat::Ref { name, .. } => ok &= resolve(name).is_some_and(|inner| decidable(inner, resolve, depth + 1)),
        _ => {}
    });
    ok
}

/// The longest leading part of `pat` a DFA can decide, used as a cheap
/// guard before running the interpreter. `None` if it would be useless.
pub fn guard_prefix(pat: &Pat, resolve: Resolve) -> Option<Pat> {
    let items: Vec<Pat> = match pat {
        Pat::Seq(items) => items.iter().take_while(|p| is_decidable(p, resolve)).cloned().collect(),
        _ => return None,
    };
    let prefix = Pat::Seq(items);
    (!prefix.nullable()).then_some(prefix)
}

fn named(n: Named) -> &'static ClassUnicode {
    static CLASSES: OnceLock<Vec<ClassUnicode>> = OnceLock::new();
    let all = CLASSES.get_or_init(|| {
        // Exactly the `char` methods the interpreter uses.
        [r"\p{Alphabetic}", r"[\p{Alphabetic}\p{N}]", r"[\p{Alphabetic}\p{N}_]", r"\p{White_Space}", r"\p{Uppercase}", r"\p{Lowercase}"]
            .iter()
            .map(|src| match regex_syntax::parse(src).map(|h| h.into_kind()) {
                Ok(HirKind::Class(Class::Unicode(c))) => c,
                other => panic!("builtin class {src} did not parse to a class: {other:?}"),
            })
            .collect()
    });
    &all[match n {
        Named::Alpha => 0,
        Named::Alnum => 1,
        Named::Word => 2,
        Named::Space => 3,
        Named::Upper => 4,
        Named::Lower => 5,
    }]
}

fn class(c: &CharClass) -> Hir {
    let (ranges, names, negated) = c.parts();
    let mut out = ClassUnicode::new(ranges.iter().map(|&(a, b)| ClassUnicodeRange::new(a, b)));
    for n in names {
        out.union(named(*n));
    }
    if negated {
        out.negate();
    }
    Hir::class(Class::Unicode(out))
}

fn any(newline: bool) -> Hir {
    let mut c = ClassUnicode::new([ClassUnicodeRange::new('\0', char::MAX)]);
    if !newline {
        c.difference(&ClassUnicode::new([ClassUnicodeRange::new('\n', '\n')]));
    }
    Hir::class(Class::Unicode(c))
}

fn to_hir(pat: &Pat, resolve: Resolve, depth: usize) -> Result<Hir, String> {
    Ok(match pat {
        Pat::Empty => Hir::empty(),
        Pat::Lit { bytes, fold: false } => Hir::literal(bytes.clone()),
        Pat::Lit { bytes, fold: true } => Hir::concat(
            String::from_utf8_lossy(bytes)
                .chars()
                .map(|ch| {
                    let (lo, up) = (ch.to_ascii_lowercase(), ch.to_ascii_uppercase());
                    Hir::class(Class::Unicode(ClassUnicode::new([ClassUnicodeRange::new(lo, lo), ClassUnicodeRange::new(up, up)])))
                })
                .collect(),
        ),
        Pat::Class(c) => class(c),
        Pat::Any { newline } => any(*newline),
        Pat::Seq(items) => Hir::concat(items.iter().map(|p| to_hir(p, resolve, depth)).collect::<Result<_, _>>()?),
        Pat::Alt(items) => Hir::alternation(items.iter().map(|p| to_hir(p, resolve, depth)).collect::<Result<_, _>>()?),
        Pat::Repeat { inner, min, max, greedy } => {
            Hir::repetition(Repetition { min: *min, max: *max, greedy: *greedy, sub: Box::new(to_hir(inner, resolve, depth)?) })
        }
        // Captures never matter for match/no-match.
        Pat::Capture { inner, .. } => to_hir(inner, resolve, depth)?,
        Pat::Look(l) => Hir::look(match l {
            Look::Bol => HLook::StartLF,
            Look::Eol => HLook::EndCRLF,
            Look::Bof => HLook::Start,
            Look::Eof => HLook::End,
            Look::WordBoundary => HLook::WordUnicode,
            Look::NotWordBoundary => HLook::WordUnicodeNegate,
        }),
        Pat::Ref { name, .. } => {
            let inner = resolve(name).filter(|_| depth < MAX_INLINE_DEPTH).ok_or_else(|| format!("`{name}` is not a pure matcher"))?;
            to_hir(inner, resolve, depth + 1)?
        }
        Pat::Skip => return Err("`*` needs the layer engine".into()),
    })
}

pub struct Dfa {
    re: Regex,
}

impl std::fmt::Debug for Dfa {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Dfa")
    }
}

impl Dfa {
    pub fn new(pat: &Pat, resolve: Resolve) -> Result<Dfa, String> {
        Dfa::from_hir(to_hir(pat, resolve, 0)?)
    }

    /// A DFA that only matches all of the haystack (for `value ~ pattern`).
    pub fn whole(pat: &Pat, resolve: Resolve) -> Result<Dfa, String> {
        let hir = Hir::concat(vec![Hir::look(HLook::Start), to_hir(pat, resolve, 0)?, Hir::look(HLook::End)]);
        Dfa::from_hir(hir)
    }

    fn from_hir(hir: Hir) -> Result<Dfa, String> {
        Regex::builder().build_from_hir(&hir).map(|re| Dfa { re }).map_err(|e| e.to_string())
    }

    pub fn cache(&self) -> Cache {
        self.re.create_cache()
    }

    /// End of the leftmost-first match starting exactly at `pos`.
    #[inline]
    pub fn anchored(&self, cache: &mut Cache, hay: &[u8], pos: usize) -> Option<usize> {
        let input = Input::new(hay).range(pos..).anchored(Anchored::Yes);
        self.re.search_with(cache, &input).map(|m| m.end())
    }

    /// The leftmost-first match at or after `from`.
    #[inline]
    pub fn find(&self, cache: &mut Cache, hay: &[u8], from: usize) -> Option<(usize, usize)> {
        let input = Input::new(hay).range(from..);
        self.re.search_with(cache, &input).map(|m| (m.start(), m.end()))
    }

    pub fn is_match(&self, hay: &[u8]) -> bool {
        self.re.is_match(hay)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pattern::regex::parse;

    fn none(_: &str) -> Option<&'static Pat> {
        None
    }

    #[test]
    fn anchored_and_find_agree_with_regex_semantics() {
        let dfa = Dfa::new(&parse(r"[ \t]+$").unwrap(), &none).unwrap();
        let mut cache = dfa.cache();
        let hay = b"a  \r\nb \nc";
        assert_eq!(dfa.find(&mut cache, hay, 0), Some((1, 3)));
        assert_eq!(dfa.anchored(&mut cache, hay, 6), Some(7));
        assert_eq!(dfa.anchored(&mut cache, hay, 0), None);
    }

    #[test]
    fn whole_matches_need_the_entire_value() {
        let p = parse(r"a|ab").unwrap();
        let w = Dfa::whole(&p, &none).unwrap();
        assert!(w.is_match(b"ab") && w.is_match(b"a") && !w.is_match(b"abc"));
    }

    #[test]
    fn pure_refs_inline() {
        let snake = parse(r"[a-z_]+").unwrap();
        let resolve = |n: &str| (n == "snake").then_some(&snake);
        let p = Pat::Seq(vec![Pat::Ref { name: "snake".into(), label: None }, Pat::lit("!")]);
        assert!(is_decidable(&p, &resolve) && !is_pure(&p));
        assert!(Dfa::whole(&p, &resolve).unwrap().is_match(b"do_it!"));
    }
}
