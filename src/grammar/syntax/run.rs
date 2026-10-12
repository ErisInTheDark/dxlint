//! Per-file state: pure contextual memo entries, lazy tokens, immutable receipts.

use std::collections::HashMap;

use regex_automata::meta::Cache;

use super::compile::{Op, Program, Terminal};
use super::{CompiledParser, Context, ParseError, Parsed, Registration, TokenSpan, TriviaSpan};
use crate::grammar::{Edge, Node};

type Fields = Vec<(u16, Vec<u32>)>;
type Answer = Result<Matched, ParseError>;

#[derive(Clone)]
struct Matched {
    start: Option<u32>,
    end: u32,
    values: Vec<u32>,
    fields: Fields,
    receipt: Option<usize>,
    cut: bool,
}

impl Matched {
    fn empty(pos: u32) -> Self {
        Self { start: None, end: pos, values: Vec::new(), fields: Vec::new(), receipt: None, cut: false }
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
    Join(Vec<usize>),
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
    token_caches: Vec<Cache>,
    trivia_caches: Vec<Cache>,
    line_cache: Cache,
    lexed: HashMap<(u16, u32), Lexed>,
    memo: HashMap<(usize, u32, Context, u16), Option<Answer>>,
    drafts: Vec<Draft>,
    receipts: Vec<Receipt>,
    depth: usize,
}

fn error(pos: u32, expected: impl Into<String>) -> ParseError {
    ParseError { offset: pos, expected: vec![expected.into()], committed: false }
}

fn merge_error(left: ParseError, mut right: ParseError) -> ParseError {
    if left.offset > right.offset { return left; }
    if right.offset > left.offset { return right; }
    for expected in left.expected {
        if !right.expected.contains(&expected) { right.expected.push(expected); }
    }
    right
}

impl<'a> Run<'a> {
    pub fn new(parser: &'a CompiledParser, text: &'a str) -> Self {
        let program = &parser.program;
        Self {
            parser, program, text,
            token_caches: program.tokens.iter().map(|dfa| dfa.cache()).collect(),
            trivia_caches: program.trivia.iter().map(|dfa| dfa.cache()).collect(),
            line_cache: program.line_break.cache(),
            lexed: HashMap::new(), memo: HashMap::new(), drafts: Vec::new(), receipts: Vec::new(), depth: 0,
        }
    }

    pub fn parse(mut self, context: Context, registration: &Registration) -> Result<Parsed, ParseError> {
        if context.goal as usize >= self.program.goals.len() || self.text.len() > u32::MAX as usize {
            return Err(error(0, "valid parser context and file size"));
        }
        if registration.matchers.len() != self.parser.nodes().len()
            || registration.labels.len() != self.parser.nodes().len()
            || registration.labels.iter().zip(self.parser.nodes()).any(|(labels, node)| labels.len() != node.fields.len())
        {
            return Err(error(0, "complete node registration"));
        }
        let root_expression = self.program.productions[self.program.root];
        let mut matched = self.memoized(root_expression, 0, context, 0)?;
        let tail = self.lex(context.goal, matched.end);
        if tail.start as usize != self.text.len() {
            return Err(error(tail.start, "end of file"));
        }
        if let Some(trivia) = tail.trivia {
            let receipt = self.receipt(Receipt::Trivia(trivia));
            matched.receipt = self.join([matched.receipt, Some(receipt)]);
        }
        if matched.values.len() != 1 {
            return Err(error(matched.end, "one root node"));
        }
        let root = matched.values[0];
        if self.drafts[root as usize].kind.is_none() {
            return Err(error(0, "a declared root node"));
        }
        self.drafts[root as usize].start = 0;
        self.drafts[root as usize].end = self.text.len() as u32;
        let (nodes, root) = self.materialize(root, registration)?;
        let mut tokens = Vec::new();
        let mut trivia = Vec::new();
        let mut pending: Vec<_> = matched.receipt.into_iter().collect();
        while let Some(id) = pending.pop() {
            match &self.receipts[id] {
                Receipt::Token(token, gap) => {
                    if let Some(gap) = gap { trivia.push(gap.clone()); }
                    tokens.push(token.clone());
                }
                Receipt::Trivia(gap) => trivia.push(gap.clone()),
                Receipt::Join(children) => pending.extend(children.iter().rev()),
            }
        }
        Ok(Parsed { nodes, root, tokens, trivia })
    }

    fn receipt(&mut self, receipt: Receipt) -> usize {
        let id = self.receipts.len();
        self.receipts.push(receipt);
        id
    }

    fn join(&mut self, receipts: impl IntoIterator<Item = Option<usize>>) -> Option<usize> {
        let mut receipts: Vec<_> = receipts.into_iter().flatten().collect();
        match receipts.len() {
            0 => None,
            1 => receipts.pop(),
            _ => Some(self.receipt(Receipt::Join(receipts))),
        }
    }

    fn append(&mut self, target: &mut Matched, mut next: Matched) {
        target.start = target.start.or(next.start);
        target.end = next.end;
        target.values.append(&mut next.values);
        target.fields.append(&mut next.fields);
        target.receipt = self.join([target.receipt, next.receipt]);
        target.cut |= next.cut;
    }

    fn draft(&mut self, kind: Option<u16>, start: u32, end: u32, fields: Fields) -> u32 {
        let id = self.drafts.len() as u32;
        self.drafts.push(Draft { kind, start, end, fields });
        id
    }

    fn lex(&mut self, goal: u16, pos: u32) -> Lexed {
        if let Some(lexed) = self.lexed.get(&(goal, pos)) { return lexed.clone(); }
        let (allowed, skip_trivia) = &self.program.goals[goal as usize];
        let mut start = pos as usize;
        if *skip_trivia {
            loop {
                let end = self.program.trivia.iter().zip(&mut self.trivia_caches)
                    .filter_map(|(dfa, cache)| dfa.anchored(cache, self.text.as_bytes(), start)).max();
                match end {
                    Some(end) if end > start => start = end,
                    _ => break,
                }
            }
        }
        let trivia = (start != pos as usize).then(|| TriviaSpan {
            start: pos, end: start as u32,
            line_break: self.program.line_break.find(&mut self.line_cache, &self.text.as_bytes()[pos as usize..start], 0).is_some(),
        });
        let mut token = None;
        for id in allowed {
            if let Some(end) = self.program.tokens[*id as usize].anchored(&mut self.token_caches[*id as usize], self.text.as_bytes(), start) {
                if end > start && token.as_ref().is_none_or(|token: &TokenSpan| end > token.end as usize) {
                    token = Some(TokenSpan { token: *id, start: start as u32, end: end as u32 });
                }
            }
        }
        let lexed = Lexed { start: start as u32, token, trivia };
        self.lexed.insert((goal, pos), lexed.clone());
        lexed
    }

    fn terminal(&mut self, terminal: &Terminal, pos: u32, context: Context) -> Answer {
        let lexed = self.lex(context.goal, pos);
        let expected = || terminal.text.clone().unwrap_or_else(|| self.parser.tokens()[terminal.token as usize].name.clone());
        let token = lexed.token.filter(|token| token.token == terminal.token
            && terminal.text.as_deref().is_none_or(|text| text == &self.text[token.start as usize..token.end as usize]))
            .ok_or_else(|| error(lexed.start, expected()))?;
        let value = self.draft(terminal.emit, token.start, token.end, Vec::new());
        let receipt = self.receipt(Receipt::Token(token.clone(), lexed.trivia));
        Ok(Matched { start: Some(token.start), end: token.end, values: vec![value], fields: Vec::new(), receipt: Some(receipt), cut: false })
    }

    fn memoized(&mut self, expression: usize, pos: u32, context: Context, power: u16) -> Answer {
        let key = (expression, pos, context, power);
        if let Some(answer) = self.memo.get(&key) {
            return answer.clone().unwrap_or_else(|| Err(error(pos, "non-recursive production")));
        }
        self.memo.insert(key, None);
        let answer = self.evaluate(expression, pos, context, power, &[]).map(|mut matched| {
            matched.fields.clear();
            matched.cut = false;
            matched
        });
        self.memo.insert(key, Some(answer.clone()));
        answer
    }

    fn evaluate(&mut self, expression: usize, pos: u32, context: Context, power: u16, scope: &[(u16, Vec<u32>)]) -> Answer {
        // One recursion owner covers productions, lookahead and Pratt recursion.
        if self.depth >= 512 {
            let mut failure = error(pos, "parser nesting limit");
            failure.committed = true;
            return Err(failure);
        }
        self.depth += 1;
        let result = self.execute(expression, pos, context, power, scope);
        self.depth -= 1;
        result
    }

    fn execute(&mut self, expression: usize, pos: u32, context: Context, power: u16, scope: &[(u16, Vec<u32>)]) -> Answer {
        match &self.program.expressions[expression] {
            Op::Token(terminal) => self.terminal(terminal, pos, context),
            Op::Call(production, power) => self.memoized(self.program.productions[*production], pos, context, *power),
            Op::Seq(items) => {
                let mut matched = Matched::empty(pos);
                let mut local = scope.to_vec();
                for item in items {
                    match self.evaluate(*item, matched.end, context, power, &local) {
                        Ok(next) => {
                            local.extend(next.fields.iter().cloned());
                            self.append(&mut matched, next);
                        }
                        Err(mut failure) => {
                            failure.committed |= matched.cut;
                            return Err(failure);
                        }
                    }
                }
                Ok(matched)
            }
            Op::Choice(items) => {
                let mut failure = error(pos, "alternative");
                for item in items {
                    match self.evaluate(*item, pos, context, power, scope) {
                        Ok(mut matched) => {
                            matched.cut = false;
                            return Ok(matched);
                        }
                        Err(error) if error.committed => return Err(error),
                        Err(error) => failure = merge_error(failure, error),
                    }
                }
                Err(failure)
            }
            Op::Repeat { inner, min, max } => {
                let mut matched = Matched::empty(pos);
                let mut count = 0;
                while max.is_none_or(|max| count < max) {
                    match self.evaluate(*inner, matched.end, context, power, scope) {
                        Ok(next) if next.end > matched.end => {
                            self.append(&mut matched, next);
                            count += 1;
                        }
                        Ok(_) => return Err(error(matched.end, "consuming repetition")),
                        Err(failure) if failure.committed || count < *min => return Err(failure),
                        Err(_) => break,
                    }
                }
                Ok(matched)
            }
            Op::Bind(field, inner) => {
                let mut matched = self.evaluate(*inner, pos, context, power, scope)?;
                matched.fields.push((*field, matched.values.clone()));
                Ok(matched)
            }
            Op::Emit(kind, inner) => {
                let mut matched = self.evaluate(*inner, pos, context, power, &[])?;
                let value = self.draft(Some(*kind), matched.start.unwrap_or(pos), matched.end, std::mem::take(&mut matched.fields));
                matched.values = vec![value];
                Ok(matched)
            }
            Op::Context { goal, set, clear, inner } => {
                let context = Context { goal: goal.unwrap_or(context.goal), flags: (context.flags | set) & !clear };
                self.evaluate(*inner, pos, context, power, scope)
            }
            Op::Flag(flag, yes, no) => self.evaluate(if context.flags & flag != 0 { *yes } else { *no }, pos, context, power, scope),
            Op::Look(inner, negative) => match self.evaluate(*inner, pos, context, power, scope) {
                Ok(_) if !negative => Ok(Matched::empty(pos)),
                Err(failure) if !negative => Err(failure),
                Err(_) => Ok(Matched::empty(pos)),
                Ok(_) => Err(error(pos, "negative lookahead")),
            },
            Op::Try(inner) => self.evaluate(*inner, pos, context, power, scope).map_err(|mut failure| {
                failure.committed = false;
                failure
            }),
            Op::Discard(inner) => self.evaluate(*inner, pos, context, power, scope).map(|mut matched| {
                matched.values.clear();
                matched.fields.clear();
                matched
            }),
            Op::Cut => Ok(Matched { cut: true, ..Matched::empty(pos) }),
            Op::SameLine => {
                let next = self.lex(context.goal, pos);
                if next.trivia.is_some_and(|gap| gap.line_break) {
                    Err(error(next.start, "same line"))
                } else {
                    Ok(Matched::empty(pos))
                }
            }
            Op::SameText(left, right) => {
                let left = self.field_path(scope, left);
                let right = self.field_path(scope, right);
                if left.zip(right).is_some_and(|(left, right)| self.text[left.start as usize..left.end as usize] == self.text[right.start as usize..right.end as usize]) {
                    Ok(Matched::empty(pos))
                } else {
                    Err(error(pos, "equal field text"))
                }
            }
            Op::Terminator { explicit, before, newline, eof } => {
                match self.terminal(explicit, pos, context) {
                    Ok(matched) => return Ok(matched),
                    Err(failure) if failure.committed => return Err(failure),
                    Err(_) => {}
                }
                let next = self.lex(context.goal, pos);
                let before = before.iter().any(|terminal| next.token.as_ref().is_some_and(|token| {
                    token.token == terminal.token && terminal.text.as_deref().is_none_or(|text| text == &self.text[token.start as usize..token.end as usize])
                }));
                if before || (*eof && next.start as usize == self.text.len()) || (*newline && next.trivia.is_some_and(|gap| gap.line_break)) {
                    Ok(Matched::empty(pos))
                } else {
                    Err(error(next.start, "statement terminator"))
                }
            }
            Op::End => {
                let next = self.lex(context.goal, pos);
                if next.start as usize == self.text.len() { Ok(Matched::empty(pos)) }
                else { Err(error(next.start, "end of file")) }
            }
            Op::Pratt { atom, prefix, rows } => {
                let mut matched = None;
                for prefix in prefix {
                    match self.evaluate(prefix.operator, pos, context, 0, scope) {
                        Ok(operator) => {
                            let argument = self.memoized(expression, operator.end, context, prefix.power)?;
                            let mut fields = operator.fields.clone();
                            if let Some(field) = prefix.operator_field { fields.push((field, operator.values.clone())); }
                            fields.push((prefix.argument, argument.values.clone()));
                            let start = operator.start.unwrap_or(pos);
                            let end = argument.end;
                            let receipt = self.join([operator.receipt, argument.receipt]);
                            let value = self.draft(Some(prefix.kind), start, end, fields);
                            matched = Some(Matched { start: Some(start), end, values: vec![value], fields: Vec::new(), receipt, cut: false });
                            break;
                        }
                        Err(failure) if failure.committed => return Err(failure),
                        Err(_) => {}
                    }
                }
                let mut matched = match matched {
                    Some(matched) => matched,
                    None => self.evaluate(*atom, pos, context, 0, scope)?,
                };
                loop {
                    let mut next = None;
                    for row in rows {
                        if row.power < power { continue; }
                        match self.evaluate(row.operator, matched.end, context, 0, scope) {
                            Ok(operator) => {
                                let right = if row.operand {
                                    Some(self.memoized(expression, operator.end, context, if row.right_associative { row.power } else { row.power + 1 })?)
                                } else { None };
                                let mut fields = operator.fields.clone();
                                fields.push((row.left, matched.values.clone()));
                                if let Some(field) = row.operator_field { fields.push((field, operator.values.clone())); }
                                if let (Some(field), Some(right)) = (row.right, &right) { fields.push((field, right.values.clone())); }
                                let end = right.as_ref().map_or(operator.end, |right| right.end);
                                let start = matched.start.unwrap_or(pos);
                                let receipt = self.join([matched.receipt, operator.receipt, right.as_ref().and_then(|right| right.receipt)]);
                                let value = self.draft(Some(row.kind), start, end, fields);
                                next = Some(Matched { start: Some(start), end, values: vec![value], fields: Vec::new(), receipt, cut: false });
                                break;
                            }
                            Err(failure) if failure.committed => return Err(failure),
                            Err(_) => {}
                        }
                    }
                    match next {
                        Some(next) if next.end > matched.end => matched = next,
                        Some(_) => return Err(error(matched.end, "consuming precedence operator")),
                        None => return Ok(matched),
                    }
                }
            }
        }
    }

    fn field_path(&self, fields: &[(u16, Vec<u32>)], path: &[u16]) -> Option<&Draft> {
        let (first, rest) = path.split_first()?;
        let (_, values) = fields.iter().rev().find(|(field, _)| field == first)?;
        let mut draft = self.drafts.get(*values.first()? as usize)?;
        for component in rest {
            let (_, values) = draft.fields.iter().find(|(field, _)| field == component)?;
            draft = self.drafts.get(*values.first()? as usize)?;
        }
        Some(draft)
    }

    fn materialize(&self, root: u32, registration: &Registration) -> Result<(Vec<Node>, u32), ParseError> {
        let mut nodes = Vec::new();
        let mut remap = HashMap::new();
        let mut pending = vec![root];
        let mut order = Vec::new();
        while let Some(id) = pending.pop() {
            if remap.contains_key(&id) { continue; }
            let draft = &self.drafts[id as usize];
            remap.insert(id, nodes.len() as u32);
            order.push(id);
            nodes.push(Node {
                matcher: draft.kind.map_or(registration.text_matcher, |kind| registration.matchers[kind as usize]),
                start: draft.start, end: draft.end, children: Vec::new(),
            });
            pending.extend(draft.fields.iter().flat_map(|(_, values)| values.iter().rev()).copied());
        }
        for id in order {
            let draft = &self.drafts[id as usize];
            let Some(kind) = draft.kind else { continue; };
            let schema = &self.program.node_fields[kind as usize];
            let mut children = Vec::new();
            for (field, values) in &draft.fields {
                let local = schema.iter().position(|candidate| candidate == field)
                    .ok_or_else(|| error(draft.start, format!("field `{}` on `{}`", self.program.fields[*field as usize], self.parser.nodes()[kind as usize].name)))?;
                for value in values {
                    children.push(Edge { label: registration.labels[kind as usize][local], node: remap[value] });
                }
            }
            nodes[remap[&id] as usize].children = children;
        }
        Ok((nodes, remap[&root]))
    }
}
