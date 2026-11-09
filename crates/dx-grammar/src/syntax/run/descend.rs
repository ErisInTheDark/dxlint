//! The executor for valid input: plain recursion over the instructions, each
//! answering its caller directly. It agrees with the frame executor
//! ([`super::execute`]) on every answer; that one still owns diagnosing
//! runs (which record what failing arms expected) and nesting deeper than
//! [`DEPTH`].

use super::execute::{apply_actions, inline, kind_slot, leaf, leaf_arm, look, own_input, sequence_step, Cursor};
use super::failure::resource;
use super::{memo, pratt, Answer, Fields, Input, Matched, Run};
use crate::syntax::compile::instructions::{Instruction, Span};
use crate::syntax::compile::Op;

/// Nesting the recursion takes before handing the parse to the frame
/// executor, keeping native stack use small on any thread.
const DEPTH: u32 = 1024;

/// The answer for `input` entered as a production, or `None` when it nests
/// deeper than [`DEPTH`].
pub(super) fn evaluate(run: &mut Run<'_>, input: Input) -> Option<Answer> {
    let answer = enter(run, input, true, 0);
    (!run.scratch.too_deep).then_some(answer)
}

fn enter(run: &mut Run<'_>, mut input: Input, production: bool, depth: u32) -> Answer {
    if depth >= DEPTH {
        run.scratch.too_deep = true;
        return Err(resource(input.pos));
    }
    if !production && run.program.instructions.inline[input.expression] {
        return inline(run, input);
    }
    run.scratch.counts.instructions += 1;
    let mut key = None;
    if production {
        match memo::enter(run, input.expression, input.pos, input.context, input.power) {
            memo::Enter::Return(answer) => return answer,
            memo::Enter::Body(body) => key = body,
        }
        input.scope = Fields::default();
        input.captures = Fields::default();
    }
    let program = run.program;
    let code = &program.instructions.code[input.expression];
    run.scratch.counts.by_instruction[kind_slot(&code.instruction)] += 1;
    let outer = input;
    let input = own_input(run, input);
    let depth = depth + 1;
    let answer = match leaf(run, input, code.instruction) {
        Some(answer) => answer,
        None => match code.instruction {
            Instruction::Call { root, power } => enter(run, input.child(root, input.pos, power, Fields::default()), true, depth),
            Instruction::Sequence(items) => sequence(run, input, items, depth),
            Instruction::Choice { items, table } => choice(run, input, items, table, depth),
            Instruction::Repeat { inner, min, max } => repeat(run, input, inner, min, max, depth),
            Instruction::Flag { flag, yes, no } => {
                enter(run, Input { expression: if input.context.flags & flag != 0 { yes } else { no }, ..input }, false, depth)
            }
            Instruction::Look { inner, negative } => {
                let answer = enter(run, Input { expression: inner, ..input }, false, depth);
                look(run, input.pos, negative, answer)
            }
            Instruction::Scope { capture, header, body } => scope(run, input, capture, header, body, depth),
            Instruction::Pratt { source } => pratt(run, input, source, depth),
            _ => unreachable!("leaf instructions answered above"),
        },
    };
    let answer = if code.actions.len != 0 { apply_actions(run, outer, code.actions, answer) } else { answer };
    if production { memo::finish(run, key, answer) } else { answer }
}

/// An arm or operator tried in place: one-token graphs are answered without
/// entering them.
fn attempt(run: &mut Run<'_>, input: Input, depth: u32) -> Answer {
    match leaf_arm(run, input) {
        Some(answer) => answer,
        None => enter(run, input, false, depth),
    }
}

fn sequence(run: &mut Run<'_>, input: Input, items: Span, depth: u32) -> Answer {
    let mut matched = Matched::empty(input.pos);
    let mut scope = input.scope;
    for &item in &run.program.instructions.operands[items.start..items.end()] {
        match enter(run, input.child(item, matched.end, input.power, scope), false, depth) {
            Ok(child) => sequence_step(run, &mut scope, &mut matched, child),
            Err(mut failure) => {
                failure.committed |= matched.cut;
                return Err(failure);
            }
        }
    }
    Ok(matched)
}

fn choice(run: &mut Run<'_>, input: Input, items: Span, table: usize, depth: u32) -> Answer {
    let program = run.program;
    let dispatch = program.lowered[table].choice.as_ref().expect("choice dispatch");
    let arms = &program.instructions.operands[items.start..items.end()];
    let mut cursor = Cursor::open(run, dispatch, input.context.goal, input.pos);
    loop {
        let Some(selected) = cursor.find(run, dispatch, input.pos, input.context, |index| Some(arms[index])) else {
            return Err(run.fail(input.pos, "alternative"));
        };
        match attempt(run, Input { expression: arms[selected], ..input }, depth) {
            Ok(mut matched) => {
                matched.cut = false;
                return Ok(matched);
            }
            Err(failure) if failure.committed || failure.fatal => return Err(failure),
            Err(_) => {}
        }
    }
}

fn repeat(run: &mut Run<'_>, input: Input, inner: usize, min: u32, max: Option<u32>, depth: u32) -> Answer {
    let mut matched = Matched::empty(input.pos);
    if max == Some(0) || (min == 0 && !run.possible_start(inner, input.pos, input.context)) { return Ok(matched); }
    let mut count = 0;
    loop {
        match enter(run, input.child(inner, matched.end, input.power, input.scope), false, depth) {
            Ok(child) if child.end > matched.end => {
                run.append(&mut matched, child);
                count += 1;
                if max.is_some_and(|max| count >= max) || (count >= min && !run.possible_start(inner, matched.end, input.context)) {
                    return Ok(matched);
                }
            }
            Ok(_) => return Err(run.fail(matched.end, "consuming repetition")),
            Err(failure) if failure.committed || failure.fatal || count < min => return Err(failure),
            Err(_) => return Ok(matched),
        }
    }
}

fn scope(run: &mut Run<'_>, input: Input, capture: u16, header: usize, body: usize, depth: u32) -> Answer {
    let mut header = enter(run, Input { expression: header, ..input }, false, depth)?;
    let mut captures = input.captures;
    run.scratch.fields.push(&mut captures, (capture, std::mem::take(&mut header.values)));
    header.fields = Fields::default();
    match enter(run, Input { captures, ..input.child(body, header.end, input.power, input.scope) }, false, depth) {
        Ok(body) => {
            run.append(&mut header, body);
            Ok(header)
        }
        Err(mut failure) => {
            failure.committed |= header.cut;
            Err(failure)
        }
    }
}

/// Precedence climbing: a prefix operator with its operand or else the atom,
/// then operator rows at or above the input's power, each folding the left.
fn pratt(run: &mut Run<'_>, input: Input, source: usize, depth: u32) -> Answer {
    let program = run.program;
    let Op::Pratt { atom, prefix, rows } = &program.expressions[source] else { unreachable!() };
    let lowered = &program.lowered[source];
    let dispatch = lowered.prefix.as_ref().expect("prefix dispatch");
    let mut cursor = Cursor::open(run, dispatch, input.context.goal, input.pos);
    let mut left = loop {
        let Some(index) = cursor.find(run, dispatch, input.pos, input.context, |index| Some(prefix[index].operator)) else {
            break enter(run, input.child(*atom, input.pos, 0, input.scope), false, depth)?;
        };
        match attempt(run, input.child(prefix[index].operator, input.pos, 0, input.scope), depth) {
            Ok(operator) => match enter(run, input.child(source, operator.end, prefix[index].power, Fields::default()), true, depth) {
                Ok(argument) => break pratt::prefix(run, &prefix[index], operator, argument, input.pos),
                Err(mut failure) => {
                    failure.committed |= operator.cut;
                    return Err(failure);
                }
            },
            Err(failure) if failure.committed || failure.fatal => return Err(failure),
            Err(_) => {}
        }
    };
    let dispatch = lowered.rows.as_ref().expect("operator dispatch");
    let mut cursor: Option<Cursor> = None;
    loop {
        let open = cursor.get_or_insert_with(|| Cursor::open(run, dispatch, input.context.goal, left.end));
        let Some(index) = open.find(run, dispatch, left.end, input.context, |index| (rows[index].power >= input.power).then_some(rows[index].operator)) else {
            return Ok(left);
        };
        let row = &rows[index];
        match attempt(run, input.child(row.operator, left.end, 0, input.scope), depth) {
            Ok(operator) if !row.operand => {
                left = pratt::fold(run, row, left, operator, None, input.pos);
                cursor = None;
            }
            Ok(operator) => {
                let power = if row.right_associative { row.power } else { row.power + 1 };
                match enter(run, input.child(source, operator.end, power, Fields::default()), true, depth) {
                    Ok(right) => {
                        left = pratt::fold(run, row, left, operator, Some(right), input.pos);
                        cursor = None;
                    }
                    Err(mut failure) => {
                        failure.committed |= operator.cut || left.cut;
                        return Err(failure);
                    }
                }
            }
            Err(failure) if failure.committed || failure.fatal => return Err(failure),
            Err(_) => {}
        }
    }
}
