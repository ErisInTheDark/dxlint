use super::{lower::Action, lower::Lowered, Op, Program};

#[derive(Clone, Copy)]
pub(crate) enum ScopeRule {
    Keep,
    Private,
    Empty,
}

pub(crate) struct Inline {
    pub expression: usize,
    pub goal: Option<u16>,
    pub set: u64,
    pub clear: u64,
    pub power: Option<u16>,
    pub scope: ScopeRule,
    pub actions: Vec<Action>,
}

pub(super) fn compile(program: &Program, lowered: &mut [Lowered]) {
    for id in 0..lowered.len() {
        let plan = plan(program, lowered, id);
        lowered[id].inline = plan;
    }
}

fn plan(program: &Program, lowered: &[Lowered], mut expression: usize) -> Option<Inline> {
    let original = expression;
    let mut actions = Vec::new();
    let (mut goal, mut set, mut clear, mut power) = (None, 0, 0, None);
    let mut scope = ScopeRule::Keep;
    loop {
        match &program.expressions[expression] {
            Op::Bind(field, inner) => { actions.push(Action::Bind(*field)); expression = *inner; }
            Op::Emit(kind, inner) => {
                actions.push(Action::Emit(*kind, goal));
                if matches!(scope, ScopeRule::Keep) { scope = ScopeRule::Private; }
                expression = *inner;
            }
            Op::Text(inner) => { actions.push(Action::Text); expression = *inner; }
            Op::Discard(inner) => { actions.push(Action::Discard); expression = *inner; }
            Op::Try(inner) => { actions.push(Action::Try); expression = *inner; }
            Op::Context { goal: next, set: add, clear: remove, inner } => {
                goal = next.or(goal);
                set = (set | add) & !remove;
                clear = (clear & !add) | remove;
                expression = *inner;
            }
            Op::Seq(items) if items.len() == 1 => expression = items[0],
            Op::Choice(items) if items.len() == 1 => { actions.push(Action::Choice); expression = items[0]; }
            Op::Call(production, threshold) if !lowered[program.productions[*production]].memo => {
                actions.push(Action::Boundary);
                scope = ScopeRule::Empty;
                power = Some(*threshold);
                expression = program.productions[*production];
            }
            _ => break,
        }
    }
    if expression == original { return None; }
    actions.reverse();
    Some(Inline { expression, goal, set, clear, power, scope, actions })
}
