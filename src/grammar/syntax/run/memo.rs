use super::{Answer, Context, Fields, Run};

pub(super) type Key = (usize, u32, Context, u16);

pub(super) enum Enter {
    Return(Answer),
    Body(Option<Key>),
}

pub(super) fn enter(run: &mut Run<'_>, expression: usize, pos: u32, context: Context, power: u16) -> Enter {
    run.scratch.counts.enters += 1;
    if !run.possible_start(expression, pos, context) {
        run.scratch.counts.impossible_starts += 1;
        // Only the labels a body would record depend on running it.
        if !run.diagnose { return Enter::Return(Err(run.fail(pos, "alternative"))); }
        return Enter::Body(None);
    }
    if !run.program.lowered[expression].memo { return Enter::Body(None); }
    let key = (expression, pos, context, power);
    match run.scratch.memo.get(&key) {
        Some(Some(answer)) => {
            run.scratch.counts.memo_hits += 1;
            return Enter::Return(answer.clone());
        }
        Some(None) => return Enter::Return(Err(run.fail(pos, "non-recursive production"))),
        None => {}
    }
    run.scratch.memo.insert(key, None);
    Enter::Body(Some(key))
}

pub(super) fn finish(run: &mut Run<'_>, key: Option<Key>, answer: Answer) -> Answer {
    let answer = answer.map(|mut matched| {
        matched.fields = Fields::default();
        matched.cut = false;
        matched
    });
    if let Some(key) = key {
        run.scratch.counts.memo_stores += 1;
        run.scratch.memo.insert(key, Some(answer.clone()));
    }
    answer
}
