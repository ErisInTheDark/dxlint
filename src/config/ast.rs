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
    pub formats: Vec<FormatDef>,
    pub sets: Vec<SetDef>,
    /// Global `ignore "glob"` items: files never linted.
    pub ignores: Vec<(String, Span)>,
}

/// `lang rust = "*.rs" | "*.rs.in"`
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

/// `[in|over A | B] match name [= pattern]`. Without a pattern it attaches
/// an existing matcher to more parents.
#[derive(Debug)]
pub struct MatcherDef {
    pub name: Name,
    pub parents: Vec<Parent>,
    /// `over`: searched across the parent's text without consuming it.
    pub overlay: bool,
    pub pattern: Option<(Pat, Span)>,
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

/// What a formatter applies to.
#[derive(Clone, Debug)]
pub enum Target {
    /// Every node of a matcher: `on fn`.
    Matcher(Name),
    /// A labelled field of a matcher: `on fn.name`.
    Field(Name, Name),
    /// The whole file of a language: `on .rust`; `on .*` for every language.
    Lang(Name),
}

#[derive(Debug)]
pub struct OptionDef {
    pub name: Name,
    pub default: Expr,
    pub description: Option<String>,
}

#[derive(Debug)]
pub struct FormatDef {
    pub name: Name,
    pub target: Target,
    pub description: Option<Expr>,
    pub severity: Option<Severity>,
    pub options: Vec<OptionDef>,
    pub files: Vec<(String, Span)>,
    pub ignore: Vec<(String, Span)>,
    pub when: Vec<Expr>,
    pub message: Option<Expr>,
    /// What the node should look like. Different text => diagnostic + fix.
    pub shape: Option<Expr>,
}

#[derive(Debug)]
pub enum SetValue {
    Severity(Severity),
    Expr(Expr),
}

/// `set rule.key = value`; `rule` may be a glob like `style-*`.
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
