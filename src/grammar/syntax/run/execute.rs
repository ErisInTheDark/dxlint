use super::{merge_error, memo, pratt, Answer, Context, Fields, Input, Lexed, Matched, Run};
use super::failure::{resource, ChoiceFailures};
use crate::grammar::syntax::compile::{Action, Dispatch, Op, ScopeRule};
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
    Choice { items: Span, table: usize, selected: usize, failures: ChoiceFailures, cursor: Cursor },
    Replay { items: Span, next: usize, record: Option<usize>, failures: ChoiceFailures, failure: super::Failure },
    Repeat { inner: usize, min: u32, max: Option<u32>, count: u32, matched: Matched },
    Look(bool),
    ScopeHeader { capture: u16, body: usize },
    ScopeBody(Matched),
    PrefixProbe { source: usize, index: usize, cursor: Cursor },
    PrefixRight { source: usize, index: usize, operator: Matched },
    Atom(usize),
    RowProbe { source: usize, index: usize, left: Matched, cursor: Cursor },
    RowRight { source: usize, index: usize, left: Matched, operator: Matched },
}

/// A dispatch's place in its candidate list at one head token. The tried
/// arm's frame keeps it, so a failed arm resumes with the next candidate
/// instead of reading the head and selecting candidates again.
struct Cursor {
    head: Lexed,
    list: u32,
    next: usize,
}

impl Cursor {
    fn open(run: &mut Run<'_>, dispatch: &Dispatch, goal: u16, pos: u32) -> Self {
        run.scratch.counts.dispatch_reads += 1;
        let head = run.lex(goal, pos);
        let text = head.token.as_ref().map_or("", |token| &run.text[token.start as usize..token.end as usize]);
        let list = dispatch.candidates(head.token.as_ref().map(|token| token.token), text);
        Self { head, list, next: 0 }
    }

    /// The next candidate whose arm (`arm(index)`, `None` when ruled out) can
    /// start at `pos`. Dispatched arms skip the start check the head token
    /// already answered, except while diagnosing (failure reports stay as
    /// they were).
    fn find(&mut self, run: &mut Run<'_>, dispatch: &Dispatch, pos: u32, context: Context, arm: impl Fn(usize) -> Option<usize>) -> Option<usize> {
        let list = dispatch.list(self.list);
        while let Some(candidate) = list.get(self.next) {
            self.next += 1;
            let Some(expression) = arm(candidate.index) else { continue };
            if (candidate.dispatched && !run.diagnose) || run.starts_at(expression, pos, context, &self.head) {
                return Some(candidate.index);
            }
        }
        None
    }
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
                if !production && !run.diagnose && run.program.instructions.inline[input.expression] {
                    next = Next::Return(inline(run, input));
                    continue;
                }
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
                    Instruction::Choice { items, table } => choice(run, input, items, table, None, ChoiceFailures::new(run), &mut frames),
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
                            Some((_, values)) => Next::Return(Ok(Matched { start: run.scratch.values.first(values)
                                .map(|value| run.scratch.drafts[value as usize].start), values, ..Matched::empty(input.pos) })),
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
                    Instruction::Pratt { source } => prefix(run, input, source, None, &mut frames),
                };
            }
            Next::Return(answer) => {
                let Some(frame) = frames.pop() else { break answer; };
                let input = frame.input;
                let program = run.program;
                next = match frame.state {
                    State::Actions(actions) => Next::Return(apply_actions(run, input, actions, answer)),
                    State::Memo(key) => Next::Return(memo::finish(run, key, answer)),
                    State::Sequence { items, next, mut matched, mut scope } => match answer {
                        Ok(child) => {
                            sequence_step(run, &mut scope, &mut matched, child);
                            if next == items.len { Next::Return(Ok(matched)) }
                            else {
                                let child = input.child(program.instructions.operands[items.start + next], matched.end, input.power, scope);
                                frames.push(Frame { input, state: State::Sequence { items, next: next + 1, matched, scope } });
                                Next::Enter(child, false)
                            }
                        }
                        Err(mut failure) => { failure.committed |= matched.cut; Next::Return(Err(failure)) }
                    },
                    State::Choice { items, table, selected, mut failures, cursor } => match answer {
                        Ok(mut matched) => { failures.clear(run); matched.cut = false; Next::Return(Ok(matched)) }
                        Err(failure) if failure.committed || failure.fatal => { failures.clear(run); Next::Return(Err(failure)) }
                        Err(failure) => {
                            if run.diagnose { failures.push(run, selected, failure); }
                            choice(run, input, items, table, Some(cursor), failures, &mut frames)
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
                    State::PrefixProbe { source, index, cursor } => match answer {
                        Ok(operator) => prefix_operand(run, input, source, index, operator, &mut frames),
                        Err(failure) if failure.committed || failure.fatal => Next::Return(Err(failure)),
                        Err(_) => prefix(run, input, source, Some(cursor), &mut frames),
                    },
                    State::PrefixRight { source, index, operator } => match answer {
                        Ok(argument) => {
                            let Op::Pratt { prefix, .. } = &program.expressions[source] else { unreachable!() };
                            let left = pratt::prefix(run, &prefix[index], operator, argument, input.pos);
                            row(run, input, source, None, left, &mut frames)
                        }
                        Err(mut failure) => { failure.committed |= operator.cut; Next::Return(Err(failure)) }
                    },
                    State::Atom(source) => match answer {
                        Ok(left) => row(run, input, source, None, left, &mut frames),
                        Err(failure) => Next::Return(Err(failure)),
                    },
                    State::RowProbe { source, index, left, cursor } => match answer {
                        Ok(operator) => match row_operand(run, input, source, index, left, operator, &mut frames) {
                            RowNext::Operand(next) => next,
                            RowNext::Folded(left) => row(run, input, source, None, left, &mut frames),
                        },
                        Err(failure) if failure.committed || failure.fatal => Next::Return(Err(failure)),
                        Err(_) => row(run, input, source, Some(cursor), left, &mut frames),
                    },
                    State::RowRight { source, index, left, operator } => match answer {
                        Ok(right) => {
                            let Op::Pratt { rows, .. } = &program.expressions[source] else { unreachable!() };
                            let left = pratt::fold(run, &rows[index], left, operator, Some(right), input.pos);
                            row(run, input, source, None, left, &mut frames)
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

/// A sequence item's answer added to the sequence's match and to the scope
/// later items see. While the scope is the match's own fields, both stay one
/// shared list instead of joining the item twice.
fn sequence_step(run: &mut Run<'_>, scope: &mut Fields, matched: &mut Matched, child: Matched) {
    let shared = *scope == matched.fields;
    let fields = child.fields;
    run.append(matched, child);
    if shared { *scope = matched.fields; } else { run.scratch.fields.append(scope, fields); }
}

/// A leaf's actions applied to its answer, as its `Actions` frame would.
fn apply_actions(run: &mut Run<'_>, input: Input, actions: Span, mut answer: Answer) -> Answer {
    for action in &run.program.instructions.actions[actions.start..actions.end()] {
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
    answer
}

/// A dispatched arm that is one token match, answered in place without arm
/// or action frames. `None` when the arm needs the general path, and always
/// while diagnosing (failure replay records each arm on its frame).
fn leaf_arm(run: &mut Run<'_>, input: Input) -> Option<Answer> {
    (!run.diagnose && run.program.instructions.inline[input.expression]).then(|| inline(run, input))
}

/// An [`Instructions::inline`] graph answered in place: the answer entering
/// it would give, with no frames (its depth is bounded). Never while
/// diagnosing: failure replay records each arm on its frame.
fn inline(run: &mut Run<'_>, outer: Input) -> Answer {
    let program = run.program;
    let code = &program.instructions.code[outer.expression];
    run.scratch.counts.fused_leaves += 1;
    let mut input = outer;
    input.scope = match code.scope {
        ScopeRule::Keep => input.scope,
        ScopeRule::Empty => Fields::default(),
        ScopeRule::Private => run.private_scope(input.scope),
    };
    input.context = Context { goal: code.goal.unwrap_or(input.context.goal), flags: (input.context.flags | code.set) & !code.clear };
    let answer = match code.instruction {
        Instruction::Match { terminal, values } => run.terminal(&program.instructions.terminals[terminal], input.pos, input.context, values),
        Instruction::Pattern { predicate, emit, values } => run.token_pattern(predicate, emit, input.pos, input.context, values),
        Instruction::SameLine => {
            let token = run.lex(input.context.goal, input.pos);
            if token.trivia.is_some_and(|gap| gap.line_break) { Err(run.fail(token.start, "same line")) } else { Ok(Matched::empty(input.pos)) }
        }
        Instruction::End => {
            let token = run.lex(input.context.goal, input.pos);
            if token.start as usize == run.text.len() { Ok(Matched::empty(input.pos)) } else { Err(run.fail(token.start, "end of file")) }
        }
        Instruction::Probe { expression, goal, negative } => {
            let context = Context { goal: goal.unwrap_or(input.context.goal), ..input.context };
            let matches = run.probe_matches(super::super::compile::Probe { expression, goal }, input.pos, input.context);
            if matches != negative { Ok(Matched::empty(input.pos)) }
            else if negative { Err(run.fail(input.pos, "negative lookahead")) }
            else {
                let at = run.lex(context.goal, input.pos).start;
                Err(match &program.expressions[expression] {
                    Op::Token(terminal) => run.fail(at, &terminal.expected),
                    _ => run.fail(at, "matching token pattern"),
                })
            }
        }
        Instruction::Flag { flag, yes, no } => inline(run, Input { expression: if input.context.flags & flag != 0 { yes } else { no }, ..input }),
        Instruction::Look { inner, negative } => match inline(run, Input { expression: inner, ..input }) {
            Err(failure) if failure.fatal => Err(failure),
            Ok(_) if !negative => Ok(Matched::empty(input.pos)),
            Err(failure) if !negative => Err(failure),
            Err(_) => Ok(Matched::empty(input.pos)),
            Ok(_) => Err(run.fail(input.pos, "negative lookahead")),
        },
        Instruction::Sequence(items) => {
            let mut matched = Matched::empty(input.pos);
            let mut scope = input.scope;
            let mut failed = None;
            for &item in &program.instructions.operands[items.start..items.end()] {
                match inline(run, input.child(item, matched.end, input.power, scope)) {
                    Ok(child) => sequence_step(run, &mut scope, &mut matched, child),
                    Err(mut failure) => {
                        failure.committed |= matched.cut;
                        failed = Some(failure);
                        break;
                    }
                }
            }
            failed.map_or(Ok(matched), Err)
        }
        Instruction::Choice { items, table } => {
            let dispatch = program.lowered[table].choice.as_ref().expect("choice dispatch");
            let mut cursor = Cursor::open(run, dispatch, input.context.goal, input.pos);
            loop {
                let Some(selected) = cursor.find(run, dispatch, input.pos, input.context, |index| Some(program.instructions.operands[items.start + index])) else {
                    break Err(run.fail(input.pos, "alternative"));
                };
                match inline(run, Input { expression: program.instructions.operands[items.start + selected], ..input }) {
                    Ok(mut matched) => { matched.cut = false; break Ok(matched) }
                    Err(failure) if failure.committed || failure.fatal => break Err(failure),
                    Err(_) => {}
                }
            }
        }
        _ => unreachable!("only in-place instructions are marked inline"),
    };
    apply_actions(run, outer, code.actions, answer)
}

fn choice(run: &mut Run<'_>, input: Input, items: Span, table: usize, cursor: Option<Cursor>, failures: ChoiceFailures, frames: &mut Vec<Frame>) -> Next {
    let program = run.program;
    let dispatch = program.lowered[table].choice.as_ref().expect("choice dispatch");
    let mut cursor = cursor.unwrap_or_else(|| Cursor::open(run, dispatch, input.context.goal, input.pos));
    loop {
        let Some(selected) = cursor.find(run, dispatch, input.pos, input.context, |index| Some(program.instructions.operands[items.start + index])) else {
            let failure = run.fail(input.pos, "alternative");
            if !run.diagnose { return Next::Return(Err(failure)); }
            return replay(run, input, items, 0, failures.first, failures, failure, frames);
        };
        let arm = Input { expression: program.instructions.operands[items.start + selected], ..input };
        match leaf_arm(run, arm) {
            Some(Ok(mut matched)) => { failures.clear(run); matched.cut = false; return Next::Return(Ok(matched)); }
            Some(Err(failure)) if failure.committed || failure.fatal => { failures.clear(run); return Next::Return(Err(failure)); }
            Some(Err(_)) => {}
            None => {
                frames.push(Frame { input, state: State::Choice { items, table, selected, failures, cursor } });
                return Next::Enter(arm, false);
            }
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

fn prefix(run: &mut Run<'_>, input: Input, source: usize, cursor: Option<Cursor>, frames: &mut Vec<Frame>) -> Next {
    let program = run.program;
    let Op::Pratt { atom, prefix, .. } = &program.expressions[source] else { unreachable!() };
    let dispatch = program.lowered[source].prefix.as_ref().expect("prefix dispatch");
    let mut cursor = cursor.unwrap_or_else(|| Cursor::open(run, dispatch, input.context.goal, input.pos));
    loop {
        let Some(index) = cursor.find(run, dispatch, input.pos, input.context, |index| Some(prefix[index].operator)) else {
            frames.push(Frame { input, state: State::Atom(source) });
            return Next::Enter(input.child(*atom, input.pos, 0, input.scope), false);
        };
        let operator = input.child(prefix[index].operator, input.pos, 0, input.scope);
        match leaf_arm(run, operator) {
            Some(Ok(operator)) => return prefix_operand(run, input, source, index, operator, frames),
            Some(Err(failure)) if failure.committed || failure.fatal => return Next::Return(Err(failure)),
            Some(Err(_)) => {}
            None => {
                frames.push(Frame { input, state: State::PrefixProbe { source, index, cursor } });
                return Next::Enter(operator, false);
            }
        }
    }
}

/// After a prefix operator: its operand, parsed at the operator's power.
fn prefix_operand(run: &Run<'_>, input: Input, source: usize, index: usize, operator: Matched, frames: &mut Vec<Frame>) -> Next {
    let Op::Pratt { prefix, .. } = &run.program.expressions[source] else { unreachable!() };
    let child = input.child(source, operator.end, prefix[index].power, Fields::default());
    frames.push(Frame { input, state: State::PrefixRight { source, index, operator } });
    Next::Enter(child, true)
}

/// After an operator row matched: its right operand, or the folded postfix
/// result as the next left side.
enum RowNext {
    Operand(Next),
    Folded(Matched),
}

fn row_operand(run: &mut Run<'_>, input: Input, source: usize, index: usize, left: Matched, operator: Matched, frames: &mut Vec<Frame>) -> RowNext {
    let program = run.program;
    let Op::Pratt { rows, .. } = &program.expressions[source] else { unreachable!() };
    let spec = &rows[index];
    if !spec.operand { return RowNext::Folded(pratt::fold(run, spec, left, operator, None, input.pos)); }
    let power = if spec.right_associative { spec.power } else { spec.power + 1 };
    let child = input.child(source, operator.end, power, Fields::default());
    frames.push(Frame { input, state: State::RowRight { source, index, left, operator } });
    RowNext::Operand(Next::Enter(child, true))
}

fn row(run: &mut Run<'_>, input: Input, source: usize, cursor: Option<Cursor>, mut left: Matched, frames: &mut Vec<Frame>) -> Next {
    let program = run.program;
    let Op::Pratt { rows, .. } = &program.expressions[source] else { unreachable!() };
    let dispatch = program.lowered[source].rows.as_ref().expect("operator dispatch");
    let power = input.power;
    let mut cursor = cursor;
    loop {
        let open = cursor.get_or_insert_with(|| Cursor::open(run, dispatch, input.context.goal, left.end));
        let Some(index) = open.find(run, dispatch, left.end, input.context, |index| (rows[index].power >= power).then_some(rows[index].operator)) else {
            return Next::Return(Ok(left));
        };
        let operator = input.child(rows[index].operator, left.end, 0, input.scope);
        match leaf_arm(run, operator) {
            Some(Ok(operator)) => match row_operand(run, input, source, index, left, operator, frames) {
                RowNext::Operand(next) => return next,
                RowNext::Folded(folded) => { left = folded; cursor = None; }
            },
            Some(Err(failure)) if failure.committed || failure.fatal => return Next::Return(Err(failure)),
            Some(Err(_)) => {}
            None => {
                let cursor = cursor.take().expect("open cursor");
                frames.push(Frame { input, state: State::RowProbe { source, index, left, cursor } });
                return Next::Enter(operator, false);
            }
        }
    }
}
