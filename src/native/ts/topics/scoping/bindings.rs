//! Declarations, scopes and names (`std/ts/scoping`): shadowing, use
//! before definition, naming, `this`.

use std::collections::{HashMap, HashSet};

use crate::expr::Value;
use crate::native::ts::facts::NativeFile;
use crate::native::ts::kinds::{Field as F, Kind as K};
use crate::native::ts::syntax::Syntax;

pub(super) fn underscore_names(file: &NativeFile, node: u32) -> Value<'static> {
    let s = &file.syntax;
    let mut names = Vec::new();
    match s.kind(node) {
        Some(K::Declarator) => {
            if let Some(pattern) = s.get(node, F::Name) {
                let mut declarations: Vec<_> = file.bindings().decl_scope.keys().copied().filter(|&n| inside(s, n, pattern)).collect();
                declarations.sort_by_key(|&n| s.tree.node(n).start);
                for name in declarations {
                    let mut cursor = s.parent(name);
                    let mut context = "variable";
                    while let Some(parent) = cursor {
                        if parent == node { break; }
                        match s.kind(parent) {
                            Some(K::ArrayPattern) => { context = "array"; break; }
                            Some(K::ObjectPattern) => { context = "object"; break; }
                            _ => {}
                        }
                        cursor = s.parent(parent);
                    }
                    names.push((name, node, context));
                }
            }
        }
        Some(K::Function | K::Arrow | K::Method) => {
            if s.is(node, K::Function) {
                if let Some(name) = s.get(node, F::Name).filter(|n| file.bindings().decls.get(n).is_some_and(|b| b.kind == "function")) {
                    names.push((name, node, "function"));
                }
            }
            for parameter in s.all(node, F::Param) {
                if let Some(name) = s.get(parameter, F::Name).map(|n| {
                    if s.is(n, K::Rest) { s.get(n, F::Arg).unwrap_or(n) } else { n }
                }).filter(|&n| s.is(n, K::Ident)) {
                    names.push((name, parameter, "parameter"));
                }
            }
            if s.is(node, K::Method) {
                let class = s.parent(node).is_some_and(|n| s.is(n, K::Class));
                if class || !matches!(s.word(node, F::Kind), Some("get" | "set")) {
                    if let Some(key) = s.get(node, F::Key).filter(|&n| matches!(s.kind(n), Some(K::Ident | K::PrivateName))) {
                        names.push((key, node, "method"));
                    }
                }
            }
        }
        Some(K::ClassProp) => {
            if let Some(key) = s.get(node, F::Key).filter(|&n| matches!(s.kind(n), Some(K::Ident | K::PrivateName))) {
                names.push((key, node, "field"));
            }
        }
        Some(K::Member | K::Index) => {
            if let Some(key) = s.get(node, F::Property).or_else(|| s.get(node, F::Index)).filter(|&n| s.is(n, K::Ident)) {
                names.push((key, node, "member"));
            }
        }
        _ => {}
    }
    Value::list(names.into_iter().filter_map(|(name, at, context)| {
        let display = s.text(name);
        let name = display.strip_prefix('#').unwrap_or(display);
        if name == "_" || !(name.starts_with('_') || name.ends_with('_')) {
            return None;
        }
        let object = s.get(node, F::Object).map(|n| s.unparen(n));
        let this_constructor = object.is_some_and(|n| s.is(n, K::Member) && s.word(n, F::Property) == Some("constructor")
            && s.get(n, F::Object).is_some_and(|o| s.is(s.unparen(o), K::This)));
        Some(Value::record(vec![
            ("name".into(), Value::from(name.to_string())),
            ("display".into(), Value::from(display.to_string())),
            ("context".into(), Value::from(context)),
            ("this".into(), Value::Bool(object.is_some_and(|n| s.is(n, K::This)))),
            ("super".into(), Value::Bool(object.is_some_and(|n| s.is(n, K::Super)))),
            ("constructor".into(), Value::Bool(this_constructor)),
            ("offset".into(), Value::from(s.tree.node(at).start as usize)),
        ]))
    }).collect())
}

struct OutsideBindings(HashMap<u32, Vec<u32>>);

fn collect_outside_bindings(file: &NativeFile) -> OutsideBindings {
    let s = &file.syntax;
    let b = file.bindings();
    let mut uses: HashMap<u32, Vec<u32>> = HashMap::new();
    for (&reference, &declaration) in &b.refs {
        uses.entry(declaration).or_default().push(reference);
    }
    for &id in b.decl_scope.keys() {
        if declaration_kind(s, id) != "var" { continue; }
        let mut cursor = s.parent(id);
        while let Some(n) = cursor {
            if s.is(n, K::Declarator) {
                if s.get(n, F::Init).is_some() {
                    uses.entry(b.merged.get(&id).copied().unwrap_or(id)).or_default().push(id);
                }
                break;
            }
            cursor = s.parent(n);
        }
    }
    let mut out = HashMap::new();
    let mut seen = HashSet::new();
    let mut declarations: Vec<u32> = b.decl_scope.keys().copied().collect();
    declarations.sort_by_key(|&id| s.tree.node(id).start);
    for id in declarations {
        if declaration_kind(s, id) != "var" {
            continue;
        }
        let canonical = b.merged.get(&id).copied().unwrap_or(id);
        let mut cursor = s.parent(id);
        let mut statement = id;
        let mut context = s.root();
        while let Some(n) = cursor {
            if s.is(n, K::Var) {
                statement = n;
            }
            if matches!(s.kind(n), Some(K::Block | K::For | K::ForIn | K::ForOf | K::Switch | K::StaticBlock | K::Program)) {
                context = n;
                break;
            }
            cursor = s.parent(n);
        }
        if !seen.insert((canonical, statement)) {
            continue;
        }
        for &reference in uses.get(&canonical).into_iter().flatten() {
            if !inside(s, reference, context) {
                out.entry(reference).or_insert_with(Vec::new).push(id);
            }
        }
    }
    OutsideBindings(out)
}

fn location(s: &Syntax, id: u32) -> (usize, usize) {
    let prefix = &s.src[..s.tree.node(id).start as usize];
    (prefix.bytes().filter(|&c| c == b'\n').count() + 1,
        prefix.rsplit('\n').next().unwrap_or("").encode_utf16().count() + 1)
}

pub(super) fn outside_bindings(file: &NativeFile, id: u32) -> Value<'static> {
    Value::list(file.cached(collect_outside_bindings).0.get(&id).into_iter().flatten().map(|&declaration| {
        let (line, column) = location(&file.syntax, declaration);
        Value::record(vec![("line".into(), Value::from(line)), ("column".into(), Value::from(column))])
    }).collect())
}

pub(super) fn strict_code(file: &NativeFile, node: u32) -> bool {
    if file.source_type() == "module" {
        return true;
    }
    let s = &file.syntax;
    let mut cursor = Some(node);
    while let Some(n) = cursor {
        if matches!(s.kind(n), Some(K::Class | K::StaticBlock)) {
            return true;
        }
        if matches!(s.kind(n), Some(K::Program | K::Function | K::Arrow | K::Method)) {
            let block = if s.is(n, K::Program) { n } else { s.get(n, F::Body).unwrap_or(n) };
            for statement in s.all(block, F::Body) {
                let Some(expression) = s.get(statement, F::Expr).filter(|&n| s.is(n, K::String)) else { break };
                if matches!(s.text(expression), "'use strict'" | "\"use strict\"") {
                    return true;
                }
            }
        }
        cursor = s.parent(n);
    }
    false
}

pub(super) fn declaration_position(file: &NativeFile, node: u32) -> Value<'static> {
    let s = &file.syntax;
    let parent = s.parent(node);
    let nested = !parent.is_some_and(|p| matches!(s.kind(p), Some(K::Program | K::StaticBlock | K::Export))
        || s.is(p, K::Block) && s.parent(p).is_some_and(|n| matches!(s.kind(n), Some(K::Function | K::Arrow | K::Method | K::StaticBlock))));
    let mut body = "program";
    let mut cursor = parent;
    while let Some(n) = cursor {
        match s.kind(n) {
            Some(K::StaticBlock) => { body = "class static block body"; break; }
            Some(K::Function | K::Arrow | K::Method) => { body = "function body"; break; }
            _ => {}
        }
        cursor = s.parent(n);
    }
    Value::record(vec![
        ("nested".into(), Value::Bool(nested)),
        ("top".into(), Value::Bool(variable_on_top(s, node))),
        ("declaration".into(), Value::Bool(s.is(node, K::Function) && parent.is_some_and(|p| matches!(s.kind(p), Some(K::Program | K::Block | K::Export | K::If | K::Labeled | K::While | K::DoWhile | K::For | K::ForIn | K::ForOf | K::Case))))),
        ("body".into(), Value::from(body)),
        ("strict".into(), Value::Bool(strict_code(file, parent.unwrap_or(node)))),
    ])
}

fn variable_on_top(s: &Syntax, node: u32) -> bool {
    let mut statement = node;
    let Some(mut parent) = s.parent(statement) else { return false };
    if s.is(parent, K::Export) {
        statement = parent;
        let Some(p) = s.parent(parent) else { return false };
        parent = p;
    }
    let top_level = matches!(s.kind(parent), Some(K::Program | K::StaticBlock))
        || s.is(parent, K::Block) && s.parent(parent).is_some_and(|n| matches!(s.kind(n), Some(K::Function | K::Arrow | K::Method | K::StaticBlock)));
    if !top_level {
        return false;
    }
    let mut declarations = false;
    for sibling in s.all(parent, F::Body) {
        if sibling == statement {
            return true;
        }
        if !declarations && !s.is(parent, K::StaticBlock) && !s.parent(parent).is_some_and(|n| s.is(n, K::StaticBlock))
            && (s.is(sibling, K::Import) || s.is(sibling, K::ExprStmt) && s.get(sibling, F::Expr).is_some_and(|n| s.is(n, K::String)))
        {
            continue;
        }
        if s.is(sibling, K::Var) || s.is(sibling, K::Export) && s.get(sibling, F::Decl).is_some_and(|n| s.is(n, K::Var)) {
            declarations = true;
        } else {
            return false;
        }
    }
    false
}

#[derive(Default)]
struct Redeclarations(HashMap<u32, (bool, bool, bool, bool)>);

pub(super) fn declaration_kind(s: &Syntax, id: u32) -> &'static str {
    let mut child = id;
    while let Some(parent) = s.parent(child) {
        match s.kind(parent) {
            Some(K::Var) => return match s.word(parent, F::Kind) {
                Some("let") => "let",
                Some("const") => "const",
                Some("using" | "await using") => "using",
                _ => "var",
            },
            Some(K::Function) => return "function",
            Some(K::Class) => return "class",
            Some(K::Interface) => return "interface",
            Some(K::Enum) => return "enum",
            Some(K::Namespace) => return "namespace",
            Some(K::TypeAlias) => return "type",
            Some(K::Param) => return "parameter",
            Some(K::TypeParam) => return "type_param",
            Some(K::Import | K::ImportSpec) => return "import",
            _ => {}
        }
        child = parent;
    }
    ""
}

fn redeclarations(file: &NativeFile) -> Redeclarations {
    let s = &file.syntax;
    let b = file.bindings();
    let ts = matches!(file.path.extension().and_then(|e| e.to_str()), Some("ts" | "tsx" | "mts" | "cts"));
    let mut groups: HashMap<(usize, &str), Vec<u32>> = HashMap::new();
    for (&id, &scope) in &b.decl_scope {
        if ts && s.parent(id).is_some_and(|p| s.is(p, K::Function) && s.get(p, F::Body).is_none()) {
            continue;
        }
        groups.entry((scope, s.text(id))).or_default().push(id);
    }
    let mut out = Redeclarations::default();
    for ((scope, name), mut ids) in groups {
        ids.sort_by_key(|&id| s.tree.node(id).start);
        let kinds: Vec<_> = ids.iter().map(|&id| declaration_kind(s, id)).collect();
        let merge_category = if kinds.iter().all(|&k| matches!(k, "class" | "interface" | "namespace")) {
            Some("class")
        } else if kinds.iter().all(|&k| matches!(k, "function" | "namespace")) {
            Some("function")
        } else if kinds.iter().all(|&k| matches!(k, "enum" | "namespace")) {
            Some("enum")
        } else {
            None
        };
        let merge_all = kinds.iter().all(|&k| k == "interface") || kinds.iter().all(|&k| k == "namespace");
        let builtin = scope == 0 && file.source_type() == "script"
            && crate::native::ts::globals::is_builtin(name)
            && matches!(file.globals().setting(name), Some("readonly" | "writable"));
        let comment = ts && scope == 0 && file.source_type() == "script" && file.globals().in_comment(name) && !builtin;
        let mut runtime_seen = false;
        for (i, (&id, &kind)) in ids.iter().zip(&kinds).enumerate() {
            let merge_report = if merge_all { false } else if let Some(category) = merge_category {
                let report = kind == category && runtime_seen;
                runtime_seen |= kind == category;
                report
            } else { i > 0 };
            out.0.insert(id, (i > 0, ts && !merge_report, builtin, comment));
        }
    }
    out
}

pub(super) fn redeclaration(file: &NativeFile, id: u32) -> Value<'static> {
    let Some(&(duplicate, safe_merge, builtin, comment)) = file.cached(redeclarations).0.get(&id) else { return Value::Null };
    Value::record(vec![
        ("duplicate".into(), Value::Bool(duplicate)),
        ("safe_merge".into(), Value::Bool(safe_merge)),
        ("builtin".into(), Value::Bool(builtin)),
        ("comment".into(), Value::Bool(comment)),
    ])
}

pub(super) fn this_alias(file: &NativeFile, node: u32) -> Value<'static> {
    let s = &file.syntax;
    let declarator = s.is(node, K::Declarator);
    let Some(name) = s.get(node, if declarator { F::Name } else { F::Left }).filter(|&n| s.is(n, K::Ident)) else {
        return Value::Null;
    };
    let value = s.get(node, if declarator { F::Init } else { F::Right });
    let is_this = value.is_some_and(|n| s.is(s.unparen(n), K::This));
    let mut assigned = value.is_some();
    if declarator && !assigned {
        let b = file.bindings();
        let first = b.merged.get(&name).copied().unwrap_or(name);
        assigned = b.decl_scope.iter().any(|(&id, &scope)| {
            b.merged.get(&id).copied().unwrap_or(id) == first && b.decl_scope.get(&name) == Some(&scope)
                && s.parent(id).is_some_and(|n| s.is(n, K::Declarator) && s.get(n, F::Init).is_some())
        }) || b.refs.iter().any(|(&reference, &declaration)| {
            declaration == first && b.ref_scope.get(&reference) == b.decl_scope.get(&name)
                && b.write_sites.get(&reference).is_some_and(|&site| {
                    s.is(site, K::Assign) && s.word(site, F::Op) == Some("=")
                        && s.get(site, F::Right).is_some_and(|n| s.is(s.unparen(n), K::This))
                })
        });
    }
    Value::record(vec![
        ("name".into(), Value::from(s.text(name).to_string())),
        ("this".into(), Value::Bool(is_this)),
        ("valid".into(), Value::Bool(is_this && (declarator || s.word(node, F::Op) == Some("=")))),
        ("value".into(), Value::Bool(value.is_some())),
        ("assigned".into(), Value::Bool(assigned)),
    ])
}

struct ScopeNames(HashMap<(usize, String), u32>);

fn scope_names(file: &NativeFile) -> ScopeNames {
    let s = &file.syntax;
    let b = file.bindings();
    let mut names = HashMap::new();
    for (&id, &scope) in &b.decl_scope {
        let canonical = b.merged.get(&id).copied().unwrap_or(id);
        let named_function = b.decls.get(&canonical).is_some_and(|b| b.kind == "function_name");
        if b.merged.contains_key(&id) && !named_function {
            continue;
        }
        let entry = names.entry((scope, s.text(id).to_string())).or_insert(id);
        if named_function && id != canonical && *entry == canonical
            || !(named_function && id == canonical && *entry != canonical) && s.tree.node(id).start < s.tree.node(*entry).start
        {
            *entry = id;
        }
    }
    ScopeNames(names)
}

fn value_binding(file: &NativeFile, id: u32) -> bool {
    let s = &file.syntax;
    let b = file.bindings();
    let kind = b.decls.get(&id).map(|b| b.kind).unwrap_or("");
    if matches!(kind, "type" | "interface" | "type_parameter") {
        return false;
    }
    if kind == "import" {
        let mut child = id;
        while let Some(parent) = s.parent(child) {
            if s.word(parent, F::Kind) == Some("type") {
                return false;
            }
            if s.is(parent, K::Import) {
                break;
            }
            child = parent;
        }
    }
    true
}

pub(super) fn shadow(file: &NativeFile, id: u32) -> Value<'static> {
    let s = &file.syntax;
    let b = file.bindings();
    let enum_scope = s.parent(id).filter(|&n| s.is(n, K::EnumMember) && s.field(id) == Some(F::Name))
        .and_then(|n| s.parent(n)).and_then(|n| s.get(n, F::Name)).and_then(|n| b.decl_scope.get(&n)).copied();
    let Some(scope) = b.decl_scope.get(&id).copied().or(enum_scope) else { return Value::Null };
    let canonical = b.merged.get(&id).copied().unwrap_or(id);
    let named_function = b.decls.get(&canonical).is_some_and(|b| b.kind == "function_name");
    if b.merged.contains_key(&id) && !named_function || s.text(id) == "this" {
        return Value::Null;
    }
    let enum_binding = crate::native::ts::bind::Binding { kind: "enum_member", ..Default::default() };
    let Some(binding) = b.decls.get(&canonical).or(enum_scope.map(|_| &enum_binding)) else { return Value::Null };
    let declaration_file = file.path.file_name().and_then(|n| n.to_str())
        .is_some_and(|n| [".d.ts", ".d.cts", ".d.mts"].iter().any(|suffix| n.ends_with(suffix)));
    if declaration_file {
        let mut cursor = s.parent(id);
        while let Some(owner) = cursor {
            if matches!(s.kind(owner), Some(K::Var | K::Class | K::Enum | K::Namespace | K::Function | K::Param)) {
                if s.has_word(owner, F::Modifier, "declare")
                    || s.is(owner, K::Function) && s.get(owner, F::Body).is_none()
                {
                    return Value::Null;
                }
                break;
            }
            cursor = s.parent(owner);
        }
    }
    let names = file.cached(scope_names);
    let mut cursor = if enum_scope.is_some() { Some(scope) } else { b.scopes[scope].parent };
    let mut outer = (named_function && id != canonical).then_some(canonical);
    while let Some(scope) = cursor {
        if outer.is_some() { break; }
        if let Some(&declaration) = names.0.get(&(scope, s.text(id).to_string())) {
            outer = Some(declaration);
            break;
        }
        cursor = b.scopes[scope].parent;
    }
    let ts = matches!(file.path.extension().and_then(|e| e.to_str()), Some("ts" | "tsx" | "mts" | "cts"));
    let builtin = outer.is_none() && (scope != 0 || file.source_type() != "script" || ts)
        && matches!(file.globals().setting(s.text(id)), Some("readonly" | "writable"));
    if outer.is_none() && !builtin {
        return Value::Null;
    }
    let mut owner = s.parent(id).unwrap_or(id);
    while matches!(s.kind(owner), Some(K::Param | K::TypeParam)) {
        owner = s.parent(owner).unwrap_or(owner);
        if owner == s.root() { break; }
    }
    let type_parameter_function = binding.kind == "parameter"
        && (matches!(s.kind(owner), Some(K::FnType | K::MethodSig | K::CallSig))
        || matches!(s.kind(owner), Some(K::Function | K::Method)) && s.get(owner, F::Body).is_none());
    let static_generic = binding.kind == "type_parameter" && s.is(owner, K::Method) && s.has_word(owner, F::Modifier, "static");
    let mut cursor = s.parent(id);
    while let Some(n) = cursor {
        if s.is(n, K::Namespace) {
            if s.word(n, F::Kind) == Some("global") || s.word(n, F::Name) == Some("global") {
                return Value::Null;
            }
            if let Some(outer) = outer.filter(|&o| !value_binding(file, o)) {
                let mut import = s.parent(outer);
                while let Some(i) = import {
                    if s.is(i, K::Import) {
                        if s.get(i, F::From).zip(s.get(n, F::Name)).is_some_and(|(from, name)| s.text(from).trim_matches(['\'', '"']) == s.text(name).trim_matches(['\'', '"'])) {
                            return Value::Null;
                        }
                        break;
                    }
                    import = s.parent(i);
                }
            }
        }
        cursor = s.parent(n);
    }
    let mut initializing = false;
    let mut name_initializer = false;
    if let Some(outer) = outer {
        let mut child = outer;
        while let Some(parent) = s.parent(child) {
            let init = match s.kind(parent) {
                Some(K::Declarator | K::Param) => s.get(parent, F::Init),
                Some(K::AssignPattern) => s.get(parent, F::Right),
                Some(K::PatternProp) => s.get(parent, F::Value).filter(|&n| s.is(n, K::AssignPattern)).and_then(|n| s.get(n, F::Right)),
                Some(K::Var) => s.parent(parent).filter(|&n| matches!(s.kind(n), Some(K::ForIn | K::ForOf))).and_then(|n| s.get(n, F::Right)),
                _ => None,
            };
            if let Some(init) = init {
                if !inside(s, id, init) {
                    child = parent;
                    continue;
                }
                initializing = initialization_callback(file, scope, outer, init)
                    && !(ts && binding.kind == "function_name");
                if id == canonical && matches!(binding.kind, "function_name" | "class_name") {
                    let mut expression = s.parent(id).unwrap_or(id);
                    while let Some(parent) = s.parent(expression) {
                        if s.is(parent, K::Paren) || s.is(parent, K::Conditional) && s.field(expression) != Some(F::Test)
                            || s.is(parent, K::Logical)
                        {
                            expression = parent;
                        } else {
                            break;
                        }
                    }
                    name_initializer = s.unparen(expression) == s.unparen(init);
                }
                break;
            }
            if matches!(s.kind(parent), Some(K::Function | K::Arrow | K::Method | K::Class | K::Import)) {
                break;
            }
            child = parent;
        }
    }
    if name_initializer || static_generic {
        return Value::Null;
    }
    let (line, column) = outer.map(|o| location(s, o)).unwrap_or((1, 1));
    Value::record(vec![
        ("global".into(), Value::Bool(builtin)),
        ("enum_member".into(), Value::Bool(ts && enum_scope.is_some())),
        ("line".into(), Value::from(line)),
        ("column".into(), Value::from(column)),
        ("kind".into(), Value::from(outer.and_then(|o| b.decls.get(&o)).map(|b| b.kind).unwrap_or(""))),
        ("early".into(), Value::Bool(outer.is_some_and(|o| s.tree.node(id).end < s.tree.node(o).start))),
        ("type_value".into(), Value::Bool(value_binding(file, id) != outer.is_none_or(|o| value_binding(file, o)))),
        ("function_type".into(), Value::Bool(type_parameter_function)),
        ("initializing".into(), Value::Bool(initializing)),
    ])
}

fn initialization_callback(file: &NativeFile, mut scope: usize, declaration: u32, init: u32) -> bool {
    let s = &file.syntax;
    let b = file.bindings();
    while !matches!(s.kind(b.scopes[scope].node), Some(K::Function | K::Arrow | K::Method | K::Program)) {
        let Some(parent) = b.scopes[scope].parent else { return false };
        scope = parent;
    }
    let node = b.scopes[scope].node;
    if !matches!(s.kind(node), Some(K::Function | K::Arrow)) {
        return false;
    }
    if b.scopes[scope].parent != b.decl_scope.get(&declaration).copied() {
        return false;
    }
    let mut cursor = s.parent(node);
    while let Some(parent) = cursor {
        if !inside(s, parent, init) { return false; }
        if s.is(parent, K::Call) { return true; }
        if matches!(s.kind(parent), Some(K::Function | K::Arrow | K::Method)) { return false; }
        cursor = s.parent(parent);
    }
    false
}

fn inside(s: &Syntax, inner: u32, outer: u32) -> bool {
    let (i, o) = (s.tree.node(inner), s.tree.node(outer));
    o.start <= i.start && i.end <= o.end
}

pub(super) fn execution_scope(file: &NativeFile, node: u32, declaration: bool) -> usize {
    let b = file.bindings();
    let mut scope = if declaration { b.decl_scope.get(&node) } else { b.ref_scope.get(&node).or_else(|| b.decl_scope.get(&node)) }.copied().unwrap_or(0);
    while !b.scopes[scope].function {
        let Some(parent) = b.scopes[scope].parent else { break };
        scope = parent;
    }
    scope
}

fn separate_context(file: &NativeFile, reference: u32, declaration: u32) -> bool {
    let s = &file.syntax;
    let b = file.bindings();
    let boundary = b.decl_scope.get(&declaration).map(|&scope| b.scopes[scope].node).unwrap_or(s.root());
    let mut child = reference;
    while let Some(parent) = s.parent(child) {
        if parent == boundary {
            break;
        }
        if matches!(s.kind(parent), Some(K::Function | K::Arrow | K::Method))
            && matches!(s.field(child), Some(F::Body | F::Param))
        {
            return true;
        }
        if s.is(parent, K::ClassProp) && s.field(child) == Some(F::Init)
            && !s.all(parent, F::Modifier).any(|m| s.text(m) == "static")
        {
            return true;
        }
        child = parent;
    }
    false
}

pub(super) fn definition_use(file: &NativeFile, reference: u32) -> Value<'static> {
    let s = &file.syntax;
    let b = file.bindings();
    let Some(&declaration) = b.refs.get(&reference) else { return Value::Null };
    let Some(binding) = b.decls.get(&declaration) else { return Value::Null };
    let separate = separate_context(file, reference, declaration);
    let mut initializing = false;
    let mut decorator = false;
    if !separate && b.access.get(&reference) != Some(&"type") {
        let mut child = declaration;
        while let Some(parent) = s.parent(child) {
            match s.kind(parent) {
                Some(K::AssignPattern | K::Param) => {
                    initializing |= s.get(parent, F::Right).or_else(|| s.get(parent, F::Init)).is_some_and(|n| inside(s, reference, n));
                }
                Some(K::PatternProp) => {
                    initializing |= s.get(parent, F::Value).filter(|&n| s.is(n, K::AssignPattern))
                        .and_then(|n| s.get(n, F::Right)).is_some_and(|n| inside(s, reference, n));
                }
                Some(K::Declarator) => {
                    initializing |= s.get(parent, F::Init).is_some_and(|n| inside(s, reference, n));
                    if let Some(loop_node) = s.parent(parent).and_then(|n| s.parent(n)) {
                        if matches!(s.kind(loop_node), Some(K::ForIn | K::ForOf)) {
                            initializing |= s.get(loop_node, F::Right).is_some_and(|n| inside(s, reference, n));
                        }
                    }
                    break;
                }
                Some(K::Class) => {
                    decorator = s.all(parent, F::Decorator).any(|n| inside(s, reference, n));
                    let static_init = s.all(parent, F::Member).any(|m| {
                        s.is(m, K::StaticBlock) && inside(s, reference, m)
                            || s.is(m, K::ClassProp) && s.all(m, F::Modifier).any(|n| s.text(n) == "static")
                                && s.get(m, F::Init).is_some_and(|n| inside(s, reference, n))
                    });
                    initializing = inside(s, reference, parent) && !static_init;
                    break;
                }
                Some(K::Function | K::Arrow | K::Method | K::Import | K::Export | K::Try) => break,
                _ => {}
            }
            child = parent;
        }
    }
    let named_export = s.parent(reference).is_some_and(|n| s.is(n, K::ExportSpec));
    Value::record(vec![
        ("kind".into(), Value::from(binding.kind)),
        ("before".into(), Value::Bool(!decorator && (s.tree.node(reference).end < s.tree.node(declaration).end || initializing))),
        ("separate".into(), Value::Bool(separate)),
        ("export".into(), Value::Bool(named_export)),
    ])
}

struct ParamPropertyWrites(HashSet<u32>);

pub(super) fn param_property_write(file: &NativeFile, node: u32) -> Value<'static> {
    Value::Bool(file.cached(collect_param_property_writes).0.contains(&node))
}

fn collect_param_property_writes(file: &NativeFile) -> ParamPropertyWrites {
    let s = &file.syntax;
    let bindings = file.bindings();
    let mut first: HashMap<(u32, u32), u32> = HashMap::new();
    for (&reference, &declaration) in &bindings.refs {
        if bindings.decls.get(&declaration).is_none_or(|b| b.kind != "parameter") {
            continue;
        }
        let mut child = reference;
        while let Some(parent) = s.parent(child) {
            match (s.kind(parent), s.field(child)) {
                (Some(K::Member), Some(F::Property))
                | (Some(K::Index), Some(F::Index))
                | (Some(K::Conditional), Some(F::Test))
                | (Some(K::Call | K::New), Some(F::Arg)) => break,
                _ => {}
            }
            if matches!(s.kind(parent), Some(K::Member | K::Index)) {
                if let Some(&site) = bindings.write_sites.get(&parent) {
                    let candidate = first.entry((site, declaration)).or_insert(reference);
                    if s.tree.node(reference).start < s.tree.node(*candidate).start {
                        *candidate = reference;
                    }
                    break;
                }
            }
            child = parent;
        }
    }
    ParamPropertyWrites(first.into_values().collect())
}

pub(super) fn scoping_receiver_use(file: &NativeFile, node: u32) -> Value<'static> {
    let s = &file.syntax;
    let body = match s.kind(node) {
        Some(K::Method) => s.get(node, F::Body),
        Some(K::ClassProp) => s.get(node, F::Init).map(|init| {
            let init = s.unparen(init);
            if s.is(init, K::Function) { s.get(init, F::Body).unwrap_or(init) } else { init }
        }),
        _ => None,
    };
    Value::Bool(body.is_some_and(|body| receiver_in(s, body)))
}

fn receiver_in(s: &Syntax, node: u32) -> bool {
    match s.kind(node) {
        Some(K::This | K::Super) => true,
        Some(K::Function | K::Method | K::StaticBlock | K::ClassProp) => false,
        Some(K::Class) => {
            s.all(node, F::Decorator).any(|n| receiver_in(s, n))
                || s.all(node, F::Extends).any(|n| receiver_in(s, n))
                || s.all(node, F::Member).any(|member| {
                    s.get(member, F::Key).is_some_and(|key| s.is(key, K::Computed) && receiver_in(s, key))
                })
        }
        _ => s.kids(node).any(|(_, child)| receiver_in(s, child)),
    }
}

pub(super) fn scoping_head_offset(file: &NativeFile, node: u32) -> Value<'static> {
    let s = &file.syntax;
    let node = s.unparen(node);
    let head = match s.kind(node) {
        Some(K::Member) => s.get(node, F::Property),
        Some(K::Index) => s.get(node, F::Index),
        Some(K::Arrow) => s.get(node, F::Op),
        _ => None,
    }
    .unwrap_or(node);
    Value::from(s.tree.node(s.unparen(head)).start as usize)
}

pub(super) fn scoping_date_utc(file: &NativeFile, node: u32) -> Value<'static> {
    let s = &file.syntax;
    let node = s.unparen(node);
    let is_utc = matches!(s.kind(node), Some(K::Member | K::Index))
        && crate::native::ts::statics::property_name(s, node).as_deref() == Some("UTC");
    Value::Bool(is_utc && s.get(node, F::Object).is_some_and(|object| is_global_date(file, object)))
}

fn is_global_date(file: &NativeFile, node: u32) -> bool {
    let s = &file.syntax;
    let node = s.unparen(node);
    match s.kind(node) {
        Some(K::Ident) => {
            s.text(node) == "Date"
                && file.bindings().unresolved.contains(&node)
                && file.globals().setting("Date") != Some("off")
        }
        Some(K::Member | K::Index) => {
            if crate::native::ts::statics::property_name(s, node).as_deref() != Some("Date") {
                return false;
            }
            s.get(node, F::Object).is_some_and(|object| {
                let object = s.unparen(object);
                s.is(object, K::Ident)
                    && matches!(s.text(object), "global" | "globalThis" | "self" | "window")
                    && file.bindings().unresolved.contains(&object)
                    && matches!(file.globals().setting(s.text(object)), Some("readonly" | "writable"))
            })
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use crate::engine::tests::native_linter;

    #[test]
    fn initialization_shadow_exception_stays_in_the_initializing_callback() {
        let l = native_linter("rule no-shadow { set ignoreOnInitialization true }");
        let path = Path::new("/root/file.mjs");
        assert!(l.lint(path, "const value = consume(value => value);").is_empty());
        assert!(l.lint(path, "const { value = 0 } = consume(value => value);").is_empty());
        assert_eq!(l.lint(path, "const value = () => { consume(value => value); };").len(), 1);
    }

    #[test]
    fn declaration_files_exempt_ambient_bindings_not_runtime_shadows() {
        let l = native_linter("rule no-shadow { set builtinGlobals true }");
        let code = "/* globals Thing */ declare class Thing {}";
        assert!(l.lint(Path::new("/root/file.d.ts"), code).is_empty());
        assert_eq!(l.lint(Path::new("/root/file.mts"), code).len(), 1);
        assert_eq!(l.lint(Path::new("/root/file.d.ts"), "/* globals Thing */ declare class Thing {} function f(Thing: string) {}").len(), 1);
    }
}
