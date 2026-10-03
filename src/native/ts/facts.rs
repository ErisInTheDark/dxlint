//! Everything the TypeScript front end knows about one file, computed on
//! first use: rules that never ask for bindings never pay for them.

use std::cell::{OnceCell, RefCell};
use std::collections::HashMap;
use std::path::Path;

use super::bind::{bind, Binding, Bindings};
use super::check::Typing;
use super::docs::leading_doc;
use super::types::Checker;
use super::kinds::{Field, Kind};
use super::modules::Project;
use super::syntax::Syntax;

/// What rule expressions can ask the front end about a node.
pub trait Facts {
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
    fn regex_error(&self, node: u32) -> Option<String>;
    fn regex_problems(&self, node: u32) -> Option<Vec<String>>;
    fn precision_lost(&self, node: u32) -> Option<bool>;
}

pub use super::bind::{MemberUses, HAS_YIELD, USES_ARGUMENTS, USES_THIS};

/// Each type parameter's default, and what the arguments infer for it,
/// printed (`None` where there is none).
pub type TypeParamFacts = (Vec<Option<String>>, Vec<Option<String>>);

impl Facts for NativeFile<'_> {
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
        NativeFile::super_calls(self, node)
    }

    fn regex_error(&self, node: u32) -> Option<String> {
        let (pattern, flags, literal) = self.regex_source(node)?;
        if literal {
            return None;
        }
        super::regex::error(&pattern, &flags)
    }

    fn regex_problems(&self, node: u32) -> Option<Vec<String>> {
        let (pattern, flags, literal) = self.regex_source(node)?;
        Some(super::regex::problems(&pattern, &flags, literal))
    }

    fn precision_lost(&self, node: u32) -> Option<bool> {
        self.syntax.is(node, Kind::Number).then(|| precision_lost(self.syntax.text(node)))
    }
}

/// The literal does not survive conversion to a double (`9007199254740993`,
/// `1.0000000000000001`).
fn precision_lost(text: &str) -> bool {
    let t: String = text.chars().filter(|&c| c != '_').collect();
    let lower = t.to_ascii_lowercase();
    let radix = match lower.get(..2) {
        Some("0x") => Some((16, &t[2..])),
        Some("0o") => Some((8, &t[2..])),
        Some("0b") => Some((2, &t[2..])),
        _ if t.len() > 1 && t.starts_with('0') && t.chars().all(|c| c.is_ascii_digit()) && !t.contains(['8', '9']) => Some((8, &t[1..])),
        _ => None,
    };
    if let Some((radix, digits)) = radix {
        return match u128::from_str_radix(digits, radix) {
            Ok(v) => (v as f64) as u128 != v,
            Err(_) => true,
        };
    }
    let Ok(value) = t.parse::<f64>() else { return false };
    if !value.is_finite() {
        return true;
    }
    let mantissa = lower.split('e').next().unwrap_or("");
    let digits: String = mantissa.chars().filter(char::is_ascii_digit).collect();
    let significant = digits.trim_start_matches('0').trim_end_matches('0');
    if significant.is_empty() {
        return false;
    }
    let precision = significant.len().min(100);
    let printed = format!("{:.*e}", precision - 1, value);
    let back: String = printed.split('e').next().unwrap_or("").chars().filter(char::is_ascii_digit).collect();
    back.trim_end_matches('0') != significant
}

pub struct NativeFile<'t> {
    pub syntax: Syntax<'t>,
    pub path: &'t Path,
    pub project: &'t Project,
    bindings: OnceCell<Bindings>,
    typing: OnceCell<Typing<'t>>,
    /// `(module, name)` of imports -> their declaration's JSDoc.
    import_docs: RefCell<HashMap<(String, String), Option<String>>>,
}

impl<'t> NativeFile<'t> {
    pub fn new(syntax: Syntax<'t>, path: &'t Path, project: &'t Project) -> Self {
        NativeFile { syntax, path, project, bindings: OnceCell::new(), typing: OnceCell::new(), import_docs: RefCell::new(HashMap::new()) }
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

    /// `(pattern, flags, is a literal)` for a regex literal, or for the
    /// string pattern of `RegExp(...)` / `new RegExp(...)` (with a string
    /// flags argument when there is one).
    fn regex_source(&self, node: u32) -> Option<(String, String, bool)> {
        let s = &self.syntax;
        match s.kind(node)? {
            Kind::Regex => {
                let text = s.text(node);
                let close = text.rfind('/')?;
                Some((text[1..close].to_string(), text[close + 1..].to_string(), true))
            }
            Kind::String | Kind::Template if s.field(node) == Some(Field::Arg) => {
                let call = s.parent(node)?;
                let callee = s.get(call, Field::Callee)?;
                if !matches!(s.kind(call), Some(Kind::Call | Kind::New)) || s.text(callee) != "RegExp" || s.all(call, Field::Arg).next() != Some(node) {
                    return None;
                }
                // Only the global `RegExp`.
                if !self.bindings().unresolved.contains(&callee) {
                    return None;
                }
                let pattern = super::regex::cook(s.text(node))?;
                let flags = match s.all(call, Field::Arg).nth(1) {
                    Some(f) if matches!(s.kind(f), Some(Kind::String | Kind::Template)) => super::regex::cook(s.text(f))?,
                    Some(_) => return None,
                    None => String::new(),
                };
                Some((pattern, flags, false))
            }
            _ => None,
        }
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
