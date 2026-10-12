//! Resolves declarations once; execution uses ids, never language names.

use std::collections::HashSet;

use crate::pattern::dfa::Dfa;
use crate::pattern::syntax::{Definition, Expr, Precedence, TokenMatch};
use crate::pattern::Pat;

pub(super) struct Program {
    pub expressions: Vec<Op>,
    pub productions: Vec<usize>,
    pub root: usize,
    pub tokens: Vec<Dfa>,
    pub trivia: Vec<Dfa>,
    pub line_break: Dfa,
    pub goals: Vec<(Vec<u16>, bool)>,
    pub fields: Vec<String>,
    pub node_fields: Vec<Vec<u16>>,
}

pub(super) enum Op {
    Token(Terminal),
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
    Cut,
    SameLine,
    SameText(Vec<u16>, Vec<u16>),
    Terminator { explicit: Terminal, before: Vec<Terminal>, newline: bool, eof: bool },
    End,
    Pratt { atom: usize, prefix: Vec<Prefix>, rows: Vec<Row> },
}

pub(super) struct Terminal {
    pub token: u16,
    pub text: Option<String>,
    pub emit: Option<u16>,
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
        let mut program = Self {
            expressions: Vec::new(), productions: Vec::new(), root,
            tokens: definition.tokens.iter().map(|token| dfa(&token.pattern)).collect::<Result<_, _>>()?,
            trivia: definition.trivia.iter().map(dfa).collect::<Result<_, _>>()?,
            line_break: dfa(&definition.line_break)?,
            goals: definition.goals.iter().map(|goal| {
                Ok((goal.tokens.iter().map(|name| index(definition.tokens.iter().map(|token| token.name.as_str()), name, "goal token"))
                    .collect::<Result<_, String>>()?, goal.trivia))
            }).collect::<Result<_, String>>()?,
            fields, node_fields: Vec::new(),
        };
        program.node_fields = definition.nodes.iter().map(|node| node.fields.iter().map(|field| program.field(&field.name)).collect())
            .collect::<Result<_, _>>()?;
        for production in &definition.productions {
            let expression = program.expression(definition, &production.body)?;
            program.productions.push(expression);
        }
        program.validate()?;
        Ok(program)
    }

    fn field(&self, name: &str) -> Result<u16, String> {
        index(self.fields.iter().map(String::as_str), name, "field")
    }

    fn kind(definition: &Definition, name: &str) -> Result<u16, String> {
        index(definition.nodes.iter().map(|node| node.name.as_str()), name, "node")
    }

    fn terminal(&self, definition: &Definition, token: &TokenMatch) -> Result<Terminal, String> {
        Ok(Terminal {
            token: index(definition.tokens.iter().map(|token| token.name.as_str()), &token.token, "token")?,
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
            Expr::Cut => Op::Cut,
            Expr::SameLine => Op::SameLine,
            Expr::SameText { left, right } => Op::SameText(
                left.split('.').map(|name| self.field(name)).collect::<Result<_, _>>()?,
                right.split('.').map(|name| self.field(name)).collect::<Result<_, _>>()?),
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
            });
        }
        Ok(Op::Pratt { atom, prefix, rows })
    }

    fn validate(&self) -> Result<(), String> {
        let mut nullable = vec![false; self.expressions.len()];
        loop {
            let mut changed = false;
            for (id, op) in self.expressions.iter().enumerate() {
                let yes = match op {
                    Op::Token(_) => false,
                    Op::Call(production, _) => nullable[self.productions[*production]],
                    Op::Seq(items) => items.iter().all(|id| nullable[*id]),
                    Op::Choice(items) => items.iter().any(|id| nullable[*id]),
                    Op::Repeat { inner, min, .. } => *min == 0 || nullable[*inner],
                    Op::Bind(_, inner) | Op::Emit(_, inner) | Op::Context { inner, .. } | Op::Try(inner) | Op::Discard(inner) => nullable[*inner],
                    Op::Flag(_, yes, no) => nullable[*yes] || nullable[*no],
                    Op::Pratt { atom, .. } => nullable[*atom],
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
        Ok(())
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
            Op::Try(inner) | Op::Discard(inner) | Op::Look(inner, _) => self.leading_calls(*inner, nullable, visiting)?,
            Op::Pratt { atom, prefix, .. } => {
                self.leading_calls(*atom, nullable, visiting)?;
                for operator in prefix { self.leading_calls(operator.operator, nullable, visiting)?; }
            }
            _ => {}
        }
        visiting.remove(&expression);
        Ok(())
    }
}
