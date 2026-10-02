//! Everything the TypeScript front end knows about one file, computed on
//! first use: rules that never ask for bindings never pay for them.

use std::cell::OnceCell;
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
}

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
}

pub struct NativeFile<'t> {
    pub syntax: Syntax<'t>,
    pub path: &'t Path,
    pub project: &'t Project,
    bindings: OnceCell<Bindings>,
    typing: OnceCell<Typing<'t>>,
}

impl<'t> NativeFile<'t> {
    pub fn new(syntax: Syntax<'t>, path: &'t Path, project: &'t Project) -> Self {
        NativeFile { syntax, path, project, bindings: OnceCell::new(), typing: OnceCell::new() }
    }

    /// Type state, created on the first type question (summarising this
    /// file for the project).
    pub fn typing(&self) -> &Typing<'t> {
        self.typing.get_or_init(|| {
            let (file, summary) = self.project.summary_for(self.path, &self.syntax, self.syntax.root());
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
            return self.project.export_doc(self.path, &from, &name).flatten();
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
