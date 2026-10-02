//! Parsed (macro-expanded, import-flattened) configuration.

use super::source::Span;
use crate::pattern::Pat;

#[derive(Clone, Debug)]
pub struct Name {
    pub text: String,
    pub span: Span,
}

#[derive(Default, Debug)]
pub struct Config {
    pub langs: Vec<LangDef>,
    pub matchers: Vec<MatcherDef>,
    pub rules: Vec<RuleDef>,
    pub sets: Vec<SetDef>,
    /// Global `ignore "glob"` items: files never linted.
    pub ignores: Vec<(String, Span)>,
    pub groups: Vec<GroupDef>,
    pub indents: Vec<IndentDef>,
}

/// `in brace or fn.body indent`: lines inside these layers' `until` (or
/// inside these fields) are one level deeper than the line the layer opened on.
#[derive(Debug)]
pub struct IndentDef {
    /// A layer or group, optionally narrowed to one of its fields.
    pub targets: Vec<(Parent, Option<Name>)>,
}

/// `group name a or b`: adds members to a named, open list of layers. Every
/// `group name ...` line adds; `in name match x` reaches all of them.
#[derive(Debug)]
pub struct GroupDef {
    pub name: Name,
    pub parents: Vec<Parent>,
}

/// `lang rust "*.rs" or "*.rs.in"`
#[derive(Debug)]
pub struct LangDef {
    pub name: Name,
    pub globs: Vec<(String, Span)>,
}

#[derive(Clone, Debug)]
pub enum Parent {
    /// `.rust`: the root layer of a language.
    Lang(Name),
    /// `.*`: the root layer of every language.
    AnyLang(Span),
    Matcher(Name),
}

/// `[in|over A or B] match name [pattern]`. Without a pattern it attaches
/// an existing matcher to more parents.
#[derive(Debug)]
pub struct MatcherDef {
    pub name: Name,
    pub parents: Vec<Parent>,
    /// `over`: searched across the parent's text without consuming it.
    pub overlay: bool,
    pub pattern: Option<(Pat, Span)>,
    /// `match name like base ...`: a specialisation of `base`. It gets all
    /// of `base`'s children and overlays, wins ties against it, and counts as
    /// a `base` for rules and ancestor lookups.
    pub like: Option<Name>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Off,
    Hint,
    Info,
    Warn,
    Error,
}

impl Severity {
    pub fn parse(s: &str) -> Option<Severity> {
        Some(match s {
            "off" => Severity::Off,
            "hint" => Severity::Hint,
            "info" => Severity::Info,
            "warn" | "warning" => Severity::Warn,
            "error" => Severity::Error,
            _ => return None,
        })
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Severity::Off => "off",
            Severity::Hint => "hint",
            Severity::Info => "info",
            Severity::Warn => "warn",
            Severity::Error => "error",
        }
    }

    pub const NAMES: &'static [&'static str] = &["off", "hint", "info", "warn", "error"];
}

/// What a rule applies to (the context it is declared in).
#[derive(Clone, Debug)]
pub enum Target {
    /// Every node of a matcher, or of each member of a group: `in fn rule`.
    Matcher(Name),
    /// A labelled field of a matcher: `in fn.name rule`.
    Field(Name, Name),
    /// The whole file of a language: `in .rust rule`; `.*` for every language.
    Lang(Name),
}

#[derive(Debug)]
pub struct OptionDef {
    pub name: Name,
    pub default: Expr,
    pub description: Option<String>,
}

/// `in target rule name [severity [message]] [fix expr] [{ clauses }]`
#[derive(Debug)]
pub struct RuleDef {
    pub name: Name,
    /// `in a or b`: every layer (or field, language, group) it applies to.
    pub targets: Vec<Target>,
    /// The severity keyword (`warn`, ...); the message follows it.
    pub severity: Option<Severity>,
    pub message: Option<Expr>,
    pub options: Vec<OptionDef>,
    pub files: Vec<(String, Span)>,
    pub ignore: Vec<(String, Span)>,
    pub when: Vec<Expr>,
    /// `fix expr`: what the node should look like. Different text =>
    /// diagnostic + fix.
    pub fix: Option<Expr>,
    /// Where diagnostics point (`report node | change | end`).
    pub report: Report,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Report {
    /// The whole node.
    #[default]
    Node,
    /// The part of the node the fix changes.
    Change,
    /// The (empty) end of the node: "missing X after", "too much before X".
    End,
}

#[derive(Debug)]
pub enum SetValue {
    Severity(Severity),
    Expr(Expr),
}

/// `set rule.key value` or `set rule severity`; `rule` may be a glob like
/// `style-*`.
#[derive(Debug)]
pub struct SetDef {
    pub target: Name,
    pub key: Name,
    pub value: SetValue,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BinOp {
    Or,
    And,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    Add,
    Sub,
    Mul,
    Rem,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnOp {
    Not,
    Neg,
}

#[derive(Debug)]
pub enum TemplatePart {
    Lit(String),
    /// A `{...}` hole and its source text.
    Expr(Expr, String),
}

#[derive(Debug)]
pub enum ExprKind {
    Str(String),
    Template(Vec<TemplatePart>),
    Num(f64),
    Bool(bool),
    Null,
    List(Vec<Expr>),
    /// `name` or `layer.field`.
    Var(Vec<Name>),
    Call(Name, Vec<Expr>),
    Unary(UnOp, Box<Expr>),
    Binary(BinOp, Box<Expr>, Box<Expr>),
    If(Box<Expr>, Box<Expr>, Box<Expr>),
    /// `value ~ pattern`: does the whole value match?
    Test(Box<Expr>, Box<Pat>),
}

#[derive(Debug)]
pub struct Expr {
    pub kind: ExprKind,
    pub span: Span,
}
