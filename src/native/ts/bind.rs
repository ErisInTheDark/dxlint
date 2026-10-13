//! Names for one file, as the TypeScript front end's consumers read them.
//! The engine resolves scopes, declarations and references from the
//! language's name items (`std/ts/names.zlint`, docs/design/type-layers.md);
//! this view adds what TypeScript code asks about bindings: declaration
//! kinds, use counts split by whether a use needs the runtime value
//! (`typeof x` and computed keys inside types do not), and write sites.

// Keyed by node ids, built for every file: the fast hasher.
use crate::fx_hash::{FxHashMap as HashMap, FxHashSet as HashSet};
use crate::grammar::names::Resolved;
use std::borrow::Cow;
use std::sync::Arc;

use super::kinds::{Field as F, Kind as K};
use super::syntax::Syntax;

/// A name's text as TypeScript means it: escapes decoded.
pub fn decoded_name(text: &str) -> Cow<'_, str> {
    if text.contains('\\') {
        Cow::Owned(super::statics::identifier_name(text))
    } else {
        Cow::Borrowed(text)
    }
}

#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub struct Binding {
    /// What declares it: `var`, `let`, `const`, `using`, `function`,
    /// `class`, `parameter`, `catch`, `import`, `type`, `interface`, `enum`,
    /// `namespace`, `type_parameter`, or `function_name`/`class_name` for
    /// the names of function and class expressions.
    pub kind: &'static str,
    /// Declared by an `export`ed statement.
    pub exported: bool,
    /// Inside `declare` (or a `declare`d namespace): no runtime code.
    pub ambient: bool,
    /// References of any kind (reads, type uses, `typeof`), writes aside.
    pub uses: u32,
    /// References that need the runtime value.
    pub value_uses: u32,
    /// Uses from inside the declaration itself (a function calling itself).
    pub self_uses: u32,
    pub writes: u32,
}

#[derive(Default)]
pub struct Bindings {
    names: Arc<Resolved>,
    value: u64,
    type_: u64,
    /// Declaring identifier -> its uses.
    pub decls: HashMap<u32, Binding>,
    /// Referencing identifier -> the identifier that declares it.
    pub refs: HashMap<u32, u32>,
    /// Value references that resolve to nothing in the file (globals).
    pub unresolved: HashSet<u32>,
    /// Referencing identifier -> how it uses the name: `read`, `write`
    /// (`x = 1`, destructuring, `for (x of ...)`), `read_write` (`x += 1`,
    /// `x++`) or `type` (type positions, `typeof x` in a type).
    pub access: HashMap<u32, &'static str>,
    /// Written identifier or member (`x`, `a.b`, `a[b]`), or array or
    /// object literal used as a destructuring target -> the node that writes
    /// it: the assignment, update, `delete` or `for` statement.
    pub write_sites: HashMap<u32, u32>,
    /// Function-like node -> what its own body does ([`USES_THIS`], ...).
    /// Arrow functions count toward the function around them.
    pub functions: HashMap<u32, u8>,
    /// Later declaring identifier of a merged name (`var a; var a;`,
    /// overloads, `class X` + `interface X`) -> the first, which owns the
    /// binding.
    pub merged: HashMap<u32, u32>,
    /// `arguments` reference -> the function whose implicit binding it reads.
    arguments: HashMap<u32, u32>,
}

pub const USES_THIS: u8 = 1;
pub const USES_ARGUMENTS: u8 = 2;

/// How a reference uses its name.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Use {
    Value,
    Type,
    /// A value read that only matters to the type checker.
    TypeOnly,
    Write,
    ReadWrite,
    /// The left of `A.B` in a type or `import x = A.B`: any meaning.
    Namespace,
    /// `export { x }`, `export default x`, `export = x`: any meaning, and
    /// a value use when it is a value.
    Export,
    /// `export type { x }`, `export { type x }`: any meaning, never a
    /// value use.
    TypeExport,
}

impl Bindings {
    /// The view of the engine's `names` for `s`'s file.
    pub fn from_names(s: &Syntax, names: Arc<Resolved>) -> Bindings {
        let v = &names.vocabulary;
        let (value, type_) = (v.declaration_class("value"), v.declaration_class("type"));
        let lexical = value | type_ | v.declaration_class("query");
        let (assigned, mutated, deleted) = (v.effect("assigned"), v.effect("mutated"), v.effect("deleted"));
        let mut b = Bindings { value, type_, ..Bindings::default() };
        let mut declared: Vec<(u32, u32)> =
            names.declared.iter().filter(|(d, (_, _, classes, _))| classes & lexical != 0 && !names.implicit.contains_key(d))
                .map(|(&d, &(_, site, ..))| (d, site)).collect();
        declared.sort_by_key(|&(d, ..)| s.tree.node(d).start);
        // Declaring identifier -> the function its uses inside count as self uses.
        let mut owners: HashMap<u32, u32> = HashMap::default();
        for (d, site) in declared {
            if !s.is(d, K::Ident) {
                continue;
            }
            let kind = declaration_kind(s, d, site);
            let (exported, ambient) = statement_flags(s, d, site, kind);
            if let Some(&first) = names.merged.get(&d) {
                b.merged.insert(d, first);
                if let Some(binding) = b.decls.get_mut(&first) {
                    binding.exported |= exported;
                    binding.ambient &= ambient;
                }
                continue;
            }
            b.decls.insert(d, Binding { kind, exported, ambient, ..Binding::default() });
            if let Some(owner) = self_use_owner(s, d, site, kind) {
                owners.insert(d, owner);
            }
        }
        for &id in &s.tree.order {
            if s.is(id, K::This) {
                if let Some(f) = this_owner(s, id) {
                    *b.functions.entry(f).or_default() |= USES_THIS;
                }
            }
        }

        for (&node, &(effect, site)) in &names.effects {
            let effect = Some(effect);
            let written = effect == assigned || effect == mutated;
            let target = matches!(s.kind(node), Some(K::Ident | K::Member | K::Index | K::Array | K::Object));
            if written && target || effect == deleted && matches!(s.kind(node), Some(K::Member | K::Index)) {
                b.write_sites.insert(node, site);
            }
        }

        for (&r, lookup) in &names.asked {
            let want = lookup.classes;
            if want & lexical == 0 || !s.is(r, K::Ident) {
                continue;
            }
            let effect = names.effects.get(&r).map(|&(e, _)| Some(e));
            let use_ = match effect {
                Some(e) if e == assigned => Use::Write,
                Some(e) if e == mutated => Use::ReadWrite,
                _ => classify(s, r, want, value, type_),
            };
            b.access.insert(
                r,
                match use_ {
                    Use::Value | Use::Namespace | Use::Export => "read",
                    Use::Write => "write",
                    Use::ReadWrite => "read_write",
                    Use::Type | Use::TypeOnly | Use::TypeExport => "type",
                },
            );
            let Some(&decl) = names.declaration.get(&r) else {
                if !matches!(use_, Use::Type | Use::TypeOnly | Use::TypeExport) {
                    b.unresolved.insert(r);
                }
                continue;
            };
            if names.implicit.contains_key(&decl) {
                b.arguments.insert(r, decl);
                if !matches!(use_, Use::TypeOnly | Use::TypeExport) {
                    *b.functions.entry(decl).or_default() |= USES_ARGUMENTS;
                }
                continue;
            }
            let decl = b.merged.get(&decl).copied().unwrap_or(decl);
            let inside = owners.get(&decl).is_some_and(|&o| {
                let (n, r) = (s.tree.node(o), s.tree.node(r));
                n.start <= r.start && r.end <= n.end
            });
            let binding = b.decls.entry(decl).or_default();
            if inside && use_ != Use::Write {
                binding.self_uses += 1;
            }
            match use_ {
                Use::Value | Use::Namespace | Use::Export => {
                    binding.uses += 1;
                    binding.value_uses += u32::from(use_ != Use::Namespace);
                }
                Use::Type | Use::TypeOnly | Use::TypeExport => binding.uses += 1,
                Use::Write => binding.writes += 1,
                Use::ReadWrite => {
                    binding.uses += 1;
                    binding.value_uses += 1;
                    binding.writes += 1;
                }
            }
            b.refs.insert(r, decl);
        }
        b.names = names;
        b
    }

    /// Implicit `arguments` has an owning function, but no declaring identifier.
    pub fn arguments_owner(&self, _s: &Syntax, reference: u32) -> Option<u32> {
        self.arguments.get(&reference).copied()
    }

    /// The declaration `name` means at `at` (a value, or any meaning when
    /// `value` is false), searching the scopes around it outward; `None`
    /// for a global, unknown name or implicit binding with no declaration.
    /// For names no reference spells out: `{}.hasOwnProperty` asks whether
    /// `Object` is the global one.
    pub fn visible(&self, s: &Syntax, at: u32, name: &str, value: bool) -> Option<u32> {
        let classes = if value { self.value } else { self.value | self.type_ };
        let found = self.names.visible(s.tree, s.src, &decoded_name, at, &decoded_name(name), classes)?;
        (!self.names.implicit.contains_key(&found)).then_some(found)
    }
}

/// How reference `r`, looked up in `want` classes, uses its name.
fn classify(s: &Syntax, r: u32, want: u64, value: u64, type_: u64) -> Use {
    if want == type_ {
        return Use::Type;
    }
    // A value read inside a type (`typeof x`, `typeof A.B`, a computed key)
    // matters only to the type checker.
    let mut at = s.parent(r);
    while let Some(n) = at {
        if s.kind(n).is_some_and(|k| k as u16 >= K::TypeRef as u16) {
            return Use::TypeOnly;
        }
        at = s.parent(n);
    }
    if want == value | type_ {
        let parent = s.parent(r);
        return match parent.and_then(|p| s.kind(p)) {
            Some(K::ExportSpec) => {
                let specifier = parent.expect("matched");
                let statement = s.parent(specifier);
                let type_only = s.word(specifier, F::Kind) == Some("type") || statement.is_some_and(|e| s.word(e, F::Kind) == Some("type"));
                if type_only { Use::TypeExport } else { Use::Export }
            }
            Some(K::Export | K::ExportAssign | K::Paren) => Use::Export,
            _ => Use::Namespace,
        };
    }
    Use::Value
}

/// What declares `name`, whose `declares` item sits on `site`.
fn declaration_kind(s: &Syntax, name: u32, site: u32) -> &'static str {
    match s.kind(site) {
        Some(K::Var) => match s.word(site, F::Kind).unwrap_or("var") {
            "let" => "let",
            "const" => "const",
            "var" => "var",
            _ => "using",
        },
        Some(K::Function | K::Arrow | K::Method | K::FnType | K::CallSig | K::MethodSig) if in_parameter(s, name, site) => "parameter",
        Some(K::Function) if is_statement(s, site) => "function",
        Some(K::Function) => "function_name",
        Some(K::Class) if is_statement(s, site) => "class",
        Some(K::Class) => "class_name",
        // A catch clause declares its parameter from its block.
        Some(K::Block) if s.parent(site).is_some_and(|p| s.is(p, K::Try)) => "catch",
        Some(K::Interface) => "interface",
        Some(K::TypeAlias) => "type",
        Some(K::Enum) => "enum",
        Some(K::Namespace) => "namespace",
        Some(K::TypeParam | K::Infer) => "type_parameter",
        Some(K::Import | K::ImportEquals) => "import",
        _ => "",
    }
}

/// `name` sits in one of `site`'s parameters.
fn in_parameter(s: &Syntax, name: u32, site: u32) -> bool {
    let mut at = Some(name);
    while let Some(n) = at.filter(|&n| n != site) {
        if s.is(n, K::Param) {
            return true;
        }
        at = s.parent(n);
    }
    false
}

/// Is this function or class a declaration (rather than an expression)?
fn is_statement(s: &Syntax, id: u32) -> bool {
    matches!(s.field(id), Some(F::Body | F::Decl))
        && s.parent(id).and_then(|p| s.kind(p)).is_some_and(|k| matches!(k, K::Program | K::Block | K::Case | K::Export | K::Labeled))
}

/// Whether the statement declaring `name` is `export`ed, and whether it is
/// ambient (inside `declare`).
fn statement_flags(s: &Syntax, name: u32, site: u32, kind: &str) -> (bool, bool) {
    // A parameter's statement is the parameter itself.
    let statement = if kind == "parameter" {
        let mut at = name;
        while !s.is(at, K::Param) {
            at = s.parent(at).unwrap_or(site);
            if at == site {
                break;
            }
        }
        at
    } else {
        site
    };
    let exported = s.parent(statement).is_some_and(|p| s.is(p, K::Export));
    let mut at = Some(statement);
    while let Some(n) = at {
        if s.has_word(n, F::Modifier, "declare") {
            return (exported, true);
        }
        at = s.parent(n);
    }
    (exported, false)
}

/// The function whose uses inside count as self uses of `name` (as ESLint
/// counts them): a function's own body, or a function initializer.
fn self_use_owner(s: &Syntax, name: u32, site: u32, kind: &str) -> Option<u32> {
    match kind {
        "function" => Some(site),
        "var" | "let" | "const" | "using" => {
            let declarator = s.parent(name).filter(|&d| s.is(d, K::Declarator) && s.get(d, F::Name) == Some(name))?;
            s.get(declarator, F::Init).filter(|&i| matches!(s.kind(i), Some(K::Function | K::Arrow)))
        }
        _ => None,
    }
}

/// The function whose own `this` a node at `id` sees: the nearest
/// non-arrow function, method or class body.
fn this_owner(s: &Syntax, id: u32) -> Option<u32> {
    let mut child = id;
    while let Some(n) = s.parent(child) {
        // A computed key (`[this.x]() {}`) runs where the class does.
        let key = s.field(child) == Some(F::Key);
        match s.kind(n) {
            Some(K::Method | K::ClassProp) if !key => return Some(n),
            Some(K::Function | K::StaticBlock) => return Some(n),
            _ => child = n,
        }
    }
    None
}

#[cfg(test)]
mod tests {
    #[test]
    fn escaped_names_share_bindings_and_keep_scope_boundaries() {
        crate::engine::tests::with_native_file(
            r"let a; void /*@*/a; { let a; void /*@*/\u{61}; } void /*@*/a; var b; var b; void /*@*/b;",
            |file, nodes| {
                let outer = file.decl_of(nodes[0]).expect("decoded outer binding");
                assert_ne!(file.decl_of(nodes[1]), Some(outer), "block binding shadows");
                assert_eq!(file.decl_of(nodes[2]), Some(outer));
                let merged = file.decl_of(nodes[3]).expect("merged binding");
                assert_eq!(file.bindings().refs.get(&nodes[3]), Some(&merged));
                assert_eq!(file.bindings().merged.values().filter(|&&first| first == merged).count(), 1);
                assert_eq!(file.bindings().visible(&file.syntax, nodes[0], "a", true), Some(outer));
            },
        );
    }

    #[test]
    fn escaped_arguments_still_use_the_implicit_function_binding() {
        use crate::native::ts::facts::Facts;
        crate::engine::tests::with_native_file(r"function f() { return /*@*/arguments; }", |file, nodes| {
            assert_eq!(file.resolved(nodes[0]), Some("local"));
            assert!(file.bindings().arguments_owner(&file.syntax, nodes[0]).is_some());
        });
    }

    #[test]
    fn escaped_type_names_do_not_shadow_value_names() {
        crate::engine::tests::with_native_file(
            r"const T = 1; function f() { type T = number; void /*@*/T; let value: /*@*/T; }",
            |file, nodes| {
                let value = file.decl_of(nodes[0]).expect("outer value");
                let type_name = file.syntax.get(nodes[1], super::F::Name).unwrap_or(nodes[1]);
                let annotation = file.decl_of(type_name).expect("inner type");
                assert_ne!(value, annotation);
                assert_eq!(file.binding(value).unwrap().kind, "const");
                assert_eq!(file.binding(annotation).unwrap().kind, "type");
            },
        );
    }

    #[test]
    fn explicit_arguments_shadowing_does_not_hide_nested_function_arguments() {
        use crate::native::ts::facts::Facts;
        crate::engine::tests::with_native_file(
            "const arguments = 'outer'; function f(arguments: string) { void /*@*/arguments; const arrow = () => /*@*/arguments; function inner() { return /*@*/arguments; } { const arguments = 1; void /*@*/arguments; } }",
            |file, nodes| {
                let bindings = file.bindings();
                let first = file.decl_of(nodes[0]).expect("the parameter");
                assert_eq!(file.decl_of(nodes[1]), Some(first), "arrow inherits the parameter");
                assert!(file.decl_of(nodes[2]).is_none(), "nested function has its own implicit binding");
                assert_eq!(file.resolved(nodes[2]), Some("local"));
                assert!(!bindings.unresolved.contains(&nodes[2]));
                assert_ne!(file.decl_of(nodes[3]), Some(first), "block local shadows the parameter");
            },
        );
        crate::engine::tests::with_native_file("const f = function arguments() { return /*@*/arguments; };", |file, nodes| {
            assert!(file.decl_of(nodes[0]).is_none(), "function expression name is outside its implicit arguments binding");
            assert_eq!(file.resolved(nodes[0]), Some("local"));
        });
    }

    #[test]
    fn arguments_value_and_type_namespaces_stay_distinct() {
        use crate::native::ts::facts::Facts;
        crate::engine::tests::with_native_file("function f() { let value: /*@*/arguments; type T = typeof /*@*/arguments; }", |file, nodes| {
            let type_name = file.syntax.get(nodes[0], super::F::Name).unwrap_or(nodes[0]);
            assert_eq!(file.resolved(type_name), None, "implicit arguments is not a type declaration");
            assert_eq!(file.resolved(nodes[1]), Some("local"), "typeof reads the implicit value");
        });
    }

    #[test]
    fn named_function_expression_parameters_shadow_the_self_name() {
        for name in ["arguments", "self"] {
            let source = format!("const f = function {name}({name}: string) {{ return /*@*/{name}; }};");
            crate::engine::tests::with_native_file(&source, |file, nodes| {
                let declaration = file.decl_of(nodes[0]).expect("the parameter binding");
                assert_eq!(file.binding(declaration).unwrap().kind, "parameter");
                assert!(file.type_flags(nodes[0]).unwrap_or_default().contains(&"string"));
            });
        }
    }

    #[test]
    fn names_resolve_at_any_node_through_enclosing_scopes() {
        let text = "let Object = 1; type T = 1; function f() { const T = 2; /*@*/a; { /*@*/b; } } /*@*/c;";
        crate::engine::tests::with_native_file(text, |file, marks| {
            let b = file.bindings();
            let s = &file.syntax;
            let name = |d: Option<u32>| d.map(|d| s.tree.node(d).start);
            let outer_object = Some(4);
            assert_eq!(name(b.visible(s, marks[0], "Object", true)), outer_object);
            assert_eq!(name(b.visible(s, marks[1], "T", true)), Some(49), "the inner const, not the outer type");
            assert_eq!(name(b.visible(s, marks[2], "T", true)), None, "only a type outside");
            assert_eq!(name(b.visible(s, marks[2], "T", false)), Some(21));
            assert_eq!(b.visible(s, marks[2], "Math", true), None);
        })
    }
}
