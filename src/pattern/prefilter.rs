//! Cheap candidate finders. Matchers only run where their first byte (or
//! leading literal) could possibly start, which is where most speed comes from.

use memchr::memmem;

use super::{Look, Pat};

/// Set of bytes that can begin a match. Never contains UTF-8 continuation
/// bytes, so every hit is a char boundary.
#[derive(Clone, PartialEq, Eq)]
pub struct ByteSet(pub [bool; 256]);

impl std::fmt::Debug for ByteSet {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "ByteSet({} bytes)", self.count())
    }
}

impl Default for ByteSet {
    fn default() -> Self {
        ByteSet([false; 256])
    }
}

impl ByteSet {
    pub fn full() -> Self {
        let mut s = ByteSet([true; 256]);
        s.0[0x80..0xC0].fill(false);
        s
    }

    #[inline]
    pub fn contains(&self, b: u8) -> bool {
        self.0[b as usize]
    }

    pub fn insert(&mut self, b: u8) {
        if !(0x80..0xC0).contains(&b) {
            self.0[b as usize] = true;
        }
    }

    pub fn union(&mut self, other: &ByteSet) {
        for (a, b) in self.0.iter_mut().zip(other.0.iter()) {
            *a |= *b;
        }
    }

    pub fn count(&self) -> usize {
        self.0.iter().filter(|b| **b).count()
    }
}

/// Which (byte, next byte) pairs can start a match: a 64K-bit table. Far
/// more selective than first bytes alone (`b"` vs `be`, `r#` vs `re`).
pub struct PairSet(Box<[u64; 1024]>);

impl std::fmt::Debug for PairSet {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "PairSet({} pairs)", self.0.iter().map(|w| w.count_ones()).sum::<u32>())
    }
}

/// Possible starts of a match, each 0-2 byte sets long. Shorter than 2
/// means the match may end there.
type Starts = Vec<Vec<ByteSet>>;

const MAX_STARTS: usize = 64;

impl PairSet {
    /// `None` when a pair filter would not help (or the pattern can be empty).
    pub fn build(pat: &Pat, refs: &dyn Fn(&str) -> ByteSet) -> Option<PairSet> {
        let starts = starts(pat, refs)?;
        let mut table = Box::new([0u64; 1024]);
        let full = ByteSet::full();
        for s in &starts {
            let b0 = s.first()?;
            let b1 = s.get(1).unwrap_or(&full);
            for x in (0..=255u8).filter(|&x| b0.contains(x)) {
                for y in 0..=255u8 {
                    // Continuation bytes are never in sets; let them through.
                    if b1.contains(y) || (0x80..0xC0).contains(&y) {
                        let bit = (x as usize) << 8 | y as usize;
                        table[bit / 64] |= 1 << (bit % 64);
                    }
                }
            }
        }
        let allowed: u32 = table.iter().map(|w| w.count_ones()).sum();
        (allowed < 65536 / 4).then_some(PairSet(table))
    }

    /// May a match start at `pos`?
    #[inline]
    pub fn allows(&self, text: &[u8], pos: usize) -> bool {
        match text.get(pos + 1) {
            Some(&y) => {
                let bit = (text[pos] as usize) << 8 | y as usize;
                self.0[bit / 64] & (1 << (bit % 64)) != 0
            }
            None => true,
        }
    }
}

fn concat(a: Starts, b: &Starts) -> Option<Starts> {
    let mut out: Starts = Vec::new();
    for p in a {
        if p.len() >= 2 {
            out.push(p);
            continue;
        }
        for q in b {
            let mut joined = p.clone();
            joined.extend(q.iter().take(2 - p.len()).cloned());
            out.push(joined);
        }
    }
    out.dedup();
    (out.len() <= MAX_STARTS).then_some(out)
}

fn starts(pat: &Pat, refs: &dyn Fn(&str) -> ByteSet) -> Option<Starts> {
    let full = ByteSet::full;
    Some(match pat {
        Pat::Empty | Pat::Look(_) => vec![vec![]],
        Pat::Lit { bytes, fold } => {
            let set = |b: u8| {
                let mut s = ByteSet::default();
                s.insert(b);
                if *fold {
                    s.insert(b.to_ascii_lowercase());
                    s.insert(b.to_ascii_uppercase());
                }
                s
            };
            vec![bytes.iter().take(2).map(|&b| set(b)).collect()]
        }
        Pat::Class(_) | Pat::Any { .. } => {
            let mut first = ByteSet::default();
            first_set(pat, refs, &mut first);
            // A multi-byte char continues with continuation bytes, which
            // `allows` always lets through.
            vec![vec![first]]
        }
        Pat::Ref { name, .. } => vec![vec![refs(name), full()]],
        Pat::Skip => vec![vec![], vec![full()]],
        Pat::Capture { inner, .. } => starts(inner, refs)?,
        Pat::Seq(items) => {
            let mut acc: Starts = vec![vec![]];
            for item in items {
                if acc.iter().all(|s| s.len() >= 2) {
                    break;
                }
                acc = concat(acc, &starts(item, refs)?)?;
            }
            acc
        }
        Pat::Alt(items) => {
            let mut out = Vec::new();
            for item in items {
                out.extend(starts(item, refs)?);
            }
            (out.len() <= MAX_STARTS).then_some(out)?
        }
        Pat::Repeat { inner, min, max, .. } => {
            let one = starts(inner, refs)?;
            let two = concat(one.clone(), &one)?;
            let mut out = if *min == 0 { vec![vec![]] } else { Vec::new() };
            out.extend(one);
            if max.is_none_or(|m| m >= 2) {
                out.extend(two);
            }
            (out.len() <= MAX_STARTS).then_some(out)?
        }
    })
}

#[derive(Debug)]
pub enum Prefilter {
    /// Every char boundary.
    All,
    LineStart,
    Byte(u8),
    Byte2(u8, u8),
    Byte3(u8, u8, u8),
    Table(Box<ByteSet>),
    Literal(Box<memmem::Finder<'static>>),
    /// Matches end at end of text and are at most this many bytes long.
    Tail(usize),
    /// Leftmost candidate of any branch (top-level alternation).
    AnyOf(Vec<Prefilter>),
}

impl Prefilter {
    pub fn build(pat: &Pat, refs: &dyn Fn(&str) -> ByteSet) -> Prefilter {
        let pre = Prefilter::build_one(pat, refs);
        if !matches!(pre, Prefilter::All) {
            return pre;
        }
        // One prefilter per branch can still be selective when their union
        // is not (`[^\n]\z | \n\n+\z`).
        if let Pat::Alt(branches) = strip(pat) {
            let parts: Vec<Prefilter> = branches.iter().map(|b| Prefilter::build_one(b, refs)).collect();
            if parts.iter().all(|p| !matches!(p, Prefilter::All)) {
                return Prefilter::AnyOf(parts);
            }
        }
        pre
    }

    fn build_one(pat: &Pat, refs: &dyn Fn(&str) -> ByteSet) -> Prefilter {
        if ends_at_eof(pat) {
            if let Some(max) = max_len(pat) {
                return Prefilter::Tail(max);
            }
        }
        if let Some(lit) = leading_literal(pat) {
            if lit.len() >= 2 {
                return Prefilter::Literal(Box::new(memmem::Finder::new(lit).into_owned()));
            }
        }
        if starts_with_bol(pat) {
            return Prefilter::LineStart;
        }
        let mut set = ByteSet::default();
        if first_set(pat, refs, &mut set) {
            return Prefilter::All;
        }
        Prefilter::from_set(&set)
    }

    pub fn from_set(set: &ByteSet) -> Prefilter {
        let bytes: Vec<u8> = (0..=255u8).filter(|&b| set.contains(b)).collect();
        match bytes.as_slice() {
            [a] => Prefilter::Byte(*a),
            [a, b] => Prefilter::Byte2(*a, *b),
            [a, b, c] => Prefilter::Byte3(*a, *b, *c),
            _ if bytes.len() >= 160 => Prefilter::All,
            _ => Prefilter::Table(Box::new(set.clone())),
        }
    }

    /// Next candidate position at or after `from`.
    #[inline]
    pub fn next(&self, text: &[u8], from: usize) -> Option<usize> {
        if from >= text.len() {
            return None;
        }
        let hay = &text[from..];
        let rel = match self {
            Prefilter::All => Some(0),
            Prefilter::LineStart => {
                if from == 0 || text[from - 1] == b'\n' {
                    Some(0)
                } else {
                    memchr::memchr(b'\n', hay).map(|p| p + 1)
                }
            }
            Prefilter::Byte(a) => memchr::memchr(*a, hay),
            Prefilter::Byte2(a, b) => memchr::memchr2(*a, *b, hay),
            Prefilter::Byte3(a, b, c) => memchr::memchr3(*a, *b, *c, hay),
            Prefilter::Table(t) => hay.iter().position(|&b| t.contains(b)),
            Prefilter::Literal(f) => f.find(hay),
            Prefilter::Tail(max) => {
                let mut start = text.len().saturating_sub(*max).max(from);
                while start < text.len() && (0x80..0xC0).contains(&text[start]) {
                    start += 1;
                }
                Some(start - from)
            }
            Prefilter::AnyOf(parts) => return parts.iter().filter_map(|p| p.next(text, from)).min(),
        };
        rel.map(|r| from + r).filter(|&p| p < text.len())
    }
}

fn strip(pat: &Pat) -> &Pat {
    match pat {
        Pat::Capture { inner, .. } => strip(inner),
        other => other,
    }
}

/// Does every way of matching `pat` end with `\z`?
fn ends_at_eof(pat: &Pat) -> bool {
    match pat {
        Pat::Look(Look::Eof) => true,
        Pat::Capture { inner, .. } => ends_at_eof(inner),
        Pat::Seq(items) => items.iter().rev().find(|p| !matches!(p, Pat::Empty)).is_some_and(ends_at_eof),
        Pat::Alt(items) => !items.is_empty() && items.iter().all(ends_at_eof),
        _ => false,
    }
}

/// Longest possible match in bytes, if bounded.
fn max_len(pat: &Pat) -> Option<usize> {
    match pat {
        Pat::Empty | Pat::Look(_) => Some(0),
        Pat::Lit { bytes, .. } => Some(bytes.len()),
        Pat::Class(_) | Pat::Any { .. } => Some(4),
        Pat::Seq(items) => items.iter().try_fold(0usize, |acc, p| Some(acc + max_len(p)?)),
        Pat::Alt(items) => items.iter().try_fold(0usize, |acc, p| Some(acc.max(max_len(p)?))),
        Pat::Repeat { inner, max, .. } => Some(max_len(inner)? * (*max)? as usize),
        Pat::Capture { inner, .. } => max_len(inner),
        Pat::Ref { .. } | Pat::Skip => None,
    }
}

fn leading_literal(pat: &Pat) -> Option<&[u8]> {
    match pat {
        Pat::Lit { bytes, fold: false } => Some(bytes),
        Pat::Capture { inner, .. } => leading_literal(inner),
        Pat::Seq(items) => items.iter().find(|p| !matches!(p, Pat::Look(_) | Pat::Empty)).and_then(leading_literal),
        _ => None,
    }
}

fn starts_with_bol(pat: &Pat) -> bool {
    match pat {
        Pat::Look(Look::Bol) => true,
        Pat::Capture { inner, .. } => starts_with_bol(inner),
        Pat::Seq(items) => items.iter().find(|p| !matches!(p, Pat::Empty)).is_some_and(starts_with_bol),
        _ => false,
    }
}

/// Adds every byte that can start `pat` to `set`; returns whether `pat` is
/// nullable (so whatever follows can also start the match).
pub fn first_set(pat: &Pat, refs: &dyn Fn(&str) -> ByteSet, set: &mut ByteSet) -> bool {
    match pat {
        Pat::Empty | Pat::Look(_) => true,
        Pat::Lit { bytes, fold } => match bytes.first() {
            None => true,
            Some(&b) => {
                set.insert(b);
                if *fold {
                    set.insert(b.to_ascii_lowercase());
                    set.insert(b.to_ascii_uppercase());
                }
                false
            }
        },
        Pat::Class(class) => {
            for b in 0..128u8 {
                if class.contains_ascii(b) {
                    set.insert(b);
                }
            }
            if class.may_match_non_ascii() {
                for b in 0xC0..=0xFFu8 {
                    set.insert(b);
                }
            }
            false
        }
        Pat::Any { newline } => {
            let had_nl = set.contains(b'\n');
            set.union(&ByteSet::full());
            if !newline && !had_nl {
                set.0[b'\n' as usize] = false;
            }
            false
        }
        Pat::Seq(items) => items.iter().all(|p| first_set(p, refs, set)),
        Pat::Alt(items) => items.iter().fold(false, |nullable, p| first_set(p, refs, set) | nullable),
        Pat::Repeat { inner, min, .. } => first_set(inner, refs, set) || *min == 0,
        Pat::Capture { inner, .. } => first_set(inner, refs, set),
        Pat::Ref { name, .. } => {
            set.union(&refs(name));
            false
        }
        Pat::Skip => {
            set.union(&ByteSet::full());
            true
        }
    }
}
