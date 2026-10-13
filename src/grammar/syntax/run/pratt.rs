use super::{Fields, Matched, Run, Values};
use crate::grammar::syntax::compile::{Prefix, Row};

pub(super) fn prefix(run: &mut Run<'_>, prefix: &Prefix, operator: Matched, argument: Matched, pos: u32) -> Matched {
    let mut fields = operator.fields;
    if let Some(field) = prefix.operator_field { run.scratch.fields.push(&mut fields, (field, operator.values)); }
    run.scratch.fields.push(&mut fields, (prefix.argument, argument.values));
    let start = operator.start.unwrap_or(pos);
    let end = argument.end;
    let receipt = run.join([operator.receipt, argument.receipt]);
    let value = run.draft(Some(prefix.kind), start, end, fields);
    Matched {
        start: Some(start), end, values: run.scratch.values.single(value), fields: Fields::default(), receipt,
        cut: operator.cut || argument.cut,
    }
}

pub(super) fn fold(run: &mut Run<'_>, row: &Row, left: Matched, operator: Matched, right: Option<Matched>, pos: u32) -> Matched {
    let mut fields = Fields::default();
    let left_values = operands(run, row, left.values);
    run.scratch.fields.push(&mut fields, (row.left, left_values));
    run.scratch.fields.append(&mut fields, operator.fields);
    if let Some(field) = row.operator_field { run.scratch.fields.push(&mut fields, (field, operator.values)); }
    if let (Some(field), Some(right)) = (row.right, &right) {
        let values = operands(run, row, right.values);
        run.scratch.fields.push(&mut fields, (field, values));
    }
    let start = left.start.unwrap_or(pos);
    let end = right.as_ref().map_or(operator.end, |right| right.end);
    let receipt = run.join([left.receipt, operator.receipt, right.as_ref().and_then(|right| right.receipt)]);
    let value = run.draft(Some(row.kind), start, end, fields);
    Matched {
        start: Some(start), end, values: run.scratch.values.single(value), fields: Fields::default(), receipt,
        cut: left.cut || operator.cut || right.is_some_and(|right| right.cut),
    }
}

fn operands(run: &mut Run<'_>, row: &Row, values: Values) -> Values {
    if !row.flatten { return values; }
    run.scratch.flattened.clear();
    for id in run.scratch.values.reversed(values) {
        let draft = &run.scratch.drafts[id as usize];
        if draft.kind == Some(row.kind) {
            for (field, values) in run.scratch.fields.reversed(draft.fields) {
                if field == row.left { run.scratch.flattened.extend(run.scratch.values.reversed(values)); }
            }
        } else {
            run.scratch.flattened.push(id);
        }
    }
    let mut flattened = Values::default();
    for index in (0..run.scratch.flattened.len()).rev() {
        run.scratch.values.push(&mut flattened, run.scratch.flattened[index]);
    }
    flattened
}
