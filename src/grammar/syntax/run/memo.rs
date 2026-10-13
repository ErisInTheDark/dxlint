use super::{error, Answer, Context, Fields, Run};

pub(super) type Key = (usize, u32, Context, u16);

pub(super) enum Enter {
    Return(Answer),
    Body(Option<Key>),
}

pub(super) fn enter(run: &mut Run<'_>, expression: usize, pos: u32, context: Context, power: u16) -> Enter {
    if !run.possible_start(expression, pos, context) { return Enter::Body(None); }
    if !run.program.lowered[expression].memo { return Enter::Body(None); }
    let key = (expression, pos, context, power);
    if let Some(answer) = run.scratch.memo.get(&key) {
        return Enter::Return(answer.clone().unwrap_or_else(|| Err(error(pos, "non-recursive production"))));
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
    if let Some(key) = key { run.scratch.memo.insert(key, Some(answer.clone())); }
    answer
}
