//! Per-file state: pure contextual memo entries, lazy tokens, immutable receipts.

mod execute;
mod tokens;
mod memo;
mod materialize;
mod pratt;
mod failure;
mod caches;
mod arena;

pub(super) use caches::Pool;

use failure::{error, merge_error, Failure};

use crate::fx_hash::FxHashMap;
use arena::{Chain, List};

use super::compile::{Action, Op, Probe, Program, Terminal};
use super::{CompiledParser, Context, ParseError, Parsed, Registration, TokenSpan, TriviaSpan};

type Values = List<u32>;
type Fields = List<(u16, Values)>;
type Answer = Result<Matched, Failure>;

#[derive(Clone, Copy)]
struct Input {
    expression: usize,
    pos: u32,
    context: Context,
    power: u16,
    scope: Fields,
}

impl Input {
    fn child(self, expression: usize, pos: u32, power: u16, scope: Fields) -> Self {
        Self { expression, pos, power, scope, ..self }
    }
}

#[derive(Clone, Copy)]
struct Matched {
    start: Option<u32>,
    end: u32,
    values: Values,
    fields: Fields,
    receipt: Option<usize>,
    cut: bool,
}

impl Matched {
    fn empty(pos: u32) -> Self {
        Self { start: None, end: pos, values: Values::default(), fields: Fields::default(), receipt: None, cut: false }
    }
}

struct Draft {
    kind: Option<u16>,
    start: u32,
    end: u32,
    fields: Fields,
}

enum Receipt {
    Token(TokenSpan, Option<TriviaSpan>),
    Trivia(TriviaSpan),
    Join(usize, usize),
}

#[derive(Clone)]
struct Lexed {
    start: u32,
    token: Option<TokenSpan>,
    trivia: Option<TriviaSpan>,
}

pub(super) struct Run<'a> {
    parser: &'a CompiledParser,
    program: &'a Program,
    text: &'a str,
    caches: caches::Lease<'a>,
    scratch: Scratch,
    /// Collects what each failing match expected. Valid files never need
    /// it, so a parse runs without it and reruns with it only to report.
    diagnose: bool,
}

#[derive(Default)]
struct Scratch {
    tokens: tokens::Tape,
    memo: FxHashMap<(usize, u32, Context, u16), Option<Answer>>,
    drafts: Vec<Draft>,
    receipts: Vec<Receipt>,
    values: Chain<u32>,
    fields: Chain<(u16, Values)>,
    flattened: Vec<u32>,
    execute: execute::Storage,
    alternatives: Vec<failure::Alternative>,
    farthest: failure::Farthest,
    materialize: materialize::Storage,
    counts: Counts,
}

/// Work counters a parse reports to phase profiling once it ends.
#[derive(Default)]
struct Counts {
    instructions: u64,
    by_instruction: [u64; execute::INSTRUCTION_KINDS.len()],
    lex_reads: u64,
    dispatch_reads: u64,
    /// Dispatched arms that were one token match, run in place.
    fused_leaves: u64,
    start_reads: u64,
    terminal_reads: u64,
    other_goal_reads: u64,
    probe_reads: u64,
    prepare_reads: u64,
    scans: u64,
    enters: u64,
    memo_hits: u64,
    memo_stores: u64,
    impossible_starts: u64,
    /// Constant-time list concatenations (captures used to be copied).
    joins: u64,
}

impl Scratch {
    fn report(&mut self) {
        let counts = std::mem::take(&mut self.counts);
        for (name, amount) in [
            ("count.instructions", counts.instructions),
            ("count.lex_reads", counts.lex_reads),
            ("count.lex_reads.dispatch", counts.dispatch_reads),
            ("count.fused_leaves", counts.fused_leaves),
            ("count.lex_reads.start", counts.start_reads),
            ("count.lex_reads.terminal", counts.terminal_reads),
            ("count.lex_reads.other_goal", counts.other_goal_reads),
            ("count.lex_reads.probe", counts.probe_reads),
            ("count.lex_reads.prepare", counts.prepare_reads),
            ("count.lex_scans", counts.scans),
            ("count.enters", counts.enters),
            ("count.memo_hits", counts.memo_hits),
            ("count.memo_stores", counts.memo_stores),
            ("count.impossible_starts", counts.impossible_starts),
            ("count.chain_joins", counts.joins),
            ("size.memo", self.memo.len() as u64),
            ("size.drafts", self.drafts.len() as u64),
            ("size.receipts", self.receipts.len() as u64),
            ("size.values", self.values.len() as u64),
            ("size.fields", self.fields.len() as u64),
            ("size.token_records", self.tokens.len() as u64),
        ] {
            super::phases::count(name, amount);
        }
        for (name, amount) in execute::INSTRUCTION_KINDS.iter().zip(counts.by_instruction) {
            super::phases::count(name, amount);
        }
    }

    fn clear(&mut self) {
        self.tokens.clear();
        self.memo.clear();
        self.drafts.clear();
        self.receipts.clear();
        self.values.clear();
        self.fields.clear();
        self.flattened.clear();
        self.execute.clear();
        self.alternatives.clear();
        self.farthest.clear();
        self.materialize.clear();
    }
}

impl Drop for Run<'_> {
    fn drop(&mut self) {
        super::phases::phase("clear", || self.scratch.clear());
        self.caches.scratch = std::mem::take(&mut self.scratch);
    }
}

impl<'a> Run<'a> {
    pub fn new(parser: &'a CompiledParser, text: &'a str) -> Self {
        let program = &parser.program;
        let mut caches = parser.caches.lease(program);
        let scratch = std::mem::take(&mut caches.scratch);
        Self { parser, program, text, caches, scratch, diagnose: false }
    }

    pub fn parse(mut self, context: Context, registration: &Registration) -> Result<Parsed, ParseError> {
        let result = self.parse_inner(context, registration);
        self.scratch.report();
        match result {
            Err(failure) if failure.from_matching() => {
                self.scratch.clear();
                self.diagnose = true;
                let result = self.parse_inner(context, registration);
                self.scratch.report();
                result.map_err(|failure| self.public(failure))
            }
            result => result.map_err(|failure| self.public(failure)),
        }
    }

    fn parse_inner(&mut self, context: Context, registration: &Registration) -> Result<Parsed, Failure> {
        if context.goal as usize >= self.program.goals.len() || self.text.len() > u32::MAX as usize {
            return Err(error(0, "valid parser context and file size"));
        }
        if registration.matchers.len() != self.parser.nodes().len()
            || registration.labels.len() != self.parser.nodes().len()
            || registration.labels.iter().zip(self.parser.nodes()).any(|(labels, node)| labels.len() != node.fields.len())
        {
            return Err(error(0, "complete node registration"));
        }
        super::phases::phase("prepare", || tokens::prepare(self, context.goal));
        let root_expression = self.program.productions[self.program.root];
        let mut matched = super::phases::phase("match", || self.memoized(root_expression, 0, context, 0))?;
        let tail = self.lex(context.goal, matched.end);
        if tail.start as usize != self.text.len() {
            return Err(self.fail(tail.start, "end of file"));
        }
        if let Some(trivia) = tail.trivia {
            let receipt = self.receipt(Receipt::Trivia(trivia));
            matched.receipt = self.join([matched.receipt, Some(receipt)]);
        }
        if matched.values.len() != 1 {
            return Err(error(matched.end, "one root node"));
        }
        let root = *matched.values.first().expect("one root value");
        if self.scratch.drafts[root as usize].kind.is_none() {
            return Err(error(0, "a declared root node"));
        }
        self.scratch.drafts[root as usize].start = 0;
        self.scratch.drafts[root as usize].end = self.text.len() as u32;
        let (nodes, root) = super::phases::phase("materialize", || materialize::build(self, root, registration))?;
        let (tokens, trivia) = super::phases::phase("receipts", || {
            let mut tokens = Vec::new();
            let mut trivia = Vec::new();
            let mut pending: Vec<_> = matched.receipt.into_iter().collect();
            while let Some(id) = pending.pop() {
                match &self.scratch.receipts[id] {
                    Receipt::Token(token, gap) => {
                        if let Some(gap) = gap { trivia.push(gap.clone()); }
                        tokens.push(token.clone());
                    }
                    Receipt::Trivia(gap) => trivia.push(gap.clone()),
                    Receipt::Join(left, right) => { pending.push(*right); pending.push(*left); }
                }
            }
            (tokens, trivia)
        });
        Ok(Parsed { nodes, root, tokens, trivia })
    }

    fn receipt(&mut self, receipt: Receipt) -> usize {
        let id = self.scratch.receipts.len();
        self.scratch.receipts.push(receipt);
        id
    }

    fn join(&mut self, receipts: impl IntoIterator<Item = Option<usize>>) -> Option<usize> {
        let mut joined = None;
        for next in receipts.into_iter().flatten() {
            joined = Some(match joined {
                None => next,
                Some(previous) => self.receipt(Receipt::Join(previous, next)),
            });
        }
        joined
    }

    fn append(&mut self, target: &mut Matched, next: Matched) {
        if !target.values.is_empty() && !next.values.is_empty() { self.scratch.counts.joins += 1; }
        if !target.fields.is_empty() && !next.fields.is_empty() { self.scratch.counts.joins += 1; }
        target.start = target.start.or(next.start);
        target.end = next.end;
        self.scratch.values.append(&mut target.values, next.values);
        self.scratch.fields.append(&mut target.fields, next.fields);
        target.receipt = self.join([target.receipt, next.receipt]);
        target.cut |= next.cut;
    }

    fn draft(&mut self, kind: Option<u16>, start: u32, end: u32, fields: Fields) -> u32 {
        let id = self.scratch.drafts.len() as u32;
        self.scratch.drafts.push(Draft { kind, start, end, fields });
        id
    }

    fn lex(&mut self, goal: u16, pos: u32) -> Lexed { tokens::read(self, goal, pos) }

    fn scan(&mut self, goal: u16, pos: u32) -> Lexed {
        let (allowed, skip_trivia) = &self.program.goals[goal as usize];
        let caches = &mut self.caches.regex;
        let mut start = pos as usize;
        if *skip_trivia {
            loop {
                let combined = self.program.trivia_lexer.as_ref().zip(caches.trivia_scan.as_mut())
                    .map(|(lexer, cache)| lexer.anchored(cache, self.text.as_bytes(), start, |pattern|
                        self.program.trivia[pattern].anchored(&mut caches.trivia[pattern], self.text.as_bytes(), start)));
                let end = match combined {
                    Some(Ok(found)) => found.map(|(_, end)| end),
                    None | Some(Err(())) => self.program.trivia.iter().zip(&mut caches.trivia)
                        .filter_map(|(dfa, cache)| dfa.anchored(cache, self.text.as_bytes(), start)).max(),
                };
                match end {
                    Some(end) if end > start => start = end,
                    _ => break,
                }
            }
        }
        let trivia = (start != pos as usize).then(|| TriviaSpan {
            start: pos, end: start as u32,
            line_break: self.program.line_break.find(&mut caches.line, &self.text.as_bytes()[pos as usize..start], 0).is_some(),
        });
        let combined = self.program.lexers[goal as usize].as_ref().zip(caches.lexers[goal as usize].as_mut())
            .map(|(lexer, cache)| lexer.anchored(cache, self.text.as_bytes(), start, |pattern| {
                let id = allowed[pattern] as usize;
                self.program.tokens[id].anchored(&mut caches.tokens[id], self.text.as_bytes(), start)
            }));
        let token = match combined {
            Some(Ok(found)) => found.map(|(pattern, end)| TokenSpan { token: allowed[pattern], start: start as u32, end: end as u32 }),
            None | Some(Err(())) => {
                let mut token = None;
                for id in allowed {
                    if let Some(end) = self.program.tokens[*id as usize].anchored(&mut caches.tokens[*id as usize], self.text.as_bytes(), start) {
                        if end > start && token.as_ref().is_none_or(|token: &TokenSpan| end > token.end as usize || (end == token.end as usize && *id < token.token)) {
                            token = Some(TokenSpan { token: *id, start: start as u32, end: end as u32 });
                        }
                    }
                }
                token
            }
        };
        let lexed = Lexed { start: start as u32, token, trivia };
        lexed
    }

    fn terminal(&mut self, terminal: &Terminal, pos: u32, context: Context, values: bool) -> Answer {
        self.scratch.counts.terminal_reads += 1;
        let lexed = self.lex(context.goal, pos);
        let Some(token) = lexed.token.filter(|token| terminal.tokens.contains(&token.token)
            && terminal.text.as_deref().is_none_or(|text| text == &self.text[token.start as usize..token.end as usize]))
        else { return Err(self.fail(lexed.start, &terminal.expected)) };
        let values = if values {
            let value = self.draft(terminal.emit, token.start, token.end, Fields::default());
            self.scratch.values.single(value)
        } else { Values::default() };
        let receipt = self.receipt(Receipt::Token(token.clone(), lexed.trivia));
        Ok(Matched { start: Some(token.start), end: token.end, values, fields: Fields::default(), receipt: Some(receipt), cut: false })
    }

    fn token_pattern(&mut self, predicate: usize, emit: Option<u16>, pos: u32, context: Context, values: bool) -> Answer {
        self.scratch.counts.terminal_reads += 1;
        let lexed = self.lex(context.goal, pos);
        let Some(token) = lexed.token else { return Err(self.fail(lexed.start, "matching token pattern")) };
        if !tokens::predicate(self, predicate, token.start, token.end) {
            return Err(self.fail(token.start, "matching token pattern"));
        }
        let values = if values {
            let value = self.draft(emit, token.start, token.end, Fields::default());
            self.scratch.values.single(value)
        } else { Values::default() };
        let receipt = self.receipt(Receipt::Token(token.clone(), lexed.trivia));
        Ok(Matched { start: Some(token.start), end: token.end, values, fields: Fields::default(), receipt: Some(receipt), cut: false })
    }

    fn memoized(&mut self, expression: usize, pos: u32, context: Context, power: u16) -> Answer {
        execute::evaluate(self, Input { expression, pos, context, power, scope: Fields::default() })
    }

    fn private_scope(&mut self, scope: Fields) -> Fields {
        if self.scratch.fields.reversed(scope).all(|(field, _)| field as usize >= self.program.public_fields) { return scope; }
        let public_fields = self.program.public_fields;
        self.scratch.fields.filter(scope, |(field, _)| field as usize >= public_fields)
    }

    fn apply(&mut self, matched: &mut Matched, action: &Action, pos: u32, context: Context) {
        match action {
            Action::Bind(field) => self.scratch.fields.push(&mut matched.fields, (*field, matched.values)),
            Action::Emit(kind, goal) => {
                let (start, end) = match matched.start {
                    Some(start) => (start, matched.end),
                    None => {
                        let anchor = self.lex(goal.unwrap_or(context.goal), pos).start;
                        (anchor, anchor)
                    }
                };
                let value = self.draft(Some(*kind), start, end, std::mem::take(&mut matched.fields));
                matched.values = self.scratch.values.single(value);
            }
            Action::Text => {
                let value = self.draft(None, matched.start.expect("terminal shape consumes text"), matched.end, Fields::default());
                matched.values = self.scratch.values.single(value);
                matched.fields = Fields::default();
            }
            Action::Discard => { matched.values = Values::default(); matched.fields = Fields::default(); }
            Action::Boundary => { matched.fields = Fields::default(); matched.cut = false; }
            Action::Choice => matched.cut = false,
            Action::Try => {}
        }
    }

    fn probe_matches(&mut self, probe: Probe, pos: u32, context: Context) -> bool {
        self.scratch.counts.probe_reads += 1;
        let head = self.lex(context.goal, pos);
        self.probe_matches_at(probe, pos, context, &head)
    }

    /// Whether `probe` matches at `pos`, where `head` is the token there in
    /// the current goal; probes in another goal lex their own.
    fn probe_matches_at(&mut self, probe: Probe, pos: u32, context: Context, head: &Lexed) -> bool {
        let other;
        let lexed = match probe.goal {
            Some(goal) if goal != context.goal => {
                self.scratch.counts.other_goal_reads += 1;
                other = self.lex(goal, pos);
                &other
            }
            _ => head,
        };
        let Some(token) = &lexed.token else { return false };
        let token = token.clone();
        self.token_matches(probe.expression, &token)
    }

    /// Whether `token` is what the token or token-pattern `expression` reads.
    fn token_matches(&mut self, expression: usize, token: &TokenSpan) -> bool {
        match &self.program.expressions[expression] {
            Op::Token(terminal) => terminal.tokens.contains(&token.token)
                && terminal.text.as_deref().is_none_or(|text| text == &self.text[token.start as usize..token.end as usize]),
            Op::TokenPattern { predicate, .. } => {
                tokens::predicate(self, *predicate, token.start, token.end)
            }
            _ => unreachable!(),
        }
    }

    fn possible_start(&mut self, expression: usize, pos: u32, context: Context) -> bool {
        if self.program.starts[expression].is_none() { return true; }
        self.scratch.counts.start_reads += 1;
        let head = self.lex(context.goal, pos);
        self.starts_at(expression, pos, context, &head)
    }

    /// [`Self::possible_start`] with the current goal's token at `pos` already read.
    fn starts_at(&mut self, expression: usize, pos: u32, context: Context, head: &Lexed) -> bool {
        let program = self.program;
        let Some(start) = &program.starts[expression] else { return true; };
        if let Some(token) = &head.token {
            if start.current.has(token.token) { return true; }
            if start.current.residual.iter().any(|&expression| self.token_matches(expression, token)) { return true; }
        }
        for (goal, starts) in &start.goals {
            let other;
            let lexed = if *goal == context.goal { head } else {
                self.scratch.counts.other_goal_reads += 1;
                other = self.lex(*goal, pos);
                &other
            };
            let Some(token) = &lexed.token else { continue };
            if starts.has(token.token) { return true; }
            if starts.residual.iter().any(|&expression| self.token_matches(expression, token)) { return true; }
        }
        false
    }

    fn field_path(&self, fields: Fields, path: &[u16]) -> Option<&Draft> {
        let (first, rest) = path.split_first()?;
        let (_, values) = self.scratch.fields.reversed(fields).find(|(field, _)| field == first)?;
        let mut draft = self.scratch.drafts.get(*values.first()? as usize)?;
        for component in rest {
            let (_, values) = self.scratch.fields.reversed(draft.fields).filter(|(field, _)| field == component).last()?;
            draft = self.scratch.drafts.get(*values.first()? as usize)?;
        }
        Some(draft)
    }

    fn without_pattern(&mut self, span: (u32, u32), predicate: usize) -> Vec<u8> {
        let source = &self.text.as_bytes()[span.0 as usize..span.1 as usize];
        let mut result = Vec::with_capacity(source.len());
        let mut at = 0;
        while let Some((start, end)) = self.program.predicates[predicate].find(&mut self.caches.regex.predicates[predicate], source, at) {
            result.extend_from_slice(&source[at..start]);
            at = end;
        }
        result.extend_from_slice(&source[at..]);
        result
    }


}
