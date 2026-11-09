use super::{lower::Lowered, Op, Program};

pub(super) fn compile(program: &Program, lowered: &mut [Lowered], nullable: &[bool]) {
    let mut leading = vec![Vec::new(); program.expressions.len()];
    loop {
        let mut changed = false;
        for (id, op) in program.expressions.iter().enumerate() {
            let mut starts = Vec::new();
            let mut extend = |child: usize| {
                for production in &leading[child] {
                    if !starts.contains(production) { starts.push(*production); }
                }
            };
            match op {
                Op::Call(production, _) => { extend(program.productions[*production]); if !starts.contains(production) { starts.push(*production); } }
                Op::Seq(items) => for child in items { extend(*child); if !nullable[*child] { break; } },
                Op::Choice(items) => for child in items { extend(*child); },
                Op::Repeat { inner, max, .. } => if *max != Some(0) { extend(*inner); },
                Op::Bind(_, inner) | Op::Emit(_, inner) | Op::Context { inner, .. } |
                Op::Look(inner, _) | Op::Try(inner) | Op::Discard(inner) | Op::Text(inner) => extend(*inner),
                Op::Flag(_, yes, no) => { extend(*yes); extend(*no); }
                Op::Scope { header, body, .. } => { extend(*header); if nullable[*header] { extend(*body); } }
                Op::Pratt { atom, prefix, .. } => { extend(*atom); for operator in prefix { extend(operator.operator); } }
                _ => {}
            }
            starts.sort_unstable();
            if leading[id] != starts { leading[id] = starts; changed = true; }
        }
        if !changed { break; }
    }
    let mut targets = vec![false; program.productions.len()];
    let mut group = |items: &[usize]| {
        let mut counts = vec![0; targets.len()];
        for child in items { for production in &leading[*child] { counts[*production] += 1; } }
        for (production, count) in counts.into_iter().enumerate() { targets[production] |= count > 1; }
    };
    for (id, op) in program.expressions.iter().enumerate() {
        match op {
            Op::Choice(items) if lowered[id].choice.as_ref().is_some_and(|table| table.backtracks()) => group(items),
            Op::Pratt { atom, prefix, rows } => {
                group(&prefix.iter().map(|item| item.operator).chain([*atom]).collect::<Vec<_>>());
                if lowered[id].rows.as_ref().is_some_and(|table| table.backtracks()) {
                    group(&rows.iter().map(|item| item.operator).collect::<Vec<_>>());
                }
            }
            _ => {}
        }
    }
    let mut visited = vec![false; program.expressions.len()];
    let mut pending: Vec<_> = program.expressions.iter().filter_map(|op| match op {
        Op::Look(inner, _) if lowered[*inner].scan_depth.is_none() => Some(*inner),
        _ => None,
    }).collect();
    while let Some(id) = pending.pop() {
        if visited[id] { continue; }
        visited[id] = true;
        match &program.expressions[id] {
            Op::Call(production, _) => targets[*production] = true,
            Op::Seq(items) | Op::Choice(items) => pending.extend(items),
            Op::Repeat { inner, .. } => pending.push(*inner),
            Op::Look(inner, _) | Op::Try(inner) | Op::Bind(_, inner) | Op::Emit(_, inner) |
            Op::Context { inner, .. } | Op::Discard(inner) | Op::Text(inner) => pending.push(*inner),
            Op::Flag(_, yes, no) => pending.extend([*yes, *no]),
            Op::Scope { header, body, .. } => pending.extend([*header, *body]),
            Op::Pratt { atom, prefix, rows } => {
                pending.push(*atom);
                pending.extend(prefix.iter().map(|operator| operator.operator));
                pending.extend(rows.iter().map(|operator| operator.operator));
            }
            _ => {}
        }
    }
    for (production, enabled) in targets.into_iter().enumerate() {
        let expression = program.productions[production];
        lowered[expression].memo = enabled && !lowered[expression].terminal;
    }
}
