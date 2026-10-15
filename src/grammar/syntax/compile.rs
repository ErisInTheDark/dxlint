//! Resolves declarations once; execution uses ids, never language names.

mod first;
mod dispatch;
pub(super) mod instructions;
mod lower;
mod memo;
mod inline;
pub(super) use first::Probe;
pub(super) use lower::Action;
pub(super) use dispatch::Dispatch;
pub(super) use inline::ScopeRule;

use std::collections::HashSet;

use crate::pattern::dfa::{Dfa, Lexer};
use crate::pattern::syntax::{Definition, Expr, Precedence, TokenMatch};
use crate::pattern::Pat;

pub(super) struct Program {
    pub expressions: Vec<Op>,
    pub productions: Vec<usize>,
    pub root: usize,
    pub tokens: Vec<Dfa>,
    pub trivia: Vec<Dfa>,
    pub trivia_lexer: Option<Lexer>,
    pub line_break: Dfa,
    pub predicates: Vec<Dfa>,
    pub goals: Vec<(Vec<u16>, bool)>,
    pub lexers: Vec<Option<Lexer>>,
    pub fields: Vec<String>,
    pub public_fields: usize,
    pub node_fields: Vec<Vec<u16>>,
    pub first: Vec<Option<first::First>>,
    pub starts: Vec<Option<first::Start>>,
    pub lowered: Vec<lower::Lowered>,
    pub instructions: instructions::Instructions,
    pub token_first: Vec<crate::pattern::prefilter::ByteSet>,
    pub trivia_first: crate::pattern::prefilter::ByteSet,
    pub goal_changes: Vec<Vec<crate::pattern::prefilter::ByteSet>>,
    pub goal_tokens: Vec<Vec<bool>>,
}

pub(super) enum Op {
    Token(Terminal),
    TokenPattern { predicate: usize, emit: Option<u16> },
    Call(usize, u16),
    Seq(Vec<usize>),
    Choice(Vec<usize>),
    Repeat { inner: usize, min: u32, max: Option<u32> },
    Bind(u16, usize),
    Emit(u16, usize),
    Context { goal: Option<u16>, set: u64, clear: u64, inner: usize },
    Flag(u64, usize, usize),
    Look(usize, bool),
    Try(usize),
    Discard(usize),
    Text(usize),
    Scope { capture: u16, header: usize, body: usize },
    Reuse(u16),
    Cut,
    SameLine,
    SameText(Vec<u16>, Vec<u16>, Option<usize>),
    Terminator { explicit: Terminal, before: Vec<Terminal>, newline: bool, eof: bool },
    End,
    Pratt { atom: usize, prefix: Vec<Prefix>, rows: Vec<Row> },
}

#[derive(Clone)]
pub(super) struct Terminal {
    pub tokens: Vec<u16>,
    pub text: Option<String>,
    pub emit: Option<u16>,
    pub expected: std::sync::Arc<str>,
}

pub(super) struct Prefix {
    pub operator: usize,
    pub power: u16,
    pub kind: u16,
    pub operator_field: Option<u16>,
    pub argument: u16,
}

pub(super) struct Row {
    pub operator: usize,
    pub power: u16,
    pub right_associative: bool,
    pub operand: bool,
    pub kind: u16,
    pub left: u16,
    pub operator_field: Option<u16>,
    pub right: Option<u16>,
    pub flatten: bool,
}

fn index<'a>(names: impl IntoIterator<Item = &'a str>, name: &str, what: &str) -> Result<u16, String> {
    names.into_iter().position(|value| value == name)
        .and_then(|id| u16::try_from(id).ok())
        .ok_or_else(|| format!("unknown {what} `{name}`"))
}

fn unique<'a>(names: impl IntoIterator<Item = &'a str>, what: &str) -> Result<(), String> {
    let mut seen = HashSet::new();
    for name in names {
        if !seen.insert(name) {
            return Err(format!("duplicate {what} `{name}`"));
        }
    }
    Ok(())
}

fn dfa(pattern: &Pat) -> Result<Dfa, String> {
    if pattern.nullable() {
        return Err("tokens and trivia must consume text".into());
    }
    Dfa::new(pattern, &|_| None)
}

impl Program {
    pub fn new(definition: &Definition) -> Result<Self, String> {
        let captures = super::captures::validate(definition)?;
        unique(definition.tokens.iter().map(|token| token.name.as_str()), "token")?;
        unique(definition.goals.iter().map(|goal| goal.name.as_str()), "goal")?;
        unique(definition.nodes.iter().map(|node| node.name.as_str()), "node")?;
        unique(definition.flags.iter().map(String::as_str), "flag")?;
        unique(definition.productions.iter().map(|production| production.name.as_str()), "production")?;
        if definition.flags.len() > 64 || definition.goals.is_empty() {
            return Err("a parser needs lexical goals and at most 64 flags".into());
        }
        let root = index(definition.productions.iter().map(|production| production.name.as_str()), &definition.root, "root production")? as usize;
        let mut fields = Vec::new();
        for node in &definition.nodes {
            unique(node.fields.iter().map(|field| field.name.as_str()), "field")?;
            for field in &node.fields {
                if !fields.contains(&field.name) {
                    fields.push(field.name.clone());
                }
                for kind in &field.kinds {
                    index(definition.nodes.iter().map(|node| node.name.as_str()), kind, "field kind")?;
                }
            }
        }
        for token in &definition.tokens {
            if let Some(kind) = &token.visible_kind {
                Self::kind(definition, kind)?;
            }
        }
        let public_fields = fields.len();
        fields.extend(captures);
        let first_bytes = |pattern: &Pat| {
            let mut set = crate::pattern::prefilter::ByteSet::default();
            crate::pattern::prefilter::first_set(pattern, &|_| crate::pattern::prefilter::ByteSet::full(), &mut set);
            set
        };
        let mut trivia_first = crate::pattern::prefilter::ByteSet::default();
        for pattern in &definition.trivia { trivia_first.union(&first_bytes(pattern)); }
        let mut program = Self {
            expressions: Vec::new(), productions: Vec::new(), root,
            tokens: definition.tokens.iter().map(|token| dfa(&token.pattern)).collect::<Result<_, _>>()?,
            trivia: definition.trivia.iter().map(dfa).collect::<Result<_, _>>()?,
            trivia_lexer: Lexer::new(&definition.trivia.iter().collect::<Vec<_>>()),
            line_break: dfa(&definition.line_break)?,
            predicates: Vec::new(),
            goals: definition.goals.iter().map(|goal| {
                Ok((goal.tokens.iter().map(|name| index(definition.tokens.iter().map(|token| token.name.as_str()), name, "goal token"))
                    .collect::<Result<_, String>>()?, goal.trivia))
            }).collect::<Result<_, String>>()?,
            lexers: Vec::new(), fields, public_fields, node_fields: Vec::new(), first: Vec::new(), starts: Vec::new(), lowered: Vec::new(),
            instructions: instructions::Instructions::default(),
            token_first: definition.tokens.iter().map(|token| first_bytes(&token.pattern)).collect(), trivia_first,
            goal_changes: Vec::new(), goal_tokens: Vec::new(),
        };
        for (tokens, _) in &mut program.goals { tokens.sort_unstable(); tokens.dedup(); }
        program.goal_tokens = program.goals.iter().map(|(tokens, _)| {
            let mut allowed = vec![false; program.tokens.len()];
            for token in tokens { allowed[*token as usize] = true; }
            allowed
        }).collect();
        program.goal_changes = program.goals.iter().map(|(old, _)| program.goals.iter().map(|(new, _)| {
            let mut starts = crate::pattern::prefilter::ByteSet::default();
            for token in new.iter().filter(|token| !old.contains(token)) {
                starts.union(&program.token_first[*token as usize]);
            }
            starts
        }).collect()).collect();
        program.lexers = program.goals.iter().map(|(tokens, _)|
            Lexer::new(&tokens.iter().map(|token| &definition.tokens[*token as usize].pattern).collect::<Vec<_>>())).collect();
        program.node_fields = definition.nodes.iter().map(|node| node.fields.iter().map(|field| program.field(&field.name)).collect())
            .collect::<Result<_, _>>()?;
        for production in &definition.productions {
            let expression = program.expression(definition, &production.body)?;
            program.productions.push(expression);
        }
        let nullable = program.validate()?;
        program.first = first::compile(&program);
        program.starts = first::starts(&program, &program.first);
        program.lowered = lower::compile(&program, &nullable);
        program.instructions = instructions::compile(&program);
        Ok(program)
    }

    fn field(&self, name: &str) -> Result<u16, String> {
        index(self.fields[..self.public_fields].iter().map(String::as_str), name, "field")
    }

    fn capture(&self, name: &str) -> Result<u16, String> {
        let id = index(self.fields[self.public_fields..].iter().map(String::as_str), name, "private capture")? as usize;
        u16::try_from(self.public_fields + id).map_err(|_| "too many parser fields and captures".into())
    }

    fn kind(definition: &Definition, name: &str) -> Result<u16, String> {
        index(definition.nodes.iter().map(|node| node.name.as_str()), name, "node")
    }

    fn terminal(&self, definition: &Definition, token: &TokenMatch) -> Result<Terminal, String> {
        let tokens = if token.token.is_empty() {
            let text = token.text.as_deref().ok_or("a literal needs text")?;
            let tokens: Vec<_> = self.tokens.iter().enumerate().filter_map(|(id, dfa)| {
                (dfa.anchored(&mut dfa.cache(), text.as_bytes(), 0) == Some(text.len())).then_some(id as u16)
            }).collect();
            if tokens.is_empty() {
                return Err(format!("literal `{text}` does not match a declared token"));
            }
            tokens
        } else {
            vec![index(definition.tokens.iter().map(|token| token.name.as_str()), &token.token, "token")?]
        };
        Ok(Terminal {
            expected: token.text.as_deref().unwrap_or(&definition.tokens[tokens[0] as usize].name).into(),
            tokens,
            text: token.text.clone(),
            emit: token.emit.as_deref().map(|kind| Self::kind(definition, kind)).transpose()?,
        })
    }

    fn expressions(&mut self, definition: &Definition, expressions: &[Expr]) -> Result<Vec<usize>, String> {
        expressions.iter().map(|expression| self.expression(definition, expression)).collect()
    }

    fn expression(&mut self, definition: &Definition, expression: &Expr) -> Result<usize, String> {
        let op = match expression {
            Expr::Token(token) => Op::Token(self.terminal(definition, token)?),
            Expr::TokenPattern { pattern, emit } => {
                if pattern.nullable() {
                    return Err("token predicates must consume text".into());
                }
                let predicate = self.predicates.len();
                self.predicates.push(Dfa::whole(pattern, &|_| None)?);
                Op::TokenPattern { predicate, emit: emit.as_deref().map(|kind| Self::kind(definition, kind)).transpose()? }
            }
            Expr::Call { production, min_binding_power } => Op::Call(
                index(definition.productions.iter().map(|production| production.name.as_str()), production, "production")? as usize, *min_binding_power),
            Expr::Seq(items) => Op::Seq(self.expressions(definition, items)?),
            Expr::Choice(items) => Op::Choice(self.expressions(definition, items)?),
            Expr::Repeat { inner, min, max } => {
                if max.is_some_and(|max| max < *min) {
                    return Err("repetition maximum is less than minimum".into());
                }
                Op::Repeat { inner: self.expression(definition, inner)?, min: *min, max: *max }
            }
            Expr::Bind { field, inner } => Op::Bind(self.field(field)?, self.expression(definition, inner)?),
            Expr::Emit { kind, inner } => Op::Emit(Self::kind(definition, kind)?, self.expression(definition, inner)?),
            Expr::Context { change, inner } => {
                let bits = |names: &[String]| -> Result<u64, String> {
                    names.iter().try_fold(0, |bits, name| Ok(bits | (1u64 << index(definition.flags.iter().map(String::as_str), name, "flag")?)))
                };
                Op::Context {
                    goal: change.goal.as_deref().map(|name| index(definition.goals.iter().map(|goal| goal.name.as_str()), name, "goal")).transpose()?,
                    set: bits(&change.set)?, clear: bits(&change.clear)?, inner: self.expression(definition, inner)?,
                }
            }
            Expr::Flag { name, yes, no } => Op::Flag(
                1u64 << index(definition.flags.iter().map(String::as_str), name, "flag")?,
                self.expression(definition, yes)?, self.expression(definition, no)?),
            Expr::Look { inner, negative } => Op::Look(self.expression(definition, inner)?, *negative),
            Expr::Try(inner) => Op::Try(self.expression(definition, inner)?),
            Expr::Discard(inner) => Op::Discard(self.expression(definition, inner)?),
            Expr::Text(inner) => Op::Text(self.expression(definition, inner)?),
            Expr::Scope { capture, header, body } => Op::Scope {
                capture: self.capture(capture)?,
                header: self.expression(definition, header)?, body: self.expression(definition, body)?,
            },
            Expr::Reuse(name) => Op::Reuse(self.capture(name)?),
            Expr::Cut => Op::Cut,
            Expr::SameLine => Op::SameLine,
            Expr::SameText { left, right, ignore } => {
                let ignored = ignore.as_ref().map(|pattern| {
                    if pattern.nullable() {
                        return Err("text comparison ignore patterns must consume text".into());
                    }
                    let predicate = self.predicates.len();
                    self.predicates.push(Dfa::new(pattern, &|_| None)?);
                    Ok::<_, String>(predicate)
                }).transpose()?;
                Op::SameText(
                    left.split('.').map(|name| self.field(name)).collect::<Result<_, _>>()?,
                    right.split('.').map(|name| self.field(name)).collect::<Result<_, _>>()?,
                    ignored,
                )
            }
            Expr::Terminator { explicit, before, newline, eof } => Op::Terminator {
                explicit: self.terminal(definition, explicit)?,
                before: before.iter().map(|token| self.terminal(definition, token)).collect::<Result<_, _>>()?,
                newline: *newline, eof: *eof,
            },
            Expr::End => Op::End,
            Expr::Pratt(precedence) => self.precedence(definition, precedence)?,
        };
        let id = self.expressions.len();
        self.expressions.push(op);
        Ok(id)
    }

    fn precedence(&mut self, definition: &Definition, precedence: &Precedence) -> Result<Op, String> {
        let atom = self.expression(definition, &precedence.atom)?;
        let mut prefix = Vec::new();
        for operator in &precedence.prefix {
            prefix.push(Prefix {
                operator: self.expression(definition, &operator.operator)?, power: operator.binding_power,
                kind: Self::kind(definition, &operator.kind)?,
                operator_field: operator.operator_field.as_deref().map(|name| self.field(name)).transpose()?,
                argument: self.field(&operator.argument)?,
            });
        }
        let mut rows = Vec::new();
        for operator in &precedence.rows {
            if operator.flatten {
                let kind = Self::kind(definition, &operator.kind)?;
                let repeated = definition.nodes[kind as usize].fields.iter()
                    .any(|field| field.name == operator.left && field.repeated);
                if !operator.operand || operator.right.as_ref() != Some(&operator.left)
                    || operator.operator_field.is_some() || !repeated {
                    return Err("flatten needs both operands in one repeated field and no operator field".into());
                }
            }
            if operator.operand && operator.right.is_none() {
                return Err("an operand operator needs a right field".into());
            }
            if !operator.right_associative && operator.binding_power == u16::MAX {
                return Err("left-associative binding power cannot be 65535".into());
            }
            rows.push(Row {
                operator: self.expression(definition, &operator.operator)?, power: operator.binding_power,
                right_associative: operator.right_associative, operand: operator.operand,
                kind: Self::kind(definition, &operator.kind)?, left: self.field(&operator.left)?,
                operator_field: operator.operator_field.as_deref().map(|name| self.field(name)).transpose()?,
                right: operator.right.as_deref().map(|name| self.field(name)).transpose()?,
                flatten: operator.flatten,
            });
        }
        Ok(Op::Pratt { atom, prefix, rows })
    }

    fn validate(&self) -> Result<Vec<bool>, String> {
        let mut nullable = vec![false; self.expressions.len()];
        loop {
            let mut changed = false;
            for (id, op) in self.expressions.iter().enumerate() {
                let yes = match op {
                    Op::Token(_) | Op::TokenPattern { .. } => false,
                    Op::Call(production, _) => nullable[self.productions[*production]],
                    Op::Seq(items) => items.iter().all(|id| nullable[*id]),
                    Op::Choice(items) => items.iter().any(|id| nullable[*id]),
                    Op::Repeat { inner, min, .. } => *min == 0 || nullable[*inner],
                    Op::Bind(_, inner) | Op::Emit(_, inner) | Op::Context { inner, .. } | Op::Try(inner) | Op::Discard(inner) | Op::Text(inner) => nullable[*inner],
                    Op::Flag(_, yes, no) => nullable[*yes] || nullable[*no],
                    Op::Pratt { atom, .. } => nullable[*atom],
                    Op::Scope { header, body, .. } => nullable[*header] && nullable[*body],
                    _ => true,
                };
                if yes && !nullable[id] {
                    nullable[id] = true;
                    changed = true;
                }
            }
            if !changed { break; }
        }
        for (id, expression) in self.productions.iter().enumerate() {
            if id != self.root && nullable[*expression] {
                return Err("ordinary productions must consume text".into());
            }
        }
        for op in &self.expressions {
            match op {
                Op::Text(inner) if nullable[*inner] => return Err("text capture bodies must consume text".into()),
                Op::Repeat { inner, .. } if nullable[*inner] => return Err("repeated expressions must consume text".into()),
                Op::Pratt { prefix, rows, .. } if prefix.iter().any(|prefix| nullable[prefix.operator]) || rows.iter().any(|row| nullable[row.operator]) => {
                    return Err("precedence operators must consume text".into());
                }
                _ => {}
            }
        }
        for production in 0..self.productions.len() {
            let mut visiting = HashSet::new();
            self.leading_calls(self.productions[production], &nullable, &mut visiting)?;
        }
        Ok(nullable)
    }

    fn leading_calls(&self, expression: usize, nullable: &[bool], visiting: &mut HashSet<usize>) -> Result<(), String> {
        if !visiting.insert(expression) {
            return Err("left-recursive productions are not supported".into());
        }
        match &self.expressions[expression] {
            Op::Call(production, _) => self.leading_calls(self.productions[*production], nullable, visiting)?,
            Op::Seq(items) => for id in items {
                self.leading_calls(*id, nullable, visiting)?;
                if !nullable[*id] { break; }
            },
            Op::Choice(items) => for id in items { self.leading_calls(*id, nullable, visiting)?; },
            Op::Flag(_, yes, no) => {
                self.leading_calls(*yes, nullable, visiting)?;
                self.leading_calls(*no, nullable, visiting)?;
            }
            Op::Repeat { inner, .. } | Op::Bind(_, inner) | Op::Emit(_, inner) | Op::Context { inner, .. } |
            Op::Try(inner) | Op::Discard(inner) | Op::Text(inner) | Op::Look(inner, _) => self.leading_calls(*inner, nullable, visiting)?,
            Op::Pratt { atom, prefix, .. } => {
                self.leading_calls(*atom, nullable, visiting)?;
                for operator in prefix { self.leading_calls(operator.operator, nullable, visiting)?; }
            }
            Op::Scope { header, body, .. } => {
                self.leading_calls(*header, nullable, visiting)?;
                if nullable[*header] { self.leading_calls(*body, nullable, visiting)?; }
            }
            _ => {}
        }
        visiting.remove(&expression);
        Ok(())
    }
}
