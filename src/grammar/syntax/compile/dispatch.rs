use crate::fx_hash::FxHashMap;
use super::{Op, Program};

#[derive(Default)]
struct Bucket {
    any: Vec<usize>,
    text: FxHashMap<String, Vec<usize>>,
}

/// An arm in a candidate list. `dispatched`: its FIRST set put it in this
/// list (plain current-goal tokens), so the head token already proves it can
/// start; fallback arms still need their start check.
#[derive(Clone, Copy)]
pub(crate) struct Candidate {
    pub index: usize,
    pub dispatched: bool,
}

/// Candidate arms per head token. Each distinct candidate list has an id, so
/// a dispatch that failed one arm resumes in the same list without reading
/// the head again.
pub(crate) struct Dispatch {
    buckets: Vec<(u32, FxHashMap<String, u32>)>,
    fallback: u32,
    lists: Vec<Box<[Candidate]>>,
}

impl Dispatch {
    pub fn backtracks(&self) -> bool {
        !self.lists[self.fallback as usize].is_empty() || self.lists.iter().any(|list| list.len() > 1)
    }

    pub fn candidates(&self, token: Option<u16>, text: &str) -> u32 {
        let Some((any, texts)) = token.and_then(|token| self.buckets.get(token as usize)) else { return self.fallback };
        texts.get(text).copied().unwrap_or(*any)
    }

    pub fn list(&self, id: u32) -> &[Candidate] { &self.lists[id as usize] }
}

pub(super) fn compile(program: &Program, items: &[usize]) -> Dispatch {
    let mut buckets: Vec<_> = (0..program.tokens.len()).map(|_| Bucket::default()).collect();
    let mut fallback = Vec::new();
    for (index, expression) in items.iter().enumerate() {
        let Some(first) = &program.first[*expression] else { fallback.push(index); continue; };
        if first.nullable || !first.skip_safe || first.probes.iter().any(|probe| probe.goal.is_some()
            || !matches!(program.expressions[probe.expression], Op::Token(_)))
        {
            fallback.push(index);
            continue;
        }
        for probe in &first.probes {
            let Op::Token(terminal) = &program.expressions[probe.expression] else { unreachable!() };
            for token in &terminal.tokens {
                let bucket = &mut buckets[*token as usize];
                match &terminal.text {
                    Some(text) => bucket.text.entry(text.clone()).or_default().push(index),
                    None => bucket.any.push(index),
                }
            }
        }
    }
    let candidate = |index: usize| Candidate { index, dispatched: !fallback.contains(&index) };
    let mut lists = vec![fallback.iter().map(|&index| candidate(index)).collect::<Box<[_]>>()];
    let mut add = |list: Vec<usize>| { lists.push(list.into_iter().map(candidate).collect()); (lists.len() - 1) as u32 };
    let buckets = buckets.into_iter().map(|mut bucket| {
        bucket.any.extend(&fallback);
        bucket.any.sort_unstable();
        bucket.any.dedup();
        let texts = bucket.text.into_iter().map(|(text, mut candidates)| {
            candidates.extend(&bucket.any);
            candidates.sort_unstable();
            candidates.dedup();
            (text, add(candidates))
        }).collect();
        (add(bucket.any), texts)
    }).collect();
    Dispatch { buckets, fallback: 0, lists }
}
