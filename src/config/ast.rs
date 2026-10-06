//! Parsed (macro-expanded, import-flattened) configuration.

use super::schema::{Schema, SchemaDef};
use super::source::Span;
use crate::expr::Value;
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
    /// `rule name ...` without a context, in source order.
    pub configs: Vec<RuleConfig>,
    /// Global `ignore "glob"` items: files never linted.
    pub ignores: Vec<(String, Span)>,
    pub groups: Vec<GroupDef>,
    /// `native ts`: native front ends whose node kinds become layers.
    pub natives: Vec<Name>,
    pub attrs: Vec<AttrDef>,
    pub forwards: Vec<ForwardDef>,
    /// `schema name T`: option schemas rules share.
    pub schemas: Vec<SchemaDef>,
    /// Names removed by `drop`: anything still using one is an error at
    /// the `drop`.
    pub dropped: Vec<Dropped>,
    /// Every macro definition, overridden ones included (for tooling).
    pub macros: Vec<MacroInfo>,
    /// `fn name(params) = body`: pure functions rules call.
    pub functions: Vec<FnDef>,
    /// Uses only the parser sees (for tooling).
    pub uses: Vec<Use>,
}

/// `fn name(a, b) = body`: computes a value from its arguments alone.
#[derive(Clone, Debug)]
pub struct FnDef {
    pub name: Name,
    pub params: Vec<Name>,
    pub body: Expr,
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
    /// A named schema in an option's schema.
    Schema(Name),
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
    /// Written inside `define { ... }`: kept only while something needs it.
    pub library: bool,
}

/// A rule's `indent X by n` / `indent in X by n`: the indentation structure
/// its `indented`, `indent_level` and `indent_anchor` read.
#[derive(Clone, Debug)]
pub struct IndentDecl {
    /// `in`: X's inside (its `until`, or its text after its first line),
    /// anchored at X's line; else X itself when it starts its own line,
    /// anchored at its owner's line.
    pub inside: bool,
    /// `dedent`: the levels count back from the anchor.
    pub dedent: bool,
    /// Layers or groups, each maybe narrowed to a field.
    pub targets: Vec<(Parent, Option<Name>)>,
    /// `by n`: levels (default 1), read on X; may use the rule's options.
    pub by: Option<Expr>,
    /// `from offset`: the anchor line is the line of this offset (or
    /// node), read on X, instead of X's (or its owner's) first line.
    pub from: Option<Expr>,
    /// `class name`: levels opened on one line collapse to one within a
    /// class (the unnamed one by default); different classes add up.
    pub class: Option<String>,
}

/// `group name a or b`: adds members to a named, open list of layers. Every
/// `group name ...` line adds; `in name match x` reaches all of them.
#[derive(Debug)]
pub struct GroupDef {
    pub name: Name,
    pub parents: Vec<Parent>,
    pub library: bool,
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

/// `[in A or B [layer L]] match name [pattern]`. Without a pattern it
/// attaches an existing matcher to more parents.
#[derive(Debug)]
pub struct MatcherDef {
    pub name: Name,
    pub parents: Vec<Parent>,
    /// `layer L`: its nodes form (part of) the named layer `L`, a tree of
    /// its own over the same text, found anywhere in the parents' text.
    pub layer: Option<Name>,
    pub pattern: Option<(Pat, Span)>,
    /// `match name like base ...`: a specialisation of `base`. It gets all
    /// of `base`'s children and overlays, wins ties against it, and counts as
    /// a `base` for rules and ancestor lookups.
    pub like: Option<Name>,
    /// Written inside `define { ... }`: a library layer, which only takes
    /// part in parsing while something needs it (see `engine::prune`).
    pub library: bool,
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

/// `option name schema = default "description"`
#[derive(Debug)]
pub struct OptionDef {
    pub name: Name,
    pub schema: Schema,
    pub default: Value<'static>,
    pub default_span: Span,
    pub description: Option<String>,
    /// `default option ...`: the one a `rule name set <value>` line sets.
    pub primary: bool,
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
    /// `fix if cond as expr`: only then is the fix attached; otherwise the
    /// diagnostic is reported without one.
    pub fix_when: Option<Expr>,
    /// What `fix` rewrites: the node, or the whitespace next to it.
    pub fix_target: FixTarget,
    /// `let name expr` clauses: values computed once per node, usable in
    /// the rule's other clauses (and later `let`s).
    pub locals: Vec<(Name, Expr)>,
    /// `indent [in] X by n` lines.
    pub indents: Vec<IndentDecl>,
    /// `indent reset expr`: preserve a winning candidate's physical indent.
    pub indent_reset: Option<Expr>,
    /// Where diagnostics point (`report node | change | end`); the whole
    /// node when not given.
    pub report: Option<Report>,
    /// `report at <offset>`: where the diagnostic starts instead (a byte
    /// offset, such as an ancestor's `offset`).
    pub report_at: Option<Expr>,
    /// `each name in list`: the rule runs once per item, with `name` bound
    /// to it in `when`, the message, `fix` and `report at`.
    pub each: Option<(Name, Expr)>,
    /// `@each`: the list reads only the rule's options.
    pub each_options: bool,
    /// Part of the rule failed to parse: running the rest would check
    /// something else than what was written.
    pub broken: bool,
    /// Defined inside `define { ... }`: off until a `rule name` line turns
    /// it on.
    pub defined_only: bool,
    /// The severity a `rule name severity` line gave it (its definition's
    /// severity stays the default).
    pub configured: Option<Severity>,
    /// Off, and reads library layers nothing else needs: not compiled (see
    /// `engine::prune`), only listed.
    pub inert: bool,
}

impl RuleDef {
    /// On after its configuration.
    pub fn enabled(&self) -> bool {
        match self.configured {
            Some(s) => s != Severity::Off,
            None => !self.defined_only && self.severity.unwrap_or(Severity::Warn) != Severity::Off,
        }
    }
}

impl RuleDef {
    /// A rule with nothing but its name and targets.
    pub fn new(name: Name, targets: Vec<Target>) -> Self {
        RuleDef {
            defined_only: false,
            configured: None,
            inert: false,
            name,
            targets,
            severity: None,
            message: None,
            options: Vec::new(),
            files: Vec::new(),
            ignore: Vec::new(),
            when: Vec::new(),
            fix: None,
            fix_when: None,
            fix_target: FixTarget::Node,
            locals: Vec::new(),
            indents: Vec::new(),
            indent_reset: None,
            report: None,
            report_at: None,
            each: None,
            each_options: false,
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
    pub library: bool,
}

/// `fix as`, `fix space_before as`, `fix space_after as`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum FixTarget {
    #[default]
    Node,
    /// The whitespace right before the node (up to a comment or token).
    SpaceBefore,
    /// The whitespace right after the node.
    SpaceAfter,
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

/// `rule name [severity [message]] [{ set option value; clauses }]`
/// without a context: turns existing rules on (at their default severity
/// unless one is given), sets options and adds clauses. `name` may glob.
#[derive(Debug)]
pub struct RuleConfig {
    /// The name or glob in `patch.name`; the severity, message, `fix` and
    /// `report` replace the rule's own, `when`/`let`/`files`/`ignore` add.
    pub patch: RuleDef,
    /// `set name value` lines, in order.
    pub options: Vec<(Name, Expr)>,
    /// `rule name set <value>`: a value for each matching rule's default
    /// option.
    pub value: Option<Expr>,
    /// Matching nothing is not a mistake: built-in rulesets name rules of
    /// languages that may not be imported, and a glob may have lost its
    /// rules to a later `drop`.
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

#[derive(Clone, Debug)]
pub enum TemplatePart {
    Lit(String),
    /// A `{...}` hole and its source text.
    Expr(Expr, String),
}

#[derive(Clone, Debug)]
pub enum ExprKind {
    Str(String),
    Template(Vec<TemplatePart>),
    Num(f64),
    Bool(bool),
    Null,
    List(Vec<Expr>),
    /// `{ key: value, ... }`
    Record(Vec<(Name, Expr)>),
    /// `x => body` or `(a, b) => body`, as a function argument.
    Lambda(Vec<Name>, Box<Expr>),
    /// `value.key` after a call or parentheses: a record's value.
    Key(Box<Expr>, Name),
    /// `name` or `layer.field`.
    Var(Vec<Name>),
    Call(Name, Vec<Expr>),
    Unary(UnOp, Box<Expr>),
    Binary(BinOp, Box<Expr>, Box<Expr>),
    If(Box<Expr>, Box<Expr>, Box<Expr>),
    /// `value ~ pattern`: does the whole value match?
    Test(Box<Expr>, Box<Pat>),
    /// `is a`, `parent is (a, b)`, `is a.field`, `n is layer lines`,
    /// `is .rust`: does the node pass one of these tests?
    Is(Box<Expr>, Vec<IsName>),
}

/// One alternative of an `is` test.
#[derive(Clone, Debug)]
pub enum IsName {
    /// Of this kind (a specialisation, a group member).
    Kind(Name),
    /// `a.field`: in that field of a node of kind `a`.
    Field(Name, Name),
    /// `layer name`: in that named layer.
    Layer(Name),
    /// `.lang`: in a file of that language.
    Lang(Name),
}

impl IsName {
    /// The kind or group named, if any.
    pub fn kind(&self) -> Option<&Name> {
        match self {
            IsName::Kind(n) | IsName::Field(n, _) => Some(n),
            IsName::Layer(_) | IsName::Lang(_) => None,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Expr {
    pub kind: ExprKind,
    pub span: Span,
}
