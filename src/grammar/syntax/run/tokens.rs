use super::{Lexed, Run};
use crate::fx_hash::FxHashMap;

struct Record {
    pos: u32,
    goal: u16,
    lexed: Lexed,
}

#[derive(Default)]
pub(super) struct Tape {
    /// The first token read at each position; `positions` indexes it.
    records: Vec<Record>,
    positions: Vec<u32>,
    /// Tokens read at a position in a goal its record cannot answer.
    alternates: FxHashMap<(u32, u16), Lexed>,
    /// The record whose token starts at each offset (first one recorded).
    starts: Vec<u32>,
    /// Predicate answers about recorded tokens, two bits per (record,
    /// predicate): `known` then `matched`, `words` words per record.
    answers: Vec<u64>,
    words: usize,
    /// Predicate answers about other spans (alternate goals, partial spans).
    predicates: FxHashMap<(usize, u32, u32), bool>,
}

impl Tape {
    pub(super) fn clear(&mut self) {
        self.records.clear();
        self.positions.clear();
        self.alternates.clear();
        self.starts.clear();
        self.answers.clear();
        self.predicates.clear();
    }

    /// The record whose token is exactly `start..end`.
    fn record_of(&self, start: u32, end: u32) -> Option<usize> {
        let id = *self.starts.get(start as usize)? as usize;
        let token = self.records.get(id)?.lexed.token.as_ref()?;
        (token.start == start && token.end == end).then_some(id)
    }

    pub(super) fn len(&self) -> usize { self.records.len() }
}

pub(super) fn prepare(run: &mut Run<'_>, goal: u16) {
    run.scratch.tokens.positions.resize(run.text.len() + 1, u32::MAX);
    run.scratch.tokens.starts.resize(run.text.len() + 1, u32::MAX);
    run.scratch.tokens.words = run.program.predicates.len().div_ceil(32);
    let mut pos = 0;
    loop {
        run.scratch.counts.prepare_reads += 1;
        let lexed = read(run, goal, pos);
        let Some(token) = lexed.token else { break; };
        if token.end <= pos { break; }
        pos = token.end;
    }
}

pub(super) fn read(run: &mut Run<'_>, goal: u16, pos: u32) -> Lexed {
    run.scratch.counts.lex_reads += 1;
    let tape = &run.scratch.tokens;
    let id = tape.positions[pos as usize] as usize;
    let recorded = tape.records.get(id).filter(|record| record.pos == pos);
    if let Some(record) = recorded {
        if record.goal == goal || compatible(run, record, goal) { return record.lexed.clone(); }
        if let Some(lexed) = run.scratch.tokens.alternates.get(&(pos, goal)) { return lexed.clone(); }
    }
    run.scratch.counts.scans += 1;
    let lexed = run.scan(goal, pos);
    let tape = &mut run.scratch.tokens;
    if recorded_at(tape, pos) {
        tape.alternates.insert((pos, goal), lexed.clone());
    } else {
        let id = tape.records.len() as u32;
        tape.positions[pos as usize] = id;
        if let Some(start) = lexed.token.as_ref().map(|token| token.start as usize) {
            if tape.starts.get(start) == Some(&u32::MAX) { tape.starts[start] = id; }
        }
        tape.records.push(Record { pos, goal, lexed: lexed.clone() });
    }
    lexed
}

fn recorded_at(tape: &Tape, pos: u32) -> bool {
    tape.records.get(tape.positions[pos as usize] as usize).is_some_and(|record| record.pos == pos)
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
    let tape = &run.scratch.tokens;
    let record = tape.record_of(start, end);
    // Two bits per predicate: 32 predicates per word.
    let slot = record.map(|id| (id * tape.words + predicate / 32, 2 * (predicate % 32)));
    match slot {
        Some((word, bit)) => {
            let answer = tape.answers.get(word).map_or(0, |word| word >> bit);
            if answer & 1 != 0 { return answer & 2 != 0; }
        }
        None => if let Some(found) = tape.predicates.get(&(predicate, start, end)) { return *found; },
    }
    let text = &run.text.as_bytes()[start as usize..end as usize];
    let found = run.program.predicates[predicate].anchored(&mut run.caches.regex.predicates[predicate], text, 0) == Some(text.len());
    let tape = &mut run.scratch.tokens;
    match slot {
        Some((word, bit)) => {
            if tape.answers.len() <= word { tape.answers.resize((word / tape.words.max(1) + 1) * tape.words.max(1), 0); }
            tape.answers[word] |= (1 | u64::from(found) << 1) << bit;
        }
        None => { tape.predicates.insert((predicate, start, end), found); }
    }
    found
}
