use super::{dispatch::compile as dispatch, Dispatch, Op, Program};

#[derive(Clone)]
pub(crate) enum Action {
    Bind(u16),
    Emit(u16, Option<u16>),
    Text,
    Discard,
    Boundary,
    Choice,
    Try,
}

pub(crate) struct Leaf {
    pub expression: usize,
    pub goal: Option<u16>,
    pub set: u64,
    pub clear: u64,
    pub actions: Vec<Action>,
}

pub(crate) struct Lowered {
    pub inline: Option<super::inline::Inline>,
    pub leaf: Option<Leaf>,
    pub choice: Option<Dispatch>,
    pub prefix: Option<Dispatch>,
    pub rows: Option<Dispatch>,
    pub memo: bool,
    pub scan_depth: Option<usize>,
    pub terminal: bool,
    pub check: bool,
}

pub(super) fn compile(program: &Program, nullable: &[bool]) -> Vec<Lowered> {
    let mut lowered: Vec<_> = program.expressions.iter().enumerate().map(|(id, op)| {
        let (choice, prefix, rows) = match op {
            Op::Choice(items) => (Some(dispatch(program, items)), None, None),
            Op::Pratt { prefix, rows, .. } => (None,
                Some(dispatch(program, &prefix.iter().map(|item| item.operator).collect::<Vec<_>>())),
                Some(dispatch(program, &rows.iter().map(|item| item.operator).collect::<Vec<_>>()))),
            _ => (None, None, None),
        };
        Lowered { inline: None, leaf: leaf(program, id), choice, prefix, rows, memo: false, scan_depth: None, terminal: false, check: false }
    }).collect();
    loop {
        let mut changed = false;
        for (id, op) in program.expressions.iter().enumerate() {
            let depth = match op {
                Op::Token(_) | Op::TokenPattern { .. } | Op::SameLine | Op::End => Some(1),
                Op::Call(production, _) => lowered[program.productions[*production]].scan_depth,
                Op::Seq(items) | Op::Choice(items) => items.iter()
                    .map(|child| lowered[*child].scan_depth).collect::<Option<Vec<_>>>()
                    .map(|depths| depths.into_iter().max().unwrap_or(0)),
                Op::Flag(_, yes, no) => lowered[*yes].scan_depth.zip(lowered[*no].scan_depth).map(|(a, b)| a.max(b)),
                Op::Bind(_, inner) | Op::Emit(_, inner) | Op::Context { inner, .. } | Op::Look(inner, _) |
                Op::Try(inner) | Op::Discard(inner) | Op::Text(inner) | Op::Repeat { inner, .. } => lowered[*inner].scan_depth,
                _ => None,
            }.and_then(|depth| (depth < 64).then_some(depth + 1));
            if lowered[id].scan_depth != depth { lowered[id].scan_depth = depth; changed = true; }
            let check = depth.is_some() && match op {
                Op::Look(_, _) | Op::SameLine | Op::End => true,
                Op::Context { inner, .. } | Op::Try(inner) => lowered[*inner].check,
                Op::Flag(_, yes, no) => lowered[*yes].check && lowered[*no].check,
                Op::Seq(items) => items.iter().all(|child| lowered[*child].check),
                Op::Choice(items) => !items.is_empty() && items.iter().all(|child| lowered[*child].check),
                _ => false,
            };
            if lowered[id].check != check { lowered[id].check = check; changed = true; }
            let terminal = depth.is_some() && (lowered[id].leaf.is_some() || match op {
                Op::Choice(items) => !items.is_empty() && items.iter().all(|child| lowered[*child].terminal),
                Op::Flag(_, yes, no) => lowered[*yes].terminal && lowered[*no].terminal,
                Op::Call(production, _) => lowered[program.productions[*production]].terminal,
                Op::Seq(items) => items.iter().filter(|child| lowered[**child].terminal).count() == 1
                    && items.iter().all(|child| lowered[*child].terminal || lowered[*child].check),
                Op::Bind(_, inner) | Op::Emit(_, inner) | Op::Context { inner, .. } |
                Op::Try(inner) | Op::Discard(inner) | Op::Text(inner) => lowered[*inner].terminal,
                _ => false,
            });
            if lowered[id].terminal != terminal { lowered[id].terminal = terminal; changed = true; }
        }
        if !changed { break; }
    }
    super::memo::compile(program, &mut lowered, nullable);
    super::inline::compile(program, &mut lowered);
    lowered
}

fn leaf(program: &Program, mut expression: usize) -> Option<Leaf> {
    let mut actions = Vec::new();
    let (mut goal, mut set, mut clear) = (None, 0, 0);
    for _ in 0..64 {
        match &program.expressions[expression] {
            Op::Token(_) | Op::TokenPattern { .. } => {
                actions.reverse();
                return Some(Leaf { expression, goal, set, clear, actions });
            }
            Op::Bind(field, inner) => { actions.push(Action::Bind(*field)); expression = *inner; }
            Op::Emit(kind, inner) => { actions.push(Action::Emit(*kind, None)); expression = *inner; }
            Op::Text(inner) => { actions.push(Action::Text); expression = *inner; }
            Op::Discard(inner) => { actions.push(Action::Discard); expression = *inner; }
            Op::Try(inner) => expression = *inner,
            Op::Seq(items) if items.len() == 1 => expression = items[0],
            Op::Call(production, _) => {
                actions.push(Action::Boundary);
                expression = program.productions[*production];
            }
            Op::Context { goal: next, set: add, clear: remove, inner } => {
                goal = next.or(goal);
                set = (set | add) & !remove;
                clear = (clear & !add) | remove;
                expression = *inner;
            }
            _ => return None,
        }
    }
    None
}
