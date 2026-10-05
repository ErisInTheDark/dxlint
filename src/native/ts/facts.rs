//! Everything the TypeScript front end knows about one file, computed on
//! first use: rules that never ask for bindings never pay for them.

use std::any::{Any, TypeId};
use std::cell::{OnceCell, RefCell};
use std::collections::HashMap;
use std::path::Path;
use std::rc::Rc;

use crate::expr::Value;

use super::bind::{bind, Binding, Bindings};
use super::check::Typing;
use super::comments::{comments, Comment};
use super::docs::leading_doc;
use super::globals::Globals;
use super::paths::{self, Reach, Supers};
use super::types::Checker;
use super::kinds::{Field, Kind};
use super::modules::Project;
use super::syntax::Syntax;

/// What rule expressions can ask the front end about a node.
pub trait Facts {
    /// `f` on the file itself (topic modules' facts).
    fn with_file(&self, f: for<'a, 't> fn(&'a NativeFile<'t>, u32) -> Value<'static>, node: u32) -> Value<'static>;
    /// `f` on the file itself, for links (nodes a node leads to).
    fn nodes_of(&self, f: for<'a, 't> fn(&'a NativeFile<'t>, u32) -> Vec<u32>, node: u32) -> Vec<u32>;
    fn binding(&self, node: u32) -> Option<Binding>;
    fn import_of(&self, node: u32) -> Option<(String, String)>;
    fn decl_doc(&self, node: u32) -> Option<String>;
    fn type_text(&self, node: u32) -> Option<String>;
    fn type_members(&self, node: u32) -> Option<Vec<Box<str>>>;
    fn type_params(&self, node: u32) -> Option<TypeParamFacts>;
    fn type_flags(&self, node: u32) -> Option<Vec<&'static str>>;
    fn type_arg_redundant(&self, node: u32) -> Option<&'static str>;
    /// `local`, `import` or `global` for a referencing identifier.
    fn resolved(&self, node: u32) -> Option<&'static str>;
    /// The kind of the declaration a referencing identifier resolves to.
    fn ref_kind(&self, node: u32) -> Option<&'static str>;
    fn member_uses(&self, node: u32) -> Option<MemberUses>;
    /// [`super::bind::USES_THIS`] and friends, on a function-like node.
    fn function_flags(&self, node: u32) -> Option<u8>;
    /// On a constructor: `all`, `some` or `none` of its paths call `super()`.
    fn super_calls(&self, node: u32) -> Option<&'static str>;
    /// On a member's name: it is never read (`private` / `#` members).
    fn unused_member(&self, node: u32) -> Option<&'static str>;
    /// What code paths say about a node (see the `code_path` fact).
    fn code_path(&self, node: u32) -> Option<&'static str>;
    fn function_name_with_kind(&self, node: u32) -> Option<String>;
    fn enclosing_function_name(&self, node: u32) -> Option<String>;
    fn function_head(&self, node: u32) -> Option<usize>;
    fn return_values(&self, node: u32) -> Option<&'static str>;
    /// On the first statement of a run of unreachable ones: where the run
    /// ends.
    fn unreachable_run_end(&self, node: u32) -> Option<usize>;
    /// What the regex rules find in a regex literal or `RegExp(...)` call.
    fn regex_findings(&self, node: u32) -> Option<Vec<RegexFinding>>;
    /// A regex site's static flags, how its pattern is written, and where.
    fn regex_site(&self, node: u32) -> Option<(Option<String>, &'static str, usize)>;
    /// no-invalid-regexp's pattern error on a `RegExp(...)` call.
    fn regex_pattern_error(&self, node: u32) -> Option<String>;
    /// On a `RegExp(...)` call: its flags as a string literal (`""` without),
    /// `Some(None)` when they are not one.
    fn regex_call_flags(&self, node: u32) -> Option<Option<String>>;
    fn precision_lost(&self, node: u32) -> Option<bool>;
    /// On a reference to a global: `readonly`, `writable` or `off`.
    fn global(&self, node: u32) -> Option<&'static str>;
    /// On a referencing identifier: `read`, `write`, `read_write` or `type`.
    fn access(&self, node: u32) -> Option<&'static str>;
    /// On a written identifier or member: the start offset of the node
    /// that writes it.
    fn write_site(&self, node: u32) -> Option<usize>;
    /// On a property, member or literal: its statically known name / value
    /// as text.
    fn static_name(&self, node: u32) -> Option<String>;
    /// On a function-like node: `get` / `set` for accessors.
    fn accessor(&self, node: u32) -> Option<&'static str>;
    /// Any node: [`Facts::accessor`] of the nearest function around it.
    fn enclosing_accessor(&self, node: u32) -> Option<&'static str>;
    /// On a function-like node: parameter names declared twice.
    fn duplicate_parameters(&self, node: u32) -> Option<Vec<String>>;
    /// On an expression: eslint's `isConstant(node, inBooleanPosition)`.
    fn is_constant(&self, node: u32, in_boolean: bool) -> bool;
    /// A `yield` inside, outside nested functions.
    fn contains_yield(&self, node: u32) -> bool;
    /// On `return`/`throw`/`break`/`continue`: it leaves a `finally`.
    fn leaves_finally(&self, node: u32) -> bool;
    /// The node's start without the parentheses around it.
    fn unparen_offset(&self, node: u32) -> Option<usize>;
    /// no-unexpected-multiline: `(offset, kind)`.
    fn unexpected_multiline(&self, node: u32) -> Option<(usize, &'static str)>;
    /// no-unsafe-optional-chaining: `(offset, arithmetic)` of each risky
    /// optional chain this node uses.
    fn optional_chain_risks(&self, node: u32) -> Vec<(usize, bool)>;
    /// On an assignment: what it assigns to itself, `(offset, text, member)`.
    fn self_assignments(&self, node: u32) -> Option<Vec<(usize, String, bool)>>;
    /// On an `if`: an earlier branch of its chain covers its test.
    fn duplicate_condition(&self, node: u32) -> Option<bool>;
    /// On a `case`: an earlier case of its switch has the same test.
    fn duplicate_case(&self, node: u32) -> Option<bool>;
    /// On a `for`: its update counts away from where its test ends it.
    fn wrong_direction(&self, node: u32) -> Option<bool>;
    /// On a binary or logical expression: no-constant-binary-expression's
    /// finding (`relational` too): `(kind, offset, detail)`.
    fn constant_comparison(&self, node: u32, relational: bool) -> Option<(&'static str, usize, &'static str)>;
    /// On an expression: the global it stands for.
    fn global_value(&self, node: u32) -> Option<String>;
    /// On a declaring identifier: the kinds of all declarations of its name
    /// that merge (`class` + `interface`).
    fn declaration_kinds(&self, node: u32) -> Option<Vec<&'static str>>;
    /// On a literal-like expression: `(type, value as text)`.
    fn literal_value(&self, node: u32) -> Option<(&'static str, String)>;
}

pub use super::bind::{MemberUses, HAS_YIELD, USES_ARGUMENTS, USES_THIS};

/// A regex finding, located in the file (see the `regex_findings` fact).
pub struct RegexFinding {
    pub check: &'static str,
    pub kind: String,
    pub data: Vec<String>,
    pub offset: usize,
    pub end: usize,
}

/// Each type parameter's default, and what the arguments infer for it,
/// printed (`None` where there is none).
pub type TypeParamFacts = (Vec<Option<String>>, Vec<Option<String>>);

impl Facts for NativeFile<'_> {
    fn with_file(&self, f: for<'a, 't> fn(&'a NativeFile<'t>, u32) -> Value<'static>, node: u32) -> Value<'static> {
        f(self, node)
    }

    fn nodes_of(&self, f: for<'a, 't> fn(&'a NativeFile<'t>, u32) -> Vec<u32>, node: u32) -> Vec<u32> {
        f(self, node)
    }

    fn binding(&self, node: u32) -> Option<Binding> {
        NativeFile::binding(self, node)
    }

    fn import_of(&self, node: u32) -> Option<(String, String)> {
        NativeFile::import_of(self, node)
    }

    fn decl_doc(&self, node: u32) -> Option<String> {
        NativeFile::decl_doc(self, node)
    }

    fn type_text(&self, node: u32) -> Option<String> {
        NativeFile::type_text(self, node)
    }

    fn type_members(&self, node: u32) -> Option<Vec<Box<str>>> {
        NativeFile::type_members(self, node)
    }

    fn type_params(&self, node: u32) -> Option<TypeParamFacts> {
        NativeFile::type_params(self, node)
    }

    fn type_flags(&self, node: u32) -> Option<Vec<&'static str>> {
        NativeFile::type_flags(self, node)
    }

    fn type_arg_redundant(&self, node: u32) -> Option<&'static str> {
        NativeFile::type_arg_redundant(self, node)
    }

    fn resolved(&self, node: u32) -> Option<&'static str> {
        let b = self.bindings();
        if b.unresolved.contains(&node) {
            return Some("global");
        }
        let decl = b.refs.get(&node)?;
        Some(if b.decls.get(decl).is_some_and(|d| d.kind == "import") { "import" } else { "local" })
    }

    fn ref_kind(&self, node: u32) -> Option<&'static str> {
        let b = self.bindings();
        Some(b.decls.get(b.refs.get(&node)?)?.kind)
    }

    fn member_uses(&self, node: u32) -> Option<MemberUses> {
        let s = &self.syntax;
        // Class members are keyed by their declaration; parameter
        // properties by the parameter.
        let key = if s.is(node, Kind::Ident) && s.parent(node).is_some_and(|p| s.is(p, Kind::Param)) { s.parent(node)? } else { node };
        if !matches!(s.kind(key), Some(Kind::ClassProp | Kind::Method | Kind::Param)) {
            return None;
        }
        Some(self.bindings().members.get(&key).copied().unwrap_or_default())
    }

    fn function_flags(&self, node: u32) -> Option<u8> {
        matches!(self.syntax.kind(node), Some(Kind::Function | Kind::Method | Kind::Arrow)).then(|| self.bindings().functions.get(&node).copied().unwrap_or(0))
    }

    fn super_calls(&self, node: u32) -> Option<&'static str> {
        self.supers().calls.get(&node).copied()
    }

    fn code_path(&self, node: u32) -> Option<&'static str> {
        let s = &self.syntax;
        let (reach, runs) = self.reach();
        match s.kind(node)? {
            Kind::Function | Kind::Arrow | Kind::Method | Kind::StaticBlock | Kind::Program => {
                Some(if reach.falls_off.get(&node).copied().unwrap_or(false) { "falls_off" } else { "returns" })
            }
            Kind::Case => Some(if reach.unreachable.contains(&node) {
                "unreachable"
            } else if reach.case_falls.contains(&node) {
                "falls_through"
            } else {
                "leaves"
            }),
            Kind::This | Kind::Member | Kind::Index if self.supers().before_super.contains(&node) => Some("before_super"),
            Kind::Call if self.supers().duplicate.contains(&node) => Some("duplicate_super"),
            Kind::Call if self.supers().bad.contains(&node) => Some("bad_super"),
            _ if runs.contains_key(&node) => Some("unreachable_run"),
            _ if reach.unreachable.contains(&node) => Some("unreachable"),
            _ => None,
        }
    }

    fn function_name_with_kind(&self, node: u32) -> Option<String> {
        NativeFile::function_name_with_kind(self, node)
    }

    fn unused_member(&self, node: u32) -> Option<&'static str> {
        let &(_, private, hash) = self.unused_members().get(&node)?;
        Some(if hash {
            "hash"
        } else if private {
            "private"
        } else {
            "public"
        })
    }

    fn enclosing_function_name(&self, node: u32) -> Option<String> {
        NativeFile::function_name_with_kind(self, self.enclosing_function(node)?)
    }

    fn function_head(&self, node: u32) -> Option<usize> {
        NativeFile::function_head(self, node)
    }

    fn return_values(&self, node: u32) -> Option<&'static str> {
        matches!(self.syntax.kind(node), Some(Kind::Function | Kind::Arrow | Kind::Method)).then(|| NativeFile::return_values(self, node)).flatten()
    }

    fn unreachable_run_end(&self, node: u32) -> Option<usize> {
        self.reach().1.get(&node).map(|&e| e as usize)
    }

    fn regex_findings(&self, node: u32) -> Option<Vec<RegexFinding>> {
        Some(
            NativeFile::regex_findings(self, node)?
                .into_iter()
                .map(|l| RegexFinding { check: l.finding.check, kind: l.finding.kind, data: l.finding.data, offset: l.offset, end: l.end })
                .collect(),
        )
    }

    fn regex_site(&self, node: u32) -> Option<(Option<String>, &'static str, usize)> {
        let site = NativeFile::regex_site(self, node)?;
        Some((site.flags, site.pattern_kind, self.syntax.tree.node(site.pattern_node).start as usize))
    }

    fn regex_pattern_error(&self, node: u32) -> Option<String> {
        NativeFile::regex_pattern_error(self, node)
    }

    fn regex_call_flags(&self, node: u32) -> Option<Option<String>> {
        let s = &self.syntax;
        (matches!(s.kind(node), Some(Kind::Call | Kind::New)) && self.global_value(s.get(node, Field::Callee)?).as_deref() == Some("RegExp")).then(|| self.regex_flags(node))
    }

    fn precision_lost(&self, node: u32) -> Option<bool> {
        self.syntax.is(node, Kind::Number).then(|| precision_lost(self.syntax.text(node)))
    }

    fn global(&self, node: u32) -> Option<&'static str> {
        self.global_setting(node)
    }

    fn access(&self, node: u32) -> Option<&'static str> {
        self.bindings().access.get(&node).copied()
    }

    fn accessor(&self, node: u32) -> Option<&'static str> {
        NativeFile::accessor(self, node)
    }

    fn enclosing_accessor(&self, node: u32) -> Option<&'static str> {
        NativeFile::accessor(self, self.enclosing_function(node)?)
    }

    fn duplicate_parameters(&self, node: u32) -> Option<Vec<String>> {
        matches!(self.syntax.kind(node), Some(Kind::Function | Kind::Arrow | Kind::Method)).then(|| NativeFile::duplicate_parameters(self, node))
    }

    fn global_value(&self, node: u32) -> Option<String> {
        NativeFile::global_value(self, node)
    }

    fn is_constant(&self, node: u32, in_boolean: bool) -> bool {
        NativeFile::is_constant(self, node, in_boolean)
    }

    fn contains_yield(&self, node: u32) -> bool {
        NativeFile::contains_yield(self, node)
    }

    fn leaves_finally(&self, node: u32) -> bool {
        matches!(self.syntax.kind(node), Some(Kind::Return | Kind::Throw | Kind::Break | Kind::Continue)) && NativeFile::leaves_finally(self, node)
    }

    fn unparen_offset(&self, node: u32) -> Option<usize> {
        self.syntax.kind(node)?;
        Some(self.syntax.tree.node(self.syntax.unparen(node)).start as usize)
    }

    fn unexpected_multiline(&self, node: u32) -> Option<(usize, &'static str)> {
        NativeFile::unexpected_multiline(self, node)
    }

    fn optional_chain_risks(&self, node: u32) -> Vec<(usize, bool)> {
        NativeFile::optional_chain_risks(self, node).into_iter().map(|(n, a)| (self.syntax.tree.node(n).start as usize, a)).collect()
    }

    fn self_assignments(&self, node: u32) -> Option<Vec<(usize, String, bool)>> {
        let s = &self.syntax;
        s.is(node, Kind::Assign).then(|| {
            NativeFile::self_assignments(self, node)
                .into_iter()
                .map(|(n, member)| (s.tree.node(n).start as usize, s.text(n).split_whitespace().collect::<String>(), member))
                .collect()
        })
    }

    fn duplicate_condition(&self, node: u32) -> Option<bool> {
        self.syntax.is(node, Kind::If).then(|| self.duplicate_else_if(node))
    }

    fn duplicate_case(&self, node: u32) -> Option<bool> {
        self.syntax.is(node, Kind::Case).then(|| NativeFile::duplicate_case(self, node))
    }

    fn wrong_direction(&self, node: u32) -> Option<bool> {
        self.syntax.is(node, Kind::For).then(|| self.for_wrong_direction(node))
    }

    fn constant_comparison(&self, node: u32, relational: bool) -> Option<(&'static str, usize, &'static str)> {
        let c = NativeFile::constant_comparison(self, node, relational)?;
        Some((c.kind, self.syntax.tree.node(c.node).start as usize, c.detail))
    }

    fn declaration_kinds(&self, node: u32) -> Option<Vec<&'static str>> {
        self.bindings().declaration_kinds.get(&node).cloned()
    }

    fn literal_value(&self, node: u32) -> Option<(&'static str, String)> {
        super::statics::literal_value(&self.syntax, node)
    }

    fn static_name(&self, node: u32) -> Option<String> {
        super::statics::property_name(&self.syntax, node).or_else(|| super::statics::literal_text(&self.syntax, node))
    }

    fn write_site(&self, node: u32) -> Option<usize> {
        let site = *self.bindings().write_sites.get(&node)?;
        Some(self.syntax.tree.node(site).start as usize)
    }
}

/// eslint's no-loss-of-precision: the literal does not survive conversion
/// to a double (`9007199254740993`, `1.0000000000000001`, and digits
/// written past what a double holds: `1230000000000000000000000.0`).
fn precision_lost(text: &str) -> bool {
    let raw: String = text.chars().filter(|&c| c != '_').collect();
    let upper = raw.to_ascii_uppercase();
    let legacy_octal = raw.len() > 1 && raw.starts_with('0') && raw[1..].chars().all(|c| ('0'..='7').contains(&c));
    let base = match upper.get(..2) {
        Some("0X") => Some((16, 2)),
        Some("0O") => Some((8, 2)),
        Some("0B") => Some((2, 2)),
        _ if legacy_octal => Some((8, 1)),
        _ => None,
    };
    let Some(value) = super::statics::number(&raw) else { return false };
    if let Some((radix, skip)) = base {
        // The written digits must end with the stored value's digits.
        if value >= 2f64.powi(128) {
            return true;
        }
        let stored = to_radix(value as u128, radix);
        return !(upper[skip..].trim_start_matches('0').ends_with(&stored) || stored == "0" && upper[skip..].chars().all(|c| c == '0'));
    }
    // A `.` right before the exponent or the end changes nothing.
    let lower = raw.to_ascii_lowercase();
    let lower = match lower.find('.') {
        Some(i) if lower[i + 1..].is_empty() || lower[i + 1..].starts_with('e') => format!("{}{}", &lower[..i], &lower[i + 1..]),
        _ => lower,
    };
    let (coefficient, magnitude) = scientific(&lower, false);
    if value == 0.0 {
        return !coefficient.chars().all(|c| c == '0');
    }
    let precision = coefficient.len();
    if precision > 100 {
        return true;
    }
    let stored = format!("{:.*e}", precision - 1, value);
    let (stored_coefficient, stored_magnitude) = scientific(&stored, true);
    magnitude != stored_magnitude || coefficient != stored_coefficient
}

/// eslint's `convertNumberToScientificNotation`: significant digits and
/// the power of ten of the first.
fn scientific(number: &str, as_float: bool) -> (String, i64) {
    let (coefficient, exponent) = match number.split_once('e') {
        Some((c, e)) => (c, e.trim_start_matches('+').parse::<i64>().unwrap_or(0)),
        None => (number, 0),
    };
    let leading = |s: &str| -> String {
        let t = s.trim_start_matches('0');
        if t.is_empty() {
            s.to_string()
        } else {
            t.to_string()
        }
    };
    let (digits, magnitude) = if as_float || number.contains('.') {
        let trimmed = leading(coefficient);
        match trimmed.find('.') {
            Some(0) => {
                let significant = leading(&trimmed[1..]);
                let m = significant.len() as i64 - trimmed.len() as i64;
                (significant, m)
            }
            None => (trimmed.clone(), trimmed.len() as i64 - 1),
            Some(i) => (trimmed.replacen('.', "", 1), i as i64 - 1),
        }
    } else {
        let trimmed = leading(coefficient);
        let significant = {
            let t = trimmed.trim_end_matches('0');
            if t.is_empty() {
                trimmed.clone()
            } else {
                t.to_string()
            }
        };
        (significant, trimmed.len() as i64 - 1)
    };
    (digits, magnitude + exponent)
}

fn to_radix(mut v: u128, radix: u32) -> String {
    if v == 0 {
        return "0".into();
    }
    let mut out = Vec::new();
    while v > 0 {
        out.push(std::char::from_digit((v % radix as u128) as u32, radix).unwrap_or('0').to_ascii_uppercase());
        v /= radix as u128;
    }
    out.iter().rev().collect()
}

pub struct NativeFile<'t> {
    pub syntax: Syntax<'t>,
    pub path: &'t Path,
    pub project: &'t Project,
    bindings: OnceCell<Bindings>,
    comments: OnceCell<Vec<Comment>>,
    globals: OnceCell<Globals>,
    reach: OnceCell<(Reach, HashMap<u32, u32>)>,
    supers: OnceCell<Supers>,
    pub(super) unused_members: OnceCell<HashMap<u32, (String, bool, bool)>>,
    /// Topic modules' per-file results ([`NativeFile::cached`]).
    topic_cache: RefCell<crate::fx_hash::FxHashMap<TypeId, Rc<dyn Any>>>,
    typing: OnceCell<Typing<'t>>,
    /// `(module, name)` of imports -> their declaration's JSDoc.
    import_docs: RefCell<HashMap<(String, String), Option<String>>>,
}

impl<'t> NativeFile<'t> {
    pub fn new(syntax: Syntax<'t>, path: &'t Path, project: &'t Project) -> Self {
        NativeFile {
            syntax,
            path,
            project,
            bindings: OnceCell::new(),
            comments: OnceCell::new(),
            globals: OnceCell::new(),
            reach: OnceCell::new(),
            supers: OnceCell::new(),
            unused_members: OnceCell::new(),
            topic_cache: RefCell::default(),
            typing: OnceCell::new(),
            import_docs: RefCell::new(HashMap::new()),
        }
    }

    /// Reuse the complete stream already scanned for token attachment.
    pub(crate) fn with_tokens(mut self, tokens: Option<super::tokens::Lexed>) -> Self {
        if let Some(tokens) = tokens {
            self.topic_cache.get_mut().insert(TypeId::of::<super::tokens::Lexed>(), Rc::new(tokens));
        }
        self
    }

    /// A topic module's whole-file result of type `T`, computed on first use
    /// (one per type: give each analysis its own result type).
    pub fn cached<T: 'static>(&self, init: impl FnOnce(&Self) -> T) -> Rc<T> {
        let key = TypeId::of::<T>();
        if let Some(v) = self.topic_cache.borrow().get(&key) {
            return v.clone().downcast::<T>().expect("keyed by type");
        }
        let v: Rc<T> = Rc::new(init(self));
        self.topic_cache.borrow_mut().insert(key, v.clone());
        v
    }

    /// eslint's `sourceType`: `commonjs` for `.cjs`/`.cts`, `module` for
    /// `.mjs`/`.mts`; other files are modules when they import or export
    /// (as TypeScript decides), else scripts.
    pub fn source_type(&self) -> &'static str {
        let s = &self.syntax;
        match self.path.extension().and_then(|e| e.to_str()) {
            Some("cjs" | "cts") => "commonjs",
            Some("mjs" | "mts") => "module",
            _ if s.all(s.root(), Field::Body).any(|n| matches!(s.kind(n), Some(Kind::Import | Kind::Export | Kind::ExportAssign | Kind::ImportEquals))) => "module",
            _ => "script",
        }
    }

    pub fn comments(&self) -> &[Comment] {
        self.comments.get_or_init(|| comments(&self.syntax))
    }

    pub fn globals(&self) -> &Globals {
        self.globals.get_or_init(|| Globals::new(self.syntax.src, self.comments()))
    }

    /// Reachability, and no-unreachable's runs (first statement -> end).
    pub fn reach(&self) -> &(Reach, HashMap<u32, u32>) {
        self.reach.get_or_init(|| {
            let reach = paths::reach(&self.syntax);
            let runs = paths::unreachable_runs(&self.syntax, &reach, self.comments());
            (reach, runs)
        })
    }

    pub fn supers(&self) -> &Supers {
        self.supers.get_or_init(|| paths::supers(&self.syntax))
    }

    /// On an identifier that refers to a global: how it is configured
    /// (`readonly`, `writable`, `off`), or `None` when nothing declares it.
    pub fn global_setting(&self, node: u32) -> Option<&'static str> {
        if !self.bindings().unresolved.contains(&node) {
            return None;
        }
        self.globals().setting(self.syntax.text(node))
    }

    /// Type state, created on the first type question (summarising this
    /// file for the project).
    pub fn typing(&self) -> &Typing<'t> {
        self.typing.get_or_init(|| {
            let (file, summary) = self.project.summary_for(self.path, self.syntax.src, &self.syntax, self.syntax.root());
            Typing::new(Checker::new(self.project, file, summary))
        })
    }

    /// The type of expression `node` as text, if known.
    pub fn type_text(&self, node: u32) -> Option<String> {
        let t = self.type_of(node);
        (!t.is_unknown()).then(|| self.typing().checker.print(&t))
    }

    /// Member names of the type of `node` (any constituent's, for unions).
    pub fn type_members(&self, node: u32) -> Option<Vec<Box<str>>> {
        let t = self.type_of(node);
        if t.is_unknown() {
            return None;
        }
        let mut out = Vec::new();
        self.typing().checker.member_names(&t, &mut out);
        Some(out)
    }

    pub fn bindings(&self) -> &Bindings {
        self.bindings.get_or_init(|| bind(&self.syntax))
    }

    /// Use counts of the binding `node` declares (`None` when it declares
    /// nothing).
    pub fn binding(&self, node: u32) -> Option<Binding> {
        self.bindings().decls.get(&node).copied()
    }

    /// The declaring identifier of `node`: itself, or what it refers to.
    pub fn decl_of(&self, node: u32) -> Option<u32> {
        let b = self.bindings();
        if b.decls.contains_key(&node) {
            return Some(node);
        }
        b.refs.get(&node).copied()
    }

    /// `(module specifier, imported name)` when `node` is or refers to an
    /// imported binding (`*` for a namespace import).
    pub fn import_of(&self, node: u32) -> Option<(String, String)> {
        let s = &self.syntax;
        let decl = self.decl_of(node)?;
        if self.binding(decl)?.kind != "import" {
            return None;
        }
        let parent = s.parent(decl)?;
        let (import, name) = match s.kind(parent)? {
            Kind::ImportSpec => (s.parent(parent)?, s.text(s.get(parent, Field::Imported)?).to_string()),
            Kind::Import if s.field(decl) == Some(Field::DefaultImport) => (parent, "default".into()),
            Kind::Import if s.field(decl) == Some(Field::Namespace) => (parent, "*".into()),
            _ => return None,
        };
        let from = s.text(s.get(import, Field::From)?);
        let from = from.get(1..from.len().saturating_sub(1)).unwrap_or(from);
        let name = name.trim_matches(|c| c == '"' || c == '\'').to_string();
        Some((from.to_string(), name))
    }

    /// The JSDoc of the declaration `node` is or refers to, following
    /// imports into other files.
    pub fn decl_doc(&self, node: u32) -> Option<String> {
        if let Some((from, name)) = self.import_of(node) {
            if name == "*" {
                return None;
            }
            // Every use of an import asks the same question.
            let key = (from, name);
            if let Some(doc) = self.import_docs.borrow().get(&key) {
                return doc.clone();
            }
            let doc = self.project.export_doc(self.path, &key.0, &key.1).flatten();
            self.import_docs.borrow_mut().insert(key, doc.clone());
            return doc;
        }
        let s = &self.syntax;
        let mut at = self.decl_of(node)?;
        // Up to the declaring statement (and its `export`).
        while let Some(p) = s.parent(at) {
            at = p;
            if matches!(
                s.kind(at),
                Some(Kind::Var | Kind::Function | Kind::Class | Kind::Interface | Kind::TypeAlias | Kind::Enum | Kind::Namespace | Kind::Param)
            ) {
                break;
            }
        }
        if s.parent(at).is_some_and(|p| s.is(p, Kind::Export)) {
            at = s.parent(at)?;
        }
        leading_doc(s.src, s.tree.node(at).start as usize)
    }
}
