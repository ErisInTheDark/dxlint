//! Everything the TypeScript front end knows about one file, computed on
//! first use: rules that never ask for bindings never pay for them.

use std::any::{Any, TypeId};
use std::cell::{OnceCell, RefCell};
use std::collections::HashMap;
use std::path::Path;
use std::rc::Rc;
use std::sync::Arc;

use crate::expr::Value;

use super::bind::{Binding, Bindings};
use super::check::Typing;
use super::comments::{comments, Comment};
use super::docs::leading_doc;
use super::globals::Globals;
use super::paths::{self, Reach};
use super::types::Checker;
use super::kinds::{Field, Kind};
use super::modules::Project;
use super::syntax::Syntax;

/// What rule expressions can ask the front end about a node.
pub trait Facts {
    /// `f` on the file itself (topic modules' facts).
    fn with_file(&self, f: for<'a, 't> fn(&'a NativeFile<'t>, u32) -> Value<'static>, node: u32) -> Value<'static>;
    /// A projected record field, or presence (`None`), at its topic owner.
    fn with_field(&self, f: for<'a, 't> fn(&'a NativeFile<'t>, u32, Option<usize>) -> Value<'static>, node: u32, field: Option<usize>) -> Value<'static>;
    /// `f` on the file itself, for links (nodes a node leads to).
    fn nodes_of(&self, f: for<'a, 't> fn(&'a NativeFile<'t>, u32) -> Vec<u32>, node: u32) -> Vec<u32>;
    fn binding(&self, node: u32) -> Option<Binding>;
    fn type_text(&self, node: u32) -> Option<String>;
    fn type_members(&self, node: u32) -> Option<Vec<Box<str>>>;
    fn type_defaults(&self, node: u32) -> Option<Vec<Option<String>>>;
    fn type_flags(&self, node: u32) -> Option<Vec<&'static str>>;
    fn type_arg_redundant(&self, node: u32) -> Option<&'static str>;
    /// `local`, `import` or `global` for a referencing identifier.
    fn resolved(&self, node: u32) -> Option<&'static str>;
    fn member_uses(&self, node: u32) -> Option<MemberUses>;
    /// On a property, member or literal: its statically known name / value
    /// as text.
    fn static_name(&self, node: u32) -> Option<String>;
}


pub use super::class_members::MemberUses;

impl Facts for NativeFile<'_> {
    fn with_file(&self, f: for<'a, 't> fn(&'a NativeFile<'t>, u32) -> Value<'static>, node: u32) -> Value<'static> {
        f(self, node)
    }

    fn with_field(&self, f: for<'a, 't> fn(&'a NativeFile<'t>, u32, Option<usize>) -> Value<'static>, node: u32, field: Option<usize>) -> Value<'static> {
        f(self, node, field)
    }

    fn nodes_of(&self, f: for<'a, 't> fn(&'a NativeFile<'t>, u32) -> Vec<u32>, node: u32) -> Vec<u32> {
        f(self, node)
    }

    fn binding(&self, node: u32) -> Option<Binding> {
        NativeFile::binding(self, node)
    }

    fn type_text(&self, node: u32) -> Option<String> {
        NativeFile::type_text(self, node)
    }

    fn type_members(&self, node: u32) -> Option<Vec<Box<str>>> {
        NativeFile::type_members(self, node)
    }

    fn type_defaults(&self, node: u32) -> Option<Vec<Option<String>>> {
        NativeFile::type_defaults(self, node)
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
        if b.arguments_owner(&self.syntax, node).is_some() {
            return Some("local");
        }
        let decl = b.refs.get(&node)?;
        Some(if b.decls.get(decl).is_some_and(|d| d.kind == "import") { "import" } else { "local" })
    }

    fn member_uses(&self, node: u32) -> Option<MemberUses> {
        super::class_members::member_uses(self, node)
    }


    fn static_name(&self, node: u32) -> Option<String> {
        super::statics::property_name(&self.syntax, node).or_else(|| super::statics::literal_text(&self.syntax, node))
    }

}

pub struct NativeFile<'t> {
    pub syntax: Syntax<'t>,
    pub path: &'t Path,
    pub project: &'t Project,
    pub(super) environment: super::environment::Environment,
    /// The engine's resolved names ([`crate::engine::frontend::Native::set_names`]).
    names: OnceCell<Arc<crate::grammar::names::Resolved>>,
    bindings: OnceCell<Bindings>,
    comments: OnceCell<Vec<Comment>>,
    globals: OnceCell<Globals>,
    reach: OnceCell<Reach>,
    /// Topic modules' per-file results ([`NativeFile::cached`]).
    topic_cache: RefCell<crate::fx_hash::FxHashMap<TypeId, Rc<dyn Any>>>,
    typing: OnceCell<Typing<'t>>,
    /// `(module, name)` of imports -> their declaration's JSDoc.
    import_docs: RefCell<HashMap<(String, String), Option<String>>>,
}

impl<'t> NativeFile<'t> {
    pub(crate) fn access(&self, node: u32) -> Option<&'static str> {
        self.bindings().access.get(&node).copied()
    }

    pub fn new(syntax: Syntax<'t>, path: &'t Path, project: &'t Project) -> Self {
        NativeFile {
            syntax,
            path,
            project,
            environment: Default::default(),
            names: OnceCell::new(),
            bindings: OnceCell::new(),
            comments: OnceCell::new(),
            globals: OnceCell::new(),
            reach: OnceCell::new(),
            topic_cache: RefCell::default(),
            typing: OnceCell::new(),
            import_docs: RefCell::new(HashMap::new()),
        }
    }

    pub(crate) fn with_environment(mut self, environment: super::environment::Environment) -> Self {
        self.environment = environment;
        self
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
        if let Some(source_type) = self.environment.source_type {
            return source_type;
        }
        let s = &self.syntax;
        match self.path.extension().and_then(|e| e.to_str()) {
            Some("cjs" | "cts") => "commonjs",
            Some("mjs" | "mts") => "module",
            _ if s.all(s.root(), Field::Body).any(|node| super::view::external_module_statement(s, node)) => "module",
            _ => "script",
        }
    }

    pub fn comments(&self) -> &[Comment] {
        self.comments.get_or_init(|| comments(&self.syntax))
    }

    pub fn globals(&self) -> &Globals {
        self.globals.get_or_init(|| Globals::new(self.syntax.src, self.comments(), &self.environment, self.source_type()))
    }

    pub fn reach(&self) -> &Reach {
        self.reach.get_or_init(|| paths::reach(&self.syntax))
    }

    /// On an identifier that refers to a global: how it is configured
    /// (`readonly`, `writable`, `off`), or `None` when nothing declares it.
    pub fn global_setting(&self, node: u32) -> Option<&'static str> {
        if !self.bindings().unresolved.contains(&node) {
            return None;
        }
        let name = self.syntax.text(node);
        if name.contains('\\') {
            self.globals().setting(&super::statics::identifier_name(name))
        } else {
            self.globals().setting(name)
        }
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

    /// The file's names: the engine gives them before the first fact read
    /// (name conditions may not read name facts).
    pub fn bindings(&self) -> &Bindings {
        self.bindings.get_or_init(|| {
            let names = self.names.get().expect("the engine resolves a file's names before its facts read them");
            Bindings::from_names(&self.syntax, names.clone())
        })
    }

    pub(super) fn set_names(&self, names: Arc<crate::grammar::names::Resolved>) {
        let _ = self.names.set(names);
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

#[cfg(test)]
mod tests {
    use crate::engine::tests::with_native_file;

    #[test]
    fn escaped_global_references_keep_configured_global_settings() {
        with_native_file(r"void /*@*/\u004fbject;", |file, nodes| {
            assert_eq!(file.global_setting(nodes[0]), file.globals().setting("Object"));
            assert!(file.global_setting(nodes[0]).is_some());
        });
    }

    #[test]
    fn source_type_matches_external_import_alias_syntax() {
        for (source, expected) in [
            ("import Alias = Local.Member; declare namespace Local { class Member {} }", "script"),
            ("import Alias = require('module');", "module"),
            ("export {};", "module"),
            ("void import('module');", "script"),
        ] {
            with_native_file(source, |file, _| assert_eq!(file.source_type(), expected, "{source}"));
        }
    }
}
