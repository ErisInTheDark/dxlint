use regex_automata::dfa::{dense, Automaton, StartKind};
use regex_automata::nfa::thompson;
use regex_automata::util::primitives::StateID;
use regex_automata::{Anchored, Input, MatchKind};

use super::{to_hir, Pat};

pub struct Lexer {
    dfa: dense::DFA<Vec<u32>>,
    longest: Vec<bool>,
}

#[cfg(test)]
mod tests {
    use super::Lexer;
    use crate::pattern::dfa::Dfa;
    use crate::pattern::regex::parse;

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
        Some(Self { dfa, longest })
    }

    pub fn cache(&self) -> LexerCache {
        LexerCache { ends: vec![0; self.longest.len()], candidates: Vec::new() }
    }

    pub fn anchored(&self, cache: &mut LexerCache, hay: &[u8], pos: usize,
        mut verify: impl FnMut(usize) -> Option<usize>) -> Result<Option<(usize, usize)>, ()>
    {
        cache.ends.fill(0);
        cache.candidates.clear();
        let input = Input::new(hay).range(pos..).anchored(Anchored::Yes);
        let mut state = self.dfa.start_state_forward(&input).map_err(|_| ())?;
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

fn longest(pattern: &Pat) -> bool {
    if fixed(pattern) { return true; }
    match pattern {
        Pat::Seq(items) => items.iter().filter(|item| !fixed(item)).count() == 1 && items.iter().all(longest),
        Pat::Capture { inner, .. } => longest(inner),
        Pat::Repeat { inner, greedy: true, .. } => fixed(inner),
        _ => false,
    }
}
