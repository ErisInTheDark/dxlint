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
    /// `native ts`: native front ends whose node kinds become layers.
    pub natives: Vec<Name>,
    pub attrs: Vec<AttrDef>,
    pub forwards: Vec<ForwardDef>,
    /// Names removed by `drop`: anything still using one is an error at
    /// the `drop`.
    pub dropped: Vec<Dropped>,
    /// Every macro definition, overridden ones included (for tooling).
    pub macros: Vec<MacroInfo>,
    /// Uses only the parser sees (for tooling).
    pub uses: Vec<Use>,
}

/// `macro name(params) body`, as written.
#[derive(Clone, Debug)]
pub struct MacroInfo {
    pub name: Name,
    pub params: Vec<String>,
    /// From `macro` to the end of the body.
    pub span: Span,
}

/// A name the parser resolved itself.
#[derive(Clone, Debug)]
pub enum Use {
    /// A macro call.
    Macro(Name),
    /// A layer named inside a pattern (`name:ident`, `:params`, `paren`).
    Layer(Name),
    /// An import's path string and the file it loaded.
    Import(Span, super::source::FileId),
}

/// What `drop` removed.
#[derive(Clone, Debug)]
pub struct Dropped {
    pub name: String,
    /// The `drop` item.
    pub span: Span,
}

impl Dropped {
    /// The tombstone for `name`, if it was dropped.
    pub fn find<'a>(dropped: &'a [Dropped], name: &str) -> Option<&'a Dropped> {
        dropped.iter().rev().find(|d| d.name == name)
    }

    /// "`name` was dropped, but this still uses it": reported at the `drop`,
    /// pointing at the use.
    pub fn still_used(&self, use_span: impl Into<Option<Span>>) -> crate::config::error::ConfigError {
        crate::config::error::ConfigError::new(self.span, format!("`{}` is dropped here, but it is still used", self.name))
            .related(use_span, "still used here")
            .note("drop or override what uses it too")
    }
}

/// `in a or b let name expr`: a value computed for every node of these
/// layers, readable as `name` on the node and `field.name` from its parent.
#[derive(Debug)]
pub struct AttrDef {
    pub name: Name,
    pub targets: Vec<Name>,
    pub expr: Expr,
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

/// `lang rust "*.rs" "*.rs.in"`
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

impl Parent {
    /// As written: `name`, `.lang` or `.*`.
    pub fn text(&self) -> String {
        match self {
            Parent::Lang(n) => format!(".{}", n.text),
            Parent::AnyLang(_) => ".*".into(),
            Parent::Matcher(n) => n.text.clone(),
        }
    }

    pub fn span(&self) -> Span {
        match self {
            Parent::Lang(n) | Parent::Matcher(n) => n.span,
            Parent::AnyLang(s) => *s,
        }
    }
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

/// `in target rule name [severity [message]] [fix as expr] [{ clauses }]`
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
    /// `fix as expr`: what the node should look like. Different text =>
    /// diagnostic + fix.
    pub fix: Option<Expr>,
    /// `let name expr` clauses: values computed once per node, usable in
    /// the rule's other clauses (and later `let`s).
    pub locals: Vec<(Name, Expr)>,
    /// Where diagnostics point (`report node | change | end`); the whole
    /// node when not given.
    pub report: Option<Report>,
    /// Part of the rule failed to parse: running the rest would check
    /// something else than what was written.
    pub broken: bool,
}

impl RuleDef {
    /// A rule with nothing but its name and targets.
    pub fn new(name: Name, targets: Vec<Target>) -> Self {
        RuleDef {
            name,
            targets,
            severity: None,
            message: None,
            options: Vec::new(),
            files: Vec::new(),
            ignore: Vec::new(),
            when: Vec::new(),
            fix: None,
            locals: Vec::new(),
            report: None,
            broken: false,
        }
    }
}

/// `in a, b forward x, y from field`: these attributes, where the layers do
/// not define them, are read from that field's node (transparent wrappers).
#[derive(Debug)]
pub struct ForwardDef {
    pub targets: Vec<Name>,
    pub attrs: Vec<Name>,
    pub field: Name,
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
    /// A glob some of whose rules were dropped later: matching nothing is
    /// then not a mistake.
    pub soft: bool,
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
    /// Values side by side: text joins as text, lists as lists.
    Join,
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
    /// `layer is a`, `parent is [a, b]`: is the named layer one of these
    /// (or a specialisation of one, or in one of these groups)?
    Is(Box<Expr>, Vec<Name>),
}

#[derive(Debug)]
pub struct Expr {
    pub kind: ExprKind,
    pub span: Span,
}
