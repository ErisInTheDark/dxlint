use super::{Lexed, Run};
use crate::fx_hash::FxHashMap;

struct Record {
    pos: u32,
    goal: u16,
    lexed: Lexed,
}

#[derive(Default)]
pub(super) struct Tape {
    records: Vec<Record>,
    positions: Vec<u32>,
    predicates: FxHashMap<(usize, u32, u32), bool>,
}

impl Tape {
    pub(super) fn clear(&mut self) {
        self.records.clear();
        self.positions.clear();
        self.predicates.clear();
    }
}

pub(super) fn prepare(run: &mut Run<'_>, goal: u16) {
    run.scratch.tokens.positions.resize(run.text.len() + 1, u32::MAX);
    let mut pos = 0;
    loop {
        let lexed = read(run, goal, pos);
        let Some(token) = lexed.token else { break; };
        if token.end <= pos { break; }
        pos = token.end;
    }
}

pub(super) fn read(run: &mut Run<'_>, goal: u16, pos: u32) -> Lexed {
    let tape = &run.scratch.tokens;
    let id = tape.positions[pos as usize] as usize;
    if let Some(record) = tape.records.get(id).filter(|record| record.pos == pos) {
        if record.goal == goal || compatible(run, record, goal) { return record.lexed.clone(); }
    }
    let lexed = run.scan(goal, pos);
    let tape = &mut run.scratch.tokens;
    let tail = tape.records.partition_point(|record| record.pos < pos);
    tape.records.truncate(tail);
    tape.positions[pos as usize] = tail as u32;
    tape.records.push(Record { pos, goal, lexed: lexed.clone() });
    lexed
}

fn compatible(run: &Run<'_>, record: &Record, goal: u16) -> bool {
    let program = run.program;
    let (_, old_trivia) = &program.goals[record.goal as usize];
    let (_, new_trivia) = &program.goals[goal as usize];
    if old_trivia != new_trivia {
        if record.lexed.start != record.pos { return false; }
        if *new_trivia && run.text.as_bytes().get(record.pos as usize)
            .is_some_and(|byte| program.trivia_first.contains(*byte)) { return false; }
    }
    let Some(token) = &record.lexed.token else {
        return record.lexed.start as usize == run.text.len();
    };
    if !program.goal_tokens[goal as usize][token.token as usize] { return false; }
    let byte = run.text.as_bytes()[token.start as usize];
    !program.goal_changes[record.goal as usize][goal as usize].contains(byte)
}

pub(super) fn predicate(run: &mut Run<'_>, predicate: usize, start: u32, end: u32) -> bool {
    let key = (predicate, start, end);
    if let Some(found) = run.scratch.tokens.predicates.get(&key) { return *found; }
    let text = &run.text.as_bytes()[start as usize..end as usize];
    let found = run.program.predicates[predicate].anchored(&mut run.caches.regex.predicates[predicate], text, 0) == Some(text.len());
    run.scratch.tokens.predicates.insert(key, found);
    found
}
