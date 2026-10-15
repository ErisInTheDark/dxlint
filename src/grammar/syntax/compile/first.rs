use super::{Op, Program};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Probe {
    pub expression: usize,
    pub goal: Option<u16>,
}

#[derive(Clone, PartialEq, Eq)]
pub(crate) struct First {
    pub nullable: bool,
    pub skip_safe: bool,
    pub probes: Vec<Probe>,
    pub one: Option<Probe>,
}

impl Default for First {
    fn default() -> Self { Self { nullable: false, skip_safe: true, probes: Vec::new(), one: None } }
}

impl First {
    fn token(expression: usize) -> Self {
        let probe = Probe { expression, goal: None };
        Self { nullable: false, skip_safe: true, probes: vec![probe], one: Some(probe) }
    }

    fn extend(&mut self, other: &Self) {
        self.skip_safe &= other.skip_safe;
        for probe in &other.probes {
            if !self.probes.contains(probe) { self.probes.push(*probe); }
        }
    }
}

/// How a run rules out an expression before entering it: what the token at
/// the position may be, in the current goal and in each goal a start fixes.
/// Absent when the expression may start anywhere.
pub(crate) struct Start {
    pub current: GoalStart,
    /// Goals named by starting tokens, each read once per check (the current
    /// goal's own head when it is the same goal).
    pub goals: Vec<(u16, GoalStart)>,
}

/// One goal's starting tokens: plain token ids, then token expressions
/// needing their text or a pattern.
pub(crate) struct GoalStart {
    tokens: Box<[u64]>,
    pub residual: Vec<usize>,
}

impl GoalStart {
    fn new(program: &Program) -> Self {
        Self { tokens: vec![0; program.tokens.len().div_ceil(64)].into(), residual: Vec::new() }
    }

    pub fn has(&self, token: u16) -> bool {
        self.tokens[token as usize / 64] & (1 << (token % 64)) != 0
    }

    fn add(&mut self, program: &Program, expression: usize) {
        match &program.expressions[expression] {
            Op::Token(terminal) if terminal.text.is_none() => {
                for token in &terminal.tokens { self.tokens[*token as usize / 64] |= 1 << (token % 64); }
            }
            _ => if !self.residual.contains(&expression) { self.residual.push(expression) },
        }
    }
}

pub(super) fn starts(program: &Program, first: &[Option<First>]) -> Vec<Option<Start>> {
    first.iter().map(|first| {
        let first = first.as_ref().filter(|first| !first.nullable && first.skip_safe)?;
        let mut start = Start { current: GoalStart::new(program), goals: Vec::new() };
        for probe in &first.probes {
            match probe.goal {
                None => start.current.add(program, probe.expression),
                Some(goal) => {
                    let index = match start.goals.iter().position(|(own, _)| *own == goal) {
                        Some(index) => index,
                        None => {
                            start.goals.push((goal, GoalStart::new(program)));
                            start.goals.len() - 1
                        }
                    };
                    start.goals[index].1.add(program, probe.expression);
                }
            }
        }
        Some(start)
    }).collect()
}

pub(super) fn compile(program: &Program) -> Vec<Option<First>> {
    let mut first = vec![None; program.expressions.len()];
    loop {
        let mut changed = false;
        for (id, op) in program.expressions.iter().enumerate() {
            let result = derive(program, &first, id, op);
            if result != first[id] {
                first[id] = result;
                changed = true;
            }
        }
        if !changed { return first; }
    }
}

fn sequence(first: &[Option<First>], items: impl IntoIterator<Item = usize>) -> Option<First> {
    let mut result = First { nullable: true, ..First::default() };
    for id in items {
        let next = first[id].as_ref()?;
        result.extend(next);
        if !next.nullable {
            result.nullable = false;
            break;
        }
    }
    Some(result)
}

fn alternatives(first: &[Option<First>], items: impl IntoIterator<Item = usize>) -> Option<First> {
    let mut result = First::default();
    for id in items {
        let next = first[id].as_ref()?;
        result.extend(next);
        result.nullable |= next.nullable;
    }
    Some(result)
}

fn derive(program: &Program, first: &[Option<First>], id: usize, op: &Op) -> Option<First> {
    match op {
        Op::Token(_) | Op::TokenPattern { .. } => Some(First::token(id)),
        Op::Call(production, _) => first[program.productions[*production]].clone(),
        Op::Seq(items) => {
            let mut result = sequence(first, items.iter().copied())?;
            if let [inner] = &items[..] { result.one = first[*inner].as_ref()?.one; }
            Some(result)
        }
        Op::Choice(items) => alternatives(first, items.iter().copied()),
        Op::Repeat { inner, min, max } => {
            if *max == Some(0) { return Some(First { nullable: true, ..First::default() }); }
            let mut result = first[*inner].clone()?;
            if *min == 0 {
                result.nullable = true;
            }
            if *min != 1 || *max != Some(1) { result.one = None; }
            Some(result)
        }
        Op::Bind(_, inner) | Op::Emit(_, inner) | Op::Text(inner) | Op::Discard(inner) | Op::Try(inner) => first[*inner].clone(),
        Op::Context { goal, inner, .. } => {
            let mut result = first[*inner].clone()?;
            if let Some(goal) = goal {
                for probe in &mut result.probes {
                    if probe.goal.is_none() { probe.goal = Some(*goal); }
                }
                if let Some(one) = &mut result.one {
                    if one.goal.is_none() { one.goal = Some(*goal); }
                }
            }
            Some(result)
        }
        Op::Flag(_, yes, no) => alternatives(first, [*yes, *no]),
        Op::Scope { header, body, .. } => sequence(first, [*header, *body]),
        Op::Pratt { atom, prefix, .. } => alternatives(first, prefix.iter().map(|operator| operator.operator).chain([*atom])),
        Op::SameLine | Op::Reuse(_) | Op::SameText(..) | Op::End => Some(First { nullable: true, ..First::default() }),
        Op::Cut => Some(First { nullable: true, skip_safe: false, ..First::default() }),
        Op::Look(inner, _) => Some(First { nullable: true,
            skip_safe: first[*inner].as_ref().is_some_and(|first| first.one.is_some()), ..First::default() }),
        Op::Terminator { .. } => None,
    }
}
