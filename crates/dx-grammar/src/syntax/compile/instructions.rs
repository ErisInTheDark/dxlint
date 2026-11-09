use super::{Action, Op, Program, ScopeRule, Terminal};

#[derive(Clone, Copy, Default)]
pub(crate) struct Span {
    pub start: usize,
    pub len: usize,
}

impl Span {
    pub fn end(self) -> usize { self.start + self.len }
}

#[derive(Clone, Copy)]
pub(crate) enum Instruction {
    Match { terminal: usize, values: bool },
    Pattern { predicate: usize, emit: Option<u16>, values: bool },
    Call { root: usize, power: u16 },
    Sequence(Span),
    Choice { items: Span, table: usize },
    Repeat { inner: usize, min: u32, max: Option<u32> },
    Flag { flag: u64, yes: usize, no: usize },
    Look { inner: usize, negative: bool },
    Probe { expression: usize, goal: Option<u16>, negative: bool },
    Scope { capture: u16, header: usize, body: usize },
    Reuse(u16),
    Cut,
    SameLine,
    SameText { source: usize },
    Terminator { source: usize },
    End,
    Pratt { source: usize },
}

pub(crate) struct Code {
    pub instruction: Instruction,
    pub goal: Option<u16>,
    pub set: u64,
    pub clear: u64,
    pub power: Option<u16>,
    pub scope: ScopeRule,
    pub actions: Span,
}

#[derive(Default)]
pub(crate) struct Instructions {
    pub code: Vec<Code>,
    pub operands: Vec<usize>,
    pub actions: Vec<Action>,
    pub terminals: Vec<Terminal>,
    /// Bounded, call-free graphs (tokens, checks, choices and sequences of
    /// them) the runtime answers in place, without frames.
    pub inline: Vec<bool>,
}

pub(super) fn compile(program: &Program) -> Instructions {
    let mut out = Instructions::default();
    for (id, lowered) in program.lowered.iter().enumerate() {
        let (source, goal, set, clear, power, scope, actions) = if let Some(leaf) = &lowered.leaf {
            (leaf.expression, leaf.goal, leaf.set, leaf.clear, None, ScopeRule::Keep, leaf.actions.as_slice())
        } else if let Some(plan) = &lowered.inline {
            (plan.expression, plan.goal, plan.set, plan.clear, plan.power, plan.scope, plan.actions.as_slice())
        } else {
            (id, None, 0, 0, None, ScopeRule::Keep, &[][..])
        };
        let action_span = Span { start: out.actions.len(), len: actions.len() };
        out.actions.extend(actions.iter().cloned());
        let values = actions.iter().find_map(|action| match action {
            Action::Bind(_) => Some(true),
            Action::Emit(..) | Action::Text | Action::Discard => Some(false),
            _ => None,
        }).unwrap_or(true);
        let instruction = match &program.expressions[source] {
            Op::Token(terminal) => {
                let index = out.terminals.len();
                out.terminals.push(terminal.clone());
                Instruction::Match { terminal: index, values }
            }
            Op::TokenPattern { predicate, emit } => Instruction::Pattern { predicate: *predicate, emit: *emit, values },
            Op::Call(production, power) => Instruction::Call { root: program.productions[*production], power: *power },
            Op::Seq(items) | Op::Choice(items) => {
                let span = Span { start: out.operands.len(), len: items.len() };
                out.operands.extend(items);
                if matches!(program.expressions[source], Op::Choice(_)) {
                    Instruction::Choice { items: span, table: source }
                } else { Instruction::Sequence(span) }
            }
            Op::Repeat { inner, min, max } => Instruction::Repeat { inner: *inner, min: *min, max: *max },
            Op::Flag(flag, yes, no) => Instruction::Flag { flag: *flag, yes: *yes, no: *no },
            Op::Look(inner, negative) => match program.first[*inner].as_ref().and_then(|first| first.one) {
                Some(probe) => Instruction::Probe { expression: probe.expression, goal: probe.goal, negative: *negative },
                None => Instruction::Look { inner: *inner, negative: *negative },
            },
            Op::Scope { capture, header, body } => Instruction::Scope { capture: *capture, header: *header, body: *body },
            Op::Reuse(capture) => Instruction::Reuse(*capture),
            Op::Cut => Instruction::Cut,
            Op::SameLine => Instruction::SameLine,
            Op::SameText(..) => Instruction::SameText { source },
            Op::Terminator { .. } => Instruction::Terminator { source },
            Op::End => Instruction::End,
            Op::Pratt { .. } => Instruction::Pratt { source },
            _ => unreachable!("wrappers have compiled plans"),
        };
        out.code.push(Code { instruction, goal, set, clear, power, scope, actions: action_span });
    }
    out.inline = inline(program, &out);
    out
}

/// Which expressions [`Instructions::inline`] marks: bounded scan depth (so
/// acyclic), no production call, no Pratt power, built only from in-place
/// instructions.
fn inline(program: &Program, out: &Instructions) -> Vec<bool> {
    let mut flags = vec![false; out.code.len()];
    loop {
        let mut changed = false;
        for (id, code) in out.code.iter().enumerate() {
            if flags[id] || code.power.is_some() || program.lowered[id].scan_depth.is_none() { continue }
            let ok = match code.instruction {
                Instruction::Match { .. } | Instruction::Pattern { .. } | Instruction::SameLine | Instruction::End | Instruction::Probe { .. } => true,
                Instruction::Sequence(items) | Instruction::Choice { items, .. } => out.operands[items.start..items.end()].iter().all(|&item| flags[item]),
                Instruction::Flag { yes, no, .. } => flags[yes] && flags[no],
                Instruction::Look { inner, .. } => flags[inner],
                _ => false,
            };
            if ok {
                flags[id] = true;
                changed = true;
            }
        }
        if !changed { break }
    }
    flags
}
