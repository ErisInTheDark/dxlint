//! Type relations at contextual and explicitly declared transfer sites.

use crate::expr::Value;
use crate::native::ts::facts::NativeFile;
use crate::native::ts::kinds::{Field as F, Kind as K};
use crate::native::ts::types::{Checker, Ty};

struct Query {
    remaining: usize,
}

impl Query {
    fn new() -> Self { Self { remaining: 4096 } }
    fn visit(&mut self) -> bool {
        if self.remaining == 0 { return false }
        self.remaining -= 1;
        true
    }
}

pub(super) fn flow(file: &NativeFile<'_>, node: u32) -> Value<'static> {
    let args = crate::expr::facts::args();
    let site = args.first().and_then(node_id).unwrap_or(node);
    let Some(source) = args.get(1).and_then(node_id) else { return Value::Null };
    let checker = &file.typing().checker;
    let mut query = Query::new();
    Value::list(contexts(file, site, source).into_iter().map(|(origin, receiver)| {
        let expression = typed_expression(file, source, &receiver, 0, &mut query);
        let mut names = Vec::new();
        enum_names(checker, &receiver, &mut Vec::new(), &mut names, &mut query);
        Value::record(vec![
            ("origin".into(), Value::from(origin)),
            ("relation".into(), expression.key("relation")),
            ("expression".into(), expression),
            ("enum_names".into(), Value::list(names.into_iter().map(Value::from).collect())),
            ("complete".into(), Value::Bool(query.remaining > 0)),
        ])
    }).collect())
}

fn typed_expression(file: &NativeFile<'_>, source: u32, receiver: &Ty, depth: usize, query: &mut Query) -> Value<'static> {
    if !query.visit() { return Value::Null }
    let syntax = &file.syntax;
    let source = syntax.unparen(source);
    let checker = &file.typing().checker;
    let kind = match syntax.kind(source) {
        Some(K::Binary) => "binary",
        Some(K::Logical) => "logical",
        Some(K::Unary) => "unary",
        _ => "leaf",
    };
    let operands = if depth >= 64 {
        query.remaining = 0;
        Vec::new()
    } else {
        let fields: &[F] = match syntax.kind(source) {
            Some(K::Binary | K::Logical) => &[F::Left, F::Right],
            Some(K::Unary) => &[F::Arg],
            _ => &[],
        };
        fields.iter().filter_map(|&field| syntax.get(source, field)).map(|operand| {
            typed_expression(file, operand, receiver, depth + 1, query)
        }).collect()
    };
    Value::record(vec![
        ("kind".into(), Value::from(kind)),
        ("operator".into(), syntax.word(source, F::Op).map_or(Value::Null, |operator| Value::from(operator.to_string()))),
        ("relation".into(), relation(checker, &file.constrained_type_of(source), receiver, &mut Vec::new(), query)),
        ("operands".into(), Value::list(operands)),
    ])
}

fn node_id(value: &Value<'_>) -> Option<u32> {
    match value { Value::Node(node, _) => Some(*node), _ => None }
}

fn contexts(file: &NativeFile<'_>, site: u32, source: u32) -> Vec<(&'static str, Ty)> {
    let syntax = &file.syntax;
    let explicit = match syntax.kind(site) {
        Some(K::Declarator | K::Param | K::ClassProp | K::As | K::TypeAssertion) =>
            syntax.get(site, F::Type).map(|annotation| ("declared", file.constrained_type_of(annotation))),
        Some(K::Assign | K::AssignPattern) =>
            syntax.get(site, F::Left).map(|left| ("declared", file.constrained_type_of(left))),
        Some(K::Update) =>
            syntax.get(site, F::Arg).map(|target| ("declared", file.constrained_type_of(target))),
        _ => None,
    };
    let mut contexts = vec![explicit.unwrap_or_else(|| ("contextual", file.contextual_type_of(source)))];
    if syntax.is(site, K::ClassProp) {
        if let Some(name) = super::super::super::statics::property_name(syntax, site) {
            let mut class = syntax.parent(site);
            while class.is_some_and(|node| !syntax.is(node, K::Class)) {
                class = class.and_then(|node| syntax.parent(node));
            }
            if let Some(class) = class {
                let checker = &file.typing().checker;
                for heritage in syntax.all(class, F::Extends).chain(syntax.all(class, F::Implements)) {
                    contexts.push(("heritage", checker.member(&file.constrained_type_of(heritage), &name)));
                }
            }
        }
    }
    contexts
}

fn parts(ty: &Ty) -> &[Ty] {
    match ty { Ty::Union(parts) => parts, ty => std::slice::from_ref(ty) }
}

fn enum_ids(checker: &Checker<'_>, ty: &Ty) -> Vec<Value<'static>> {
    let Ty::Named(symbol, _) = ty else { return Vec::new() };
    if !checker.is_enum(symbol) { return Vec::new() }
    symbol.iter().map(|decl| Value::from(format!("{}:{}", decl.file.0, decl.decl))).collect()
}

fn domains(checker: &Checker<'_>, ty: &Ty) -> Vec<Value<'static>> {
    match ty {
        Ty::Prim(domain) | Ty::Lit(_, domain) => vec![Value::from(*domain)],
        Ty::Named(symbol, _) if checker.is_enum(symbol) => {
            let mut domains = Vec::new();
            for decl in symbol.iter() {
                let Some(summary) = checker.summary_of(decl.file) else { continue };
                for member in &summary.decls[decl.decl as usize].members {
                    let domain = if member.ty.is_some() { "string" } else { "number" };
                    if !domains.contains(&domain) { domains.push(domain) }
                }
            }
            domains.into_iter().map(Value::from).collect()
        }
        _ => Vec::new(),
    }
}

fn arm(checker: &Checker<'_>, ty: &Ty) -> Value<'static> {
    Value::record(vec![
        ("domains".into(), Value::list(domains(checker, ty))),
        ("enums".into(), Value::list(enum_ids(checker, ty))),
    ])
}

fn arguments(ty: &Ty) -> Vec<&Ty> {
    match ty {
        Ty::Named(_, args) => args.iter().collect(),
        Ty::Array(array) => vec![&array.element],
        Ty::Tuple(tuple) => tuple.iter().collect(),
        _ => Vec::new(),
    }
}

fn can_have_properties(checker: &Checker<'_>, ty: &Ty) -> bool {
    match ty {
        Ty::Named(symbol, _) => !checker.is_enum(symbol),
        Ty::Obj(..) | Ty::Struct(..) => true,
        Ty::Union(parts) | Ty::Inter(parts) => parts.iter().any(|part| can_have_properties(checker, part)),
        _ => false,
    }
}

fn relation(checker: &Checker<'_>, sender: &Ty, receiver: &Ty, path: &mut Vec<(Ty, Ty)>, query: &mut Query) -> Value<'static> {
    if !query.visit() { return Value::record(vec![("complete".into(), Value::Bool(false))]) }
    let sender = checker.readonly_source(sender).unwrap_or(sender);
    let receiver = checker.readonly_source(receiver).unwrap_or(receiver);
    if path.len() >= 64 {
        query.remaining = 0;
        return Value::record(vec![("complete".into(), Value::Bool(false))]);
    }
    let repeated = path.iter().any(|(left, right)| left.same_identity(sender) && right.same_identity(receiver));
    let complete = !repeated;
    let mut children = Vec::new();
    if complete {
        path.push((sender.clone(), receiver.clone()));
        let source_args = arguments(sender);
        let target_args = arguments(receiver);
        if source_args.len() == target_args.len() {
            for (source, target) in source_args.into_iter().zip(target_args) {
                if query.remaining == 0 { break }
                children.push(relation(checker, source, target, path, query));
            }
        }
        let source_element = checker.indexed_type(sender, &Ty::Prim("number"));
        let target_element = checker.indexed_type(receiver, &Ty::Prim("number"));
        if !source_element.is_opaque() && !target_element.is_opaque() {
            children.push(relation(checker, &source_element, &target_element, path, query));
        }
        if can_have_properties(checker, sender) && can_have_properties(checker, receiver) {
            let mut names = Vec::new();
            checker.member_names(receiver, &mut names);
            names.sort();
            names.dedup();
            for name in names {
                if query.remaining == 0 { break }
                let source = checker.member_resolution(sender, &name);
                if !source.declared { continue }
                let target = checker.member(receiver, &name);
                if !source.ty.is_opaque() && !target.is_opaque() {
                    children.push(relation(checker, &source.ty, &target, path, query));
                }
            }
        }
        path.pop();
    }
    let assignable = parts(sender).iter().map(|source| Value::list(parts(receiver).iter().map(|target| {
        checker.assignable(source, target).map_or(Value::Null, Value::Bool)
    }).collect())).collect();
    Value::record(vec![
        ("sender".into(), Value::list(parts(sender).iter().map(|ty| arm(checker, ty)).collect())),
        ("receiver".into(), Value::list(parts(receiver).iter().map(|ty| arm(checker, ty)).collect())),
        ("assignable".into(), Value::list(assignable)),
        ("children".into(), Value::list(children)),
        ("complete".into(), Value::Bool(complete && query.remaining > 0)),
    ])
}

fn enum_names(checker: &Checker<'_>, ty: &Ty, path: &mut Vec<Ty>, out: &mut Vec<String>, query: &mut Query) {
    if !query.visit() { return }
    if path.len() >= 64 { query.remaining = 0; return }
    if path.iter().any(|seen| seen.same_identity(ty)) { return }
    if let Ty::Named(symbol, _) = ty {
        if checker.is_enum(symbol) {
            for decl in symbol.iter() {
                if let Some(summary) = checker.summary_of(decl.file) {
                    let name = summary.decls[decl.decl as usize].name.to_string();
                    if !out.contains(&name) { out.push(name) }
                }
            }
            return;
        }
    }
    path.push(ty.clone());
    if let Ty::Union(parts) | Ty::Inter(parts) = ty {
        for part in parts { enum_names(checker, part, path, out, query) }
        path.pop();
        return;
    }
    for argument in arguments(ty) { enum_names(checker, argument, path, out, query) }
    let element = checker.indexed_type(ty, &Ty::Prim("number"));
    if !element.is_opaque() { enum_names(checker, &element, path, out, query) }
    if can_have_properties(checker, ty) {
        let mut names = Vec::new();
        checker.member_names(ty, &mut names);
        for name in names {
            if query.remaining == 0 { break }
            enum_names(checker, &checker.member(ty, &name), path, out, query);
        }
    }
    path.pop();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::tests::with_native_file;

    fn relation_count(value: &Value<'_>) -> usize {
        1 + value.key("children").items().iter().map(relation_count).sum::<usize>()
    }

    #[test]
    fn repeated_enum_tuple_members_keep_the_type_graph_bounded() {
        let source = "interface Number { valueOf(): number } interface Function { bind(): Function } enum Fruit { Apple, Banana } function f<T extends readonly [Fruit, Fruit]>() { const value: /*@*/T = /*@*/[Fruit.Apple, 1] as const; }";
        with_native_file(source, |file, nodes| {
            let checker = &file.typing().checker;
            let receiver = file.constrained_type_of(nodes[0]);
            let sender = file.constrained_type_of(nodes[1]);
            let mut query = Query::new();
            let graph = relation(checker, &sender, &receiver, &mut Vec::new(), &mut query);
            let mut names = Vec::new();
            enum_names(checker, &receiver, &mut Vec::new(), &mut names, &mut query);
            assert!(relation_count(&graph) < 64);
            assert!(query.remaining > 0);
            assert_eq!(names, ["Fruit"]);
        });
    }

    #[test]
    fn exhausted_queries_expose_an_incomplete_relation() {
        let mut query = Query { remaining: 0 };
        with_native_file("const x = /*@*/1;", |file, nodes| {
            let checker = &file.typing().checker;
            let graph = relation(checker, &file.type_of(nodes[0]), &Ty::Prim("number"), &mut Vec::new(), &mut query);
            assert_eq!(graph.key("complete"), Value::Bool(false));
            assert!(graph.key("sender").items().is_empty());
        });
    }
}
