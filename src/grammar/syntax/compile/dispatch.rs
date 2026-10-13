use crate::fx_hash::FxHashMap;
use super::{Op, Program};

#[derive(Default)]
struct Bucket {
    any: Vec<usize>,
    text: FxHashMap<String, Vec<usize>>,
}

pub(crate) struct Dispatch {
    buckets: Vec<Bucket>,
    fallback: Vec<usize>,
}

impl Dispatch {
    pub fn backtracks(&self) -> bool {
        !self.fallback.is_empty() || self.buckets.iter().any(|bucket|
            bucket.any.len() > 1 || bucket.text.values().any(|items| items.len() > 1))
    }

    pub fn candidates(&self, token: Option<u16>, text: &str) -> &[usize] {
        let Some(bucket) = token.and_then(|token| self.buckets.get(token as usize)) else { return &self.fallback };
        bucket.text.get(text).map_or(&bucket.any, Vec::as_slice)
    }
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
    for bucket in &mut buckets {
        bucket.any.extend(&fallback);
        bucket.any.sort_unstable();
        bucket.any.dedup();
        for candidates in bucket.text.values_mut() {
            candidates.extend(&bucket.any);
            candidates.sort_unstable();
            candidates.dedup();
        }
    }
    Dispatch { buckets, fallback }
}
