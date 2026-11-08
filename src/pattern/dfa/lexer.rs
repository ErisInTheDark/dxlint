use regex_automata::dfa::{dense, Automaton, StartKind};
use regex_automata::nfa::thompson;
use regex_automata::util::primitives::StateID;
use regex_automata::{Anchored, Input, MatchKind};

use super::{to_hir, Pat};
use crate::pattern::prefilter::{first_set, ByteSet};

pub struct Lexer {
    dfa: dense::DFA<Vec<u32>>,
    longest: Vec<bool>,
    /// The anchored start state when it does not depend on the text before.
    start: Option<StateID>,
}

#[cfg(test)]
mod tests {
    use super::Lexer;
    use crate::pattern::dfa::Dfa;
    use crate::pattern::regex::parse;

    /// Patterns trusted to match longest (no verifying rerun) must agree with
    /// their leftmost-first DFA; the rest must not be trusted.
    #[test]
    fn deterministic_patterns_match_longest() {
        let cases = [("(?:a|!b)(?:a|b|!a)*", true), ("b(?:a|!(?:aa|b))*", true), ("(?:a|!)(?:!a|a)*b?", true),
            ("a(?:ab|a)*", false), ("(?:a|ab)b*", false)];
        let mut texts = vec![String::new()];
        for _ in 0..5 {
            let previous = texts.clone();
            for text in previous {
                for next in ["a", "b", "!"] { texts.push(format!("{text}{next}")); }
            }
        }
        for (source, trusted) in cases {
            let pattern = parse(source).unwrap();
            assert_eq!(super::longest(&pattern), trusted, "{source}");
            if !trusted { continue; }
            let lexer = Lexer::new(&[&pattern]).unwrap();
            let mut cache = lexer.cache();
            let dfa = Dfa::new(&pattern, &|_| None).unwrap();
            let mut dfa_cache = dfa.cache();
            for text in &texts {
                let expected = dfa.anchored(&mut dfa_cache, text.as_bytes(), 0).filter(|end| *end > 0).map(|end| (0, end));
                let actual = lexer.anchored(&mut cache, text.as_bytes(), 0, |_| unreachable!("trusted patterns skip verifying")).unwrap();
                assert_eq!(actual, expected, "{source} on {text:?}");
            }
        }
    }

    #[test]
    fn shared_scan_preserves_pattern_priority_longest_token_and_id_ties() {
        let patterns: Vec<_> = ["a+(ab)?", "a|ab", "ab*?", "a+", "[ab]+", "!", "α+"]
            .iter().map(|pattern| parse(pattern).unwrap()).collect();
        let lexer = Lexer::new(&patterns.iter().collect::<Vec<_>>()).unwrap();
        let mut cache = lexer.cache();
        let individual: Vec<_> = patterns.iter().map(|pattern| Dfa::new(pattern, &|_| None).unwrap()).collect();
        let mut caches: Vec<_> = individual.iter().map(Dfa::cache).collect();
        let mut texts = vec![String::new()];
        for _ in 0..4 {
            let previous = texts.clone();
            for text in previous {
                for next in ["a", "b", "!", " ", "\n", "α"] { texts.push(format!("{text}{next}")); }
            }
        }
        for text in texts {
            let source = format!(" {text}");
            let hay = source.as_bytes();
            let expected = individual.iter().zip(&mut caches).enumerate()
                .filter_map(|(id, (dfa, cache))| dfa.anchored(cache, hay, 1).map(|end| (id, end)))
                .max_by(|(a, x), (b, y)| x.cmp(y).then(b.cmp(a)));
            let actual = lexer.anchored(&mut cache, hay, 1, |id| individual[id].anchored(&mut caches[id], hay, 1)).unwrap();
            assert_eq!(actual, expected, "{text:?}");
        }
    }
}

pub struct LexerCache {
    ends: Vec<usize>,
    candidates: Vec<usize>,
}

impl Lexer {
    pub fn new(patterns: &[&Pat]) -> Option<Self> {
        if patterns.is_empty() { return None; }
        let hirs = patterns.iter().map(|pattern| to_hir(pattern, &|_| None, 0))
            .collect::<Result<Vec<_>, _>>().ok()?;
        let nfa = thompson::Compiler::new()
            .configure(thompson::Config::new().which_captures(thompson::WhichCaptures::None))
            .build_many_from_hir(&hirs).ok()?;
        const DFA_BYTES: usize = 16 * 1024 * 1024;
        const BUILD_BYTES: usize = 64 * 1024 * 1024;
        let dfa = dense::Builder::new().configure(dense::Config::new()
            .match_kind(MatchKind::All).start_kind(StartKind::Anchored)
            .dfa_size_limit(Some(DFA_BYTES)).determinize_size_limit(Some(BUILD_BYTES)))
            .build_from_nfa(&nfa).ok()?;
        let longest = patterns.iter().map(|pattern| longest(pattern)).collect();
        let start = dfa.universal_start_state(Anchored::Yes);
        Some(Self { dfa, longest, start })
    }

    pub fn cache(&self) -> LexerCache {
        LexerCache { ends: vec![0; self.longest.len()], candidates: Vec::new() }
    }

    fn start(&self, hay: &[u8], pos: usize) -> Result<StateID, ()> {
        match self.start {
            Some(state) => Ok(state),
            None => self.dfa.start_state_forward(&Input::new(hay).range(pos..).anchored(Anchored::Yes)).map_err(|_| ()),
        }
    }

    /// The winning pattern and its end at `pos`: the longest token, lowest
    /// pattern id on ties, where `verify` gives the leftmost-first end of a
    /// pattern whose DFA end may not be its own.
    pub fn anchored(&self, cache: &mut LexerCache, hay: &[u8], pos: usize,
        mut verify: impl FnMut(usize) -> Option<usize>) -> Result<Option<(usize, usize)>, ()>
    {
        // Usually the lowest pattern matching at the farthest match wins
        // outright: only that last match state matters. A pattern whose DFA
        // end may not be its own wins there when its own end is that far.
        let mut state = self.start(hay, pos)?;
        let mut last = None;
        let mut ended = false;
        for (offset, byte) in hay[pos..].iter().enumerate() {
            state = self.dfa.next_state(state, *byte);
            if self.dfa.is_special_state(state) {
                if self.dfa.is_quit_state(state) { return Err(()); }
                if self.dfa.is_match_state(state) { last = Some((state, pos + offset)); }
                if self.dfa.is_dead_state(state) { ended = true; break; }
            }
        }
        if !ended {
            state = self.dfa.next_eoi_state(state);
            if self.dfa.is_quit_state(state) { return Err(()); }
            if self.dfa.is_match_state(state) { last = Some((state, hay.len())); }
        }
        let Some((state, end)) = last else { return Ok(None) };
        let pattern = (0..self.dfa.match_len(state)).map(|index| self.dfa.match_pattern(state, index).as_usize()).min();
        match pattern {
            Some(pattern) if end > pos && (self.longest[pattern] || verify(pattern) == Some(end)) => Ok(Some((pattern, end))),
            _ => self.anchored_all(cache, hay, pos, verify),
        }
    }

    /// [`Self::anchored`] weighing every pattern's own end.
    fn anchored_all(&self, cache: &mut LexerCache, hay: &[u8], pos: usize,
        mut verify: impl FnMut(usize) -> Option<usize>) -> Result<Option<(usize, usize)>, ()>
    {
        cache.ends.fill(0);
        cache.candidates.clear();
        let mut state = self.start(hay, pos)?;
        for (offset, byte) in hay[pos..].iter().enumerate() {
            state = self.dfa.next_state(state, *byte);
            if self.dfa.is_quit_state(state) { return Err(()); }
            self.record(cache, state, pos + offset);
            if self.dfa.is_dead_state(state) { return Ok(self.winner(cache, pos, &mut verify)); }
        }
        state = self.dfa.next_eoi_state(state);
        if self.dfa.is_quit_state(state) { return Err(()); }
        self.record(cache, state, hay.len());
        Ok(self.winner(cache, pos, &mut verify))
    }

    fn record(&self, cache: &mut LexerCache, state: StateID, end: usize) {
        if !self.dfa.is_match_state(state) { return; }
        for index in 0..self.dfa.match_len(state) {
            let pattern = self.dfa.match_pattern(state, index).as_usize();
            if cache.ends[pattern] == 0 { cache.candidates.push(pattern); }
            cache.ends[pattern] = end;
        }
    }

    fn winner(&self, cache: &mut LexerCache, pos: usize, verify: &mut impl FnMut(usize) -> Option<usize>) -> Option<(usize, usize)> {
        cache.candidates.sort_unstable_by(|a, b| cache.ends[*b].cmp(&cache.ends[*a]).then(a.cmp(b)));
        let mut best = None;
        for pattern in &cache.candidates {
            let upper = cache.ends[*pattern];
            if best.is_some_and(|(id, end)| upper < end || (upper == end && *pattern > id)) { break; }
            let end = if self.longest[*pattern] { Some(upper) } else { verify(*pattern) };
            if let Some(end) = end.filter(|end| *end > pos) {
                if best.is_none_or(|(id, current)| end > current || (end == current && *pattern < id)) {
                    best = Some((*pattern, end));
                }
            }
        }
        best
    }
}

fn fixed(pattern: &Pat) -> bool {
    match pattern {
        Pat::Empty | Pat::Lit { .. } | Pat::Class(_) | Pat::Any { .. } | Pat::Look(_) => true,
        Pat::Seq(items) => items.iter().all(fixed),
        Pat::Capture { inner, .. } => fixed(inner),
        Pat::Repeat { inner, min, max, .. } => *max == Some(*min) && fixed(inner),
        _ => false,
    }
}

/// Whether every match of `pattern` follows one path the next byte decides
/// (alternatives start with different bytes, a greedy repetition's body with
/// bytes nothing after it can start with), so its leftmost-first match is its
/// longest. `follow`: the bytes that can come right after it.
fn deterministic(pattern: &Pat, follow: &ByteSet) -> bool {
    let first = |pattern: &Pat| {
        let mut set = ByteSet::default();
        let nullable = first_set(pattern, &|_| ByteSet::full(), &mut set);
        (set, nullable)
    };
    let disjoint = |a: &ByteSet, b: &ByteSet| (0..=255u8).all(|byte| !(a.contains(byte) && b.contains(byte)));
    match pattern {
        Pat::Empty | Pat::Lit { .. } | Pat::Class(_) | Pat::Any { .. } => true,
        Pat::Capture { inner, .. } => deterministic(inner, follow),
        Pat::Seq(items) => {
            let mut after = follow.clone();
            for item in items.iter().rev() {
                if !deterministic(item, &after) { return false; }
                let (start, nullable) = first(item);
                if nullable { after.union(&start); } else { after = start; }
            }
            true
        }
        Pat::Alt(items) => {
            let mut seen = ByteSet::default();
            items.iter().all(|item| {
                let (start, nullable) = first(item);
                let ok = !nullable && disjoint(&seen, &start) && deterministic(item, follow);
                seen.union(&start);
                ok
            })
        }
        Pat::Repeat { inner, greedy: true, .. } => {
            let (start, nullable) = first(inner);
            let mut after = follow.clone();
            after.union(&start);
            !nullable && disjoint(&start, follow) && deterministic(inner, &after)
        }
        _ => false,
    }
}

fn longest(pattern: &Pat) -> bool {
    if fixed(pattern) || deterministic(pattern, &ByteSet::default()) { return true; }
    match pattern {
        Pat::Seq(items) => items.iter().filter(|item| !fixed(item)).count() == 1 && items.iter().all(longest),
        Pat::Capture { inner, .. } => longest(inner),
        Pat::Repeat { inner, greedy: true, .. } => fixed(inner),
        _ => false,
    }
}
