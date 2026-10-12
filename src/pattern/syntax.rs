//! Language-neutral structured grammars. Language policy lives in declarations.

use super::Pat;

#[derive(Clone, Debug)]
pub struct Definition {
    pub name: String,
    pub trivia: Vec<Pat>,
    pub line_break: Pat,
    pub tokens: Vec<Token>,
    pub goals: Vec<Goal>,
    pub flags: Vec<String>,
    pub nodes: Vec<Node>,
    pub productions: Vec<Production>,
    pub root: String,
}

#[derive(Clone, Debug)]
pub struct Token {
    pub name: String,
    pub pattern: Pat,
    /// Public token kind, when token nodes are requested.
    pub visible_kind: Option<String>,
}

#[derive(Clone, Debug)]
pub struct Goal {
    pub name: String,
    /// Longest match wins; declaration order breaks ties.
    pub tokens: Vec<String>,
    pub trivia: bool,
}

#[derive(Clone, Debug)]
pub struct Node {
    pub name: String,
    pub fields: Vec<Field>,
}

#[derive(Clone, Debug)]
pub struct Field {
    pub name: String,
    pub kinds: Vec<String>,
    pub text: bool,
    pub optional: bool,
    pub repeated: bool,
}

#[derive(Clone, Debug)]
pub struct Production {
    pub name: String,
    pub body: Expr,
}

#[derive(Clone, Debug)]
pub struct TokenMatch {
    pub token: String,
    pub text: Option<String>,
    /// Omitted: plain text, usable as an operator or modifier field.
    pub emit: Option<String>,
}

#[derive(Clone, Debug, Default)]
pub struct ContextChange {
    pub goal: Option<String>,
    pub set: Vec<String>,
    pub clear: Vec<String>,
}

#[derive(Clone, Debug)]
pub enum Expr {
    Token(TokenMatch),
    Call { production: String, min_binding_power: u16 },
    Seq(Vec<Expr>),
    Choice(Vec<Expr>),
    Repeat { inner: Box<Expr>, min: u32, max: Option<u32> },
    Bind { field: String, inner: Box<Expr> },
    Emit { kind: String, inner: Box<Expr> },
    Context { change: ContextChange, inner: Box<Expr> },
    Flag { name: String, yes: Box<Expr>, no: Box<Expr> },
    Look { inner: Box<Expr>, negative: bool },
    Try(Box<Expr>),
    Discard(Box<Expr>),
    Cut,
    SameLine,
    SameText { left: String, right: String },
    Terminator {
        explicit: TokenMatch,
        before: Vec<TokenMatch>,
        newline: bool,
        eof: bool,
    },
    End,
    Pratt(Precedence),
}

#[derive(Clone, Debug)]
pub struct Precedence {
    pub atom: Box<Expr>,
    pub prefix: Vec<Prefix>,
    pub rows: Vec<Operator>,
}

#[derive(Clone, Debug)]
pub struct Prefix {
    pub operator: Expr,
    pub binding_power: u16,
    pub kind: String,
    pub operator_field: Option<String>,
    pub argument: String,
}

#[derive(Clone, Debug)]
pub struct Operator {
    pub operator: Expr,
    pub binding_power: u16,
    pub right_associative: bool,
    /// False for suffixes: arguments and other fields come from `operator`.
    pub operand: bool,
    pub kind: String,
    pub left: String,
    pub operator_field: Option<String>,
    pub right: Option<String>,
}
