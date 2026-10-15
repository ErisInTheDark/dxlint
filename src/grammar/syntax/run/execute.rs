use super::{merge_error, memo, pratt, Answer, Context, Fields, Input, Matched, Run};
use super::failure::{resource, ChoiceFailures};
use crate::grammar::syntax::compile::{Action, Op, ScopeRule};
use crate::grammar::syntax::compile::instructions::{Instruction, Span};

/// Profile names of instruction kinds, in [`kind_slot`] order.
pub(super) const INSTRUCTION_KINDS: [&str; 17] = [
    "count.enter.match", "count.enter.pattern", "count.enter.call", "count.enter.sequence", "count.enter.choice",
    "count.enter.repeat", "count.enter.flag", "count.enter.look", "count.enter.probe", "count.enter.scope",
    "count.enter.reuse", "count.enter.cut", "count.enter.same_line", "count.enter.same_text", "count.enter.terminator",
    "count.enter.end", "count.enter.pratt",
];

fn kind_slot(instruction: &Instruction) -> usize {
    match instruction {
        Instruction::Match { .. } => 0,
        Instruction::Pattern { .. } => 1,
        Instruction::Call { .. } => 2,
        Instruction::Sequence(_) => 3,
        Instruction::Choice { .. } => 4,
        Instruction::Repeat { .. } => 5,
        Instruction::Flag { .. } => 6,
        Instruction::Look { .. } => 7,
        Instruction::Probe { .. } => 8,
        Instruction::Scope { .. } => 9,
        Instruction::Reuse(_) => 10,
        Instruction::Cut => 11,
        Instruction::SameLine => 12,
        Instruction::SameText { .. } => 13,
        Instruction::Terminator { .. } => 14,
        Instruction::End => 15,
        Instruction::Pratt { .. } => 16,
    }
}

#[derive(Default)]
pub(super) struct Storage {
    frames: Vec<Frame>,
}

impl Storage {
    pub(super) fn clear(&mut self) { self.frames.clear(); }
}

struct Frame {
    input: Input,
    state: State,
}

enum State {
    Actions(Span),
    Memo(Option<memo::Key>),
    Sequence { items: Span, next: usize, matched: Matched, scope: Fields },
    Choice { items: Span, table: usize, selected: usize, failures: ChoiceFailures },
    Replay { items: Span, next: usize, record: Option<usize>, failures: ChoiceFailures, failure: super::Failure },
    Repeat { inner: usize, min: u32, max: Option<u32>, count: u32, matched: Matched },
    Look(bool),
    ScopeHeader { capture: u16, body: usize },
    ScopeBody(Matched),
    PrefixProbe { source: usize, index: usize },
    PrefixRight { source: usize, index: usize, operator: Matched },
    Atom(usize),
    RowProbe { source: usize, index: usize, left: Matched },
    RowRight { source: usize, index: usize, left: Matched, operator: Matched },
}

enum Next {
    Enter(Input, bool),
    Return(Answer),
}

pub(super) fn evaluate(run: &mut Run<'_>, input: Input) -> Answer {
    let mut frames = std::mem::take(&mut run.scratch.execute.frames);
    let mut next = Next::Enter(input, true);
    let answer = loop {
        if frames.len() >= 32768 { break Err(resource(match &next {
            Next::Enter(input, _) => input.pos,
            Next::Return(Ok(matched)) => matched.end,
            Next::Return(Err(failure)) => failure.offset,
        })); }
        match next {
            Next::Enter(mut input, production) => {
                run.scratch.counts.instructions += 1;
                if production {
                    match memo::enter(run, input.expression, input.pos, input.context, input.power) {
                        memo::Enter::Return(answer) => { next = Next::Return(answer); continue; }
                        memo::Enter::Body(key) => frames.push(Frame { input, state: State::Memo(key) }),
                    }
                    input.scope = Fields::default();
                }
                let program = run.program;
                let code = &program.instructions.code[input.expression];
                run.scratch.counts.by_instruction[kind_slot(&code.instruction)] += 1;
                if code.actions.len != 0 {
                    frames.push(Frame { input, state: State::Actions(code.actions) });
                }
                input.scope = match code.scope {
                    ScopeRule::Keep => input.scope,
                    ScopeRule::Empty => Fields::default(),
                    ScopeRule::Private => run.private_scope(input.scope),
                };
                input.context = Context { goal: code.goal.unwrap_or(input.context.goal),
                    flags: (input.context.flags | code.set) & !code.clear };
                input.power = code.power.unwrap_or(input.power);
                next = match code.instruction {
                    Instruction::Match { terminal, values } => Next::Return(run.terminal(
                        &program.instructions.terminals[terminal], input.pos, input.context, values)),
                    Instruction::Pattern { predicate, emit, values } => Next::Return(run.token_pattern(
                        predicate, emit, input.pos, input.context, values)),
                    Instruction::Call { root, power } => Next::Enter(input.child(root, input.pos, power, Fields::default()), true),
                    Instruction::Sequence(items) => {
                        if items.len == 0 { Next::Return(Ok(Matched::empty(input.pos))) }
                        else {
                            frames.push(Frame { input, state: State::Sequence {
                                items, next: 1, matched: Matched::empty(input.pos), scope: input.scope,
                            } });
                            Next::Enter(input.child(program.instructions.operands[items.start], input.pos, input.power, input.scope), false)
                        }
                    }
                    Instruction::Choice { items, table } => choice(run, input, items, table, 0, ChoiceFailures::new(run), &mut frames),
                    Instruction::Repeat { inner, min, max } => {
                        if max == Some(0) || (min == 0 && !run.possible_start(inner, input.pos, input.context)) {
                            Next::Return(Ok(Matched::empty(input.pos)))
                        }
                        else {
                            frames.push(Frame { input, state: State::Repeat { inner, min, max, count: 0, matched: Matched::empty(input.pos) } });
                            Next::Enter(input.child(inner, input.pos, input.power, input.scope), false)
                        }
                    }
                    Instruction::Flag { flag, yes, no } => Next::Enter(Input {
                        expression: if input.context.flags & flag != 0 { yes } else { no }, ..input
                    }, false),
                    Instruction::Probe { expression, goal, negative } => {
                        let context = Context { goal: goal.unwrap_or(input.context.goal), ..input.context };
                        let matches = run.probe_matches(super::super::compile::Probe { expression, goal }, input.pos, input.context);
                        Next::Return(if matches != negative { Ok(Matched::empty(input.pos)) }
                            else if negative { Err(run.fail(input.pos, "negative lookahead")) }
                            else {
                                let at = run.lex(context.goal, input.pos).start;
                                Err(match &program.expressions[expression] {
                                    Op::Token(terminal) => run.fail(at, &terminal.expected),
                                    _ => run.fail(at, "matching token pattern"),
                                })
                            })
                    }
                    Instruction::Look { inner, negative } => {
                        frames.push(Frame { input, state: State::Look(negative) });
                        Next::Enter(Input { expression: inner, ..input }, false)
                    }
                    Instruction::Scope { capture, header, body } => {
                        frames.push(Frame { input, state: State::ScopeHeader { capture, body } });
                        Next::Enter(Input { expression: header, ..input }, false)
                    }
                    Instruction::Reuse(capture) => {
                        let found = run.scratch.fields.reversed(input.scope).find(|(field, _)| *field == capture);
                        match found {
                            Some((_, values)) => Next::Return(Ok(Matched { start: values.first()
                                .map(|value| run.scratch.drafts[*value as usize].start), values, ..Matched::empty(input.pos) })),
                            None => Next::Return(Err(run.fail(input.pos, "private capture"))),
                        }
                    }
                    Instruction::Cut => Next::Return(Ok(Matched { cut: true, ..Matched::empty(input.pos) })),
                    Instruction::SameLine => {
                        let token = run.lex(input.context.goal, input.pos);
                        Next::Return(if token.trivia.is_some_and(|gap| gap.line_break) { Err(run.fail(token.start, "same line")) }
                            else { Ok(Matched::empty(input.pos)) })
                    }
                    Instruction::End => {
                        let token = run.lex(input.context.goal, input.pos);
                        Next::Return(if token.start as usize == run.text.len() { Ok(Matched::empty(input.pos)) }
                            else { Err(run.fail(token.start, "end of file")) })
                    }
                    Instruction::SameText { source } => {
                        let Op::SameText(left, right, ignore) = &program.expressions[source] else { unreachable!() };
                        let left = run.field_path(input.scope, left).map(|value| (value.start, value.end));
                        let right = run.field_path(input.scope, right).map(|value| (value.start, value.end));
                        let equal = left.zip(right).is_some_and(|(left, right)| match ignore {
                            Some(predicate) => run.without_pattern(left, *predicate) == run.without_pattern(right, *predicate),
                            None => run.text[left.0 as usize..left.1 as usize] == run.text[right.0 as usize..right.1 as usize],
                        });
                        Next::Return(if equal { Ok(Matched::empty(input.pos)) } else { Err(run.fail(input.pos, "equal field text")) })
                    }
                    Instruction::Terminator { source } => {
                        let Op::Terminator { explicit, before, newline, eof } = &program.expressions[source] else { unreachable!() };
                        let answer = match run.terminal(explicit, input.pos, input.context, true) {
                            Ok(matched) => Ok(matched),
                            Err(failure) if failure.committed || failure.fatal => Err(failure),
                            Err(_) => {
                                let token = run.lex(input.context.goal, input.pos);
                                let before = before.iter().any(|terminal| token.token.as_ref().is_some_and(|token|
                                    terminal.tokens.contains(&token.token) && terminal.text.as_deref()
                                        .is_none_or(|text| text == &run.text[token.start as usize..token.end as usize])));
                                if before || (*eof && token.start as usize == run.text.len())
                                    || (*newline && token.trivia.is_some_and(|gap| gap.line_break)) {
                                    Ok(Matched::empty(input.pos))
                                } else { Err(run.fail(token.start, "statement terminator")) }
                            }
                        };
                        Next::Return(answer)
                    }
                    Instruction::Pratt { source } => prefix(run, input, source, 0, &mut frames),
                };
            }
            Next::Return(mut answer) => {
                let Some(frame) = frames.pop() else { break answer; };
                let input = frame.input;
                let program = run.program;
                next = match frame.state {
                    State::Actions(actions) => {
                        for action in &program.instructions.actions[actions.start..actions.end()] {
                            answer = match answer {
                                Ok(mut matched) => { run.apply(&mut matched, action, input.pos, input.context); Ok(matched) }
                                Err(mut failure) => {
                                    match action {
                                        Action::Try if !failure.fatal => failure.committed = false,
                                        Action::Choice if !failure.committed && !failure.fatal => failure = merge_error(run.fail(input.pos, "alternative"), failure),
                                        _ => {}
                                    }
                                    Err(failure)
                                }
                            };
                        }
                        Next::Return(answer)
                    }
                    State::Memo(key) => Next::Return(memo::finish(run, key, answer)),
                    State::Sequence { items, next, mut matched, mut scope } => match answer {
                        Ok(child) => {
                            run.scratch.fields.append(&mut scope, child.fields);
                            run.append(&mut matched, child);
                            if next == items.len { Next::Return(Ok(matched)) }
                            else {
                                let child = input.child(program.instructions.operands[items.start + next], matched.end, input.power, scope);
                                frames.push(Frame { input, state: State::Sequence { items, next: next + 1, matched, scope } });
                                Next::Enter(child, false)
                            }
                        }
                        Err(mut failure) => { failure.committed |= matched.cut; Next::Return(Err(failure)) }
                    },
                    State::Choice { items, table, selected, mut failures } => match answer {
                        Ok(mut matched) => { failures.clear(run); matched.cut = false; Next::Return(Ok(matched)) }
                        Err(failure) if failure.committed || failure.fatal => { failures.clear(run); Next::Return(Err(failure)) }
                        Err(failure) => {
                            if run.diagnose { failures.push(run, selected, failure); }
                            choice(run, input, items, table, selected + 1, failures, &mut frames)
                        }
                    },
                    State::Replay { items, next, record, failures, failure } => {
                        let child = match answer { Err(failure) => failure, Ok(_) => unreachable!("excluded FIRST arm cannot match") };
                        if child.fatal { failures.clear(run); Next::Return(Err(child)) }
                        else { replay(run, input, items, next, record, failures, merge_error(failure, child), &mut frames) }
                    }
                    State::Repeat { inner, min, max, count, mut matched } => match answer {
                        Ok(child) if child.end > matched.end => {
                            run.append(&mut matched, child);
                            let count = count + 1;
                            if max.is_some_and(|max| count >= max)
                                || (count >= min && !run.possible_start(inner, matched.end, input.context)) {
                                Next::Return(Ok(matched))
                            }
                            else {
                                let child = input.child(inner, matched.end, input.power, input.scope);
                                frames.push(Frame { input, state: State::Repeat { inner, min, max, count, matched } });
                                Next::Enter(child, false)
                            }
                        }
                        Ok(_) => Next::Return(Err(run.fail(matched.end, "consuming repetition"))),
                        Err(failure) if failure.committed || failure.fatal || count < min => Next::Return(Err(failure)),
                        Err(_) => Next::Return(Ok(matched)),
                    },
                    State::Look(negative) => Next::Return(match answer {
                        Err(failure) if failure.fatal => Err(failure),
                        Ok(_) if !negative => Ok(Matched::empty(input.pos)),
                        Err(failure) if !negative => Err(failure),
                        Err(_) => Ok(Matched::empty(input.pos)),
                        Ok(_) => Err(run.fail(input.pos, "negative lookahead")),
                    }),
                    State::ScopeHeader { capture, body } => match answer {
                        Ok(mut header) => {
                            let mut scope = input.scope;
                            run.scratch.fields.push(&mut scope, (capture, std::mem::take(&mut header.values)));
                            header.fields = Fields::default();
                            let child = input.child(body, header.end, input.power, scope);
                            frames.push(Frame { input, state: State::ScopeBody(header) });
                            Next::Enter(child, false)
                        }
                        Err(failure) => Next::Return(Err(failure)),
                    },
                    State::ScopeBody(mut header) => Next::Return(match answer {
                        Ok(body) => { run.append(&mut header, body); Ok(header) }
                        Err(mut failure) => { failure.committed |= header.cut; Err(failure) }
                    }),
                    State::PrefixProbe { source, index } => match answer {
                        Ok(operator) => {
                            let Op::Pratt { prefix, .. } = &program.expressions[source] else { unreachable!() };
                            let child = input.child(source, operator.end, prefix[index].power, Fields::default());
                            frames.push(Frame { input, state: State::PrefixRight { source, index, operator } });
                            Next::Enter(child, true)
                        }
                        Err(failure) if failure.committed || failure.fatal => Next::Return(Err(failure)),
                        Err(_) => prefix(run, input, source, index + 1, &mut frames),
                    },
                    State::PrefixRight { source, index, operator } => match answer {
                        Ok(argument) => {
                            let Op::Pratt { prefix, .. } = &program.expressions[source] else { unreachable!() };
                            let left = pratt::prefix(run, &prefix[index], operator, argument, input.pos);
                            row(run, input, source, 0, left, &mut frames)
                        }
                        Err(mut failure) => { failure.committed |= operator.cut; Next::Return(Err(failure)) }
                    },
                    State::Atom(source) => match answer {
                        Ok(left) => row(run, input, source, 0, left, &mut frames),
                        Err(failure) => Next::Return(Err(failure)),
                    },
                    State::RowProbe { source, index, left } => match answer {
                        Ok(operator) => {
                            let Op::Pratt { rows, .. } = &program.expressions[source] else { unreachable!() };
                            let spec = &rows[index];
                            if spec.operand {
                                let power = if spec.right_associative { spec.power } else { spec.power + 1 };
                                let child = input.child(source, operator.end, power, Fields::default());
                                frames.push(Frame { input, state: State::RowRight { source, index, left, operator } });
                                Next::Enter(child, true)
                            } else {
                                let left = pratt::fold(run, spec, left, operator, None, input.pos);
                                row(run, input, source, 0, left, &mut frames)
                            }
                        }
                        Err(failure) if failure.committed || failure.fatal => Next::Return(Err(failure)),
                        Err(_) => row(run, input, source, index + 1, left, &mut frames),
                    },
                    State::RowRight { source, index, left, operator } => match answer {
                        Ok(right) => {
                            let Op::Pratt { rows, .. } = &program.expressions[source] else { unreachable!() };
                            let left = pratt::fold(run, &rows[index], left, operator, Some(right), input.pos);
                            row(run, input, source, 0, left, &mut frames)
                        }
                        Err(mut failure) => { failure.committed |= operator.cut || left.cut; Next::Return(Err(failure)) }
                    },
                };
            }
        }
    };
    frames.clear();
    run.scratch.execute.frames = frames;
    run.scratch.alternatives.clear();
    answer
}

fn choice(run: &mut Run<'_>, input: Input, items: Span, table: usize, from: usize, failures: ChoiceFailures, frames: &mut Vec<Frame>) -> Next {
    let program = run.program;
    run.scratch.counts.dispatch_reads += 1;
    let token = run.lex(input.context.goal, input.pos);
    let text = token.token.as_ref().map_or("", |token| &run.text[token.start as usize..token.end as usize]);
    let selected = program.lowered[table].choice.as_ref().expect("choice dispatch")
        .candidates(token.token.as_ref().map(|token| token.token), text).iter().copied()
        .filter(|index| *index >= from)
        .find(|index| run.starts_at(program.instructions.operands[items.start + index], input.pos, input.context, &token));
    match selected {
        Some(selected) => {
            frames.push(Frame { input, state: State::Choice { items, table, selected, failures } });
            Next::Enter(Input { expression: program.instructions.operands[items.start + selected], ..input }, false)
        }
        None => {
            let failure = run.fail(input.pos, "alternative");
            if !run.diagnose { return Next::Return(Err(failure)); }
            replay(run, input, items, 0, failures.first, failures, failure, frames)
        }
    }
}

fn replay(run: &mut Run<'_>, input: Input, items: Span, mut next: usize, mut record: Option<usize>, failures: ChoiceFailures, mut failure: super::Failure, frames: &mut Vec<Frame>) -> Next {
    while next < items.len {
        if let Some(id) = record {
            let found = &run.scratch.alternatives[id];
            if found.to == next {
                failure = merge_error(failure, found.failure.clone());
                record = found.next;
                next += 1;
                continue;
            }
        }
        frames.push(Frame { input, state: State::Replay { items, next: next + 1, record, failures, failure } });
        return Next::Enter(Input { expression: run.program.instructions.operands[items.start + next], ..input }, false);
    }
    failures.clear(run);
    Next::Return(Err(failure))
}

fn prefix(run: &mut Run<'_>, input: Input, source: usize, from: usize, frames: &mut Vec<Frame>) -> Next {
    let program = run.program;
    let Op::Pratt { atom, prefix, .. } = &program.expressions[source] else { unreachable!() };
    run.scratch.counts.dispatch_reads += 1;
    let token = run.lex(input.context.goal, input.pos);
    let text = token.token.as_ref().map_or("", |token| &run.text[token.start as usize..token.end as usize]);
    let selected = program.lowered[source].prefix.as_ref().expect("prefix dispatch")
        .candidates(token.token.as_ref().map(|token| token.token), text).iter().copied()
        .filter(|index| *index >= from).find(|index| run.starts_at(prefix[*index].operator, input.pos, input.context, &token));
    match selected {
        Some(index) => {
            frames.push(Frame { input, state: State::PrefixProbe { source, index } });
            Next::Enter(input.child(prefix[index].operator, input.pos, 0, input.scope), false)
        }
        None => {
            frames.push(Frame { input, state: State::Atom(source) });
            Next::Enter(input.child(*atom, input.pos, 0, input.scope), false)
        }
    }
}

fn row(run: &mut Run<'_>, input: Input, source: usize, from: usize, left: Matched, frames: &mut Vec<Frame>) -> Next {
    let program = run.program;
    let Op::Pratt { rows, .. } = &program.expressions[source] else { unreachable!() };
    run.scratch.counts.dispatch_reads += 1;
    let token = run.lex(input.context.goal, left.end);
    let text = token.token.as_ref().map_or("", |token| &run.text[token.start as usize..token.end as usize]);
    let selected = program.lowered[source].rows.as_ref().expect("operator dispatch")
        .candidates(token.token.as_ref().map(|token| token.token), text).iter().copied()
        .filter(|index| *index >= from && rows[*index].power >= input.power)
        .find(|index| run.starts_at(rows[*index].operator, left.end, input.context, &token));
    match selected {
        Some(index) => {
            frames.push(Frame { input, state: State::RowProbe { source, index, left } });
            Next::Enter(input.child(rows[index].operator, left.end, 0, input.scope), false)
        }
        None => Next::Return(Ok(left)),
    }
}
