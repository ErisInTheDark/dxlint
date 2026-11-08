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
    /// The latest read: a choice, its arm and the arm's first token all
    /// read the same position in the same goal.
    last: Option<(u32, u16, Lexed)>,
}

impl Tape {
    pub(super) fn clear(&mut self) {
        self.last = None;
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
        let lexed = self.records.get(id)?.lexed;
        (lexed.kind().is_some() && lexed.start == start && lexed.end == end).then_some(id)
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
        if lexed.kind().is_none() || lexed.end <= pos { break; }
        pos = lexed.end;
    }
}

#[inline]
pub(super) fn read(run: &mut Run<'_>, goal: u16, pos: u32) -> Lexed {
    run.scratch.counts.lex_reads += 1;
    if let Some((last_pos, last_goal, lexed)) = run.scratch.tokens.last {
        if last_pos == pos && last_goal == goal { return lexed; }
    }
    let lexed = lookup(run, goal, pos);
    run.scratch.tokens.last = Some((pos, goal, lexed));
    lexed
}

#[inline(never)]
fn lookup(run: &mut Run<'_>, goal: u16, pos: u32) -> Lexed {
    let tape = &run.scratch.tokens;
    let id = tape.positions[pos as usize] as usize;
    let recorded = tape.records.get(id).filter(|record| record.pos == pos);
    if let Some(record) = recorded {
        if record.goal == goal || compatible(run, record, goal) { return record.lexed; }
        if let Some(lexed) = run.scratch.tokens.alternates.get(&(pos, goal)) { return *lexed; }
    }
    run.scratch.counts.scans += 1;
    let lexed = run.scan(goal, pos);
    let tape = &mut run.scratch.tokens;
    if recorded_at(tape, pos) {
        tape.alternates.insert((pos, goal), lexed);
    } else {
        let id = tape.records.len() as u32;
        tape.positions[pos as usize] = id;
        if lexed.kind().is_some() && tape.starts.get(lexed.start as usize) == Some(&u32::MAX) {
            tape.starts[lexed.start as usize] = id;
        }
        tape.records.push(Record { pos, goal, lexed });
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
    let Some(kind) = record.lexed.kind() else {
        return record.lexed.start as usize == run.text.len();
    };
    if !program.goal_tokens[goal as usize][kind as usize] { return false; }
    let byte = run.text.as_bytes()[record.lexed.start as usize];
    !program.goal_changes[record.goal as usize][goal as usize].contains(byte)
}

/// Whether the token `start..end`, whose word is `word`, matches `predicate`.
pub(super) fn predicate(run: &mut Run<'_>, predicate: usize, start: u32, end: u32, word: u32) -> bool {
    if let Some(words) = &run.program.predicate_words[predicate] { return words.binary_search(&word).is_ok(); }
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
