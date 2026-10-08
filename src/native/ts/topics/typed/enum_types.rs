//! Type relations at contextual and explicitly declared transfer sites.

use crate::expr::Value;
use crate::native::ts::facts::NativeFile;
use crate::native::ts::kinds::{Field as F, Kind as K};
use crate::native::ts::types::{Checker, Ty};
use std::cell::RefCell;

#[derive(Default)]
struct NameGraphs {
    graphs: RefCell<Vec<(Ty, Vec<String>, usize)>>,
    #[cfg(test)]
    expansions: std::cell::Cell<usize>,
}

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
    let transfers = file.argument_types(source, false).map(|pairs| ("argument", pairs))
        .or_else(|| file.return_types(source).map(|pairs| ("return", pairs)))
        .or_else(|| file.spread_types(source).map(|pairs| ("contextual", pairs)));
    let contexts = if let Some((origin, pairs)) = transfers {
        pairs.into_iter().map(|(sender, receiver)| (origin, sender, receiver)).collect()
    } else {
        let sender = file.constrained_type_of(source);
        file.expected_contracts(site, source).into_iter().map(|(origin, receiver)| (origin, sender.clone(), receiver)).collect::<Vec<_>>()
    };
    Value::list(contexts.into_iter().map(|(origin, sender, receiver)| {
        let expression = checker.with_relation_work(|| typed_expression(file, source, &receiver, Some(&sender), 0, &mut query));
        let names = receiver_names(file, &receiver, &mut query);
        Value::record(vec![
            ("origin".into(), Value::from(origin)),
            ("relation".into(), expression.key("relation")),
            ("expression".into(), expression),
            ("enum_names".into(), Value::list(names.into_iter().map(Value::from).collect())),
            ("complete".into(), Value::Bool(query.remaining > 0)),
        ])
    }).collect())
}

fn typed_expression(file: &NativeFile<'_>, source: u32, receiver: &Ty, sender: Option<&Ty>, depth: usize, query: &mut Query) -> Value<'static> {
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
            typed_expression(file, operand, receiver, None, depth + 1, query)
        }).collect()
    };
    Value::record(vec![
        ("kind".into(), Value::from(kind)),
        ("operator".into(), syntax.word(source, F::Op).map_or(Value::Null, |operator| Value::from(operator.to_string()))),
        ("relation".into(), relation(checker, sender.unwrap_or(&file.constrained_type_of(source)), receiver, &mut Vec::new(), query)),
        ("operands".into(), Value::list(operands)),
    ])
}

fn node_id(value: &Value<'_>) -> Option<u32> {
    match value { Value::Node(node, _) => Some(*node), _ => None }
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
    let identical = !sender.is_opaque() && sender.same_identity(receiver);
    let mut children = Vec::new();
    if complete && !identical {
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
        ("identity".into(), Value::Bool(identical)),
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

fn receiver_names(file: &NativeFile<'_>, ty: &Ty, query: &mut Query) -> Vec<String> {
    let checker = &file.typing().checker;
    if !can_have_properties(checker, ty) || !ty.same_identity(ty) {
        let mut names = Vec::new();
        enum_names(checker, ty, &mut Vec::new(), &mut names, query);
        return names;
    }
    let graphs = file.cached(|_| NameGraphs::default());
    if let Some((_, names, cost)) = graphs.graphs.borrow().iter().find(|(cached, _, _)| cached.same_identity(ty)) {
        query.remaining = query.remaining.saturating_sub(*cost);
        return names.clone();
    }
    #[cfg(test)]
    graphs.expansions.update(|count| count + 1);
    let mut names = Vec::new();
    let mut graph = Query::new();
    let budget = graph.remaining;
    enum_names(checker, ty, &mut Vec::new(), &mut names, &mut graph);
    let cost = budget - graph.remaining;
    query.remaining = query.remaining.saturating_sub(cost);
    graphs.graphs.borrow_mut().push((ty.clone(), names.clone(), cost));
    names
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::native::ts::types::StructProperty;
    use crate::engine::tests::with_native_file;

    #[test]
    fn enum_argument_contracts_cover_calls_constructors_tags_and_spreads() {
        let linter = crate::engine::tests::native_linter("rule no-unsafe-enum-assignment");
        for calls in [
            "function take(value: Fruit): void {} take(Fruit.Apple); take(1);",
            "class Basket { constructor(public value: Fruit) {} } new Basket(Fruit.Apple); new Basket(1);",
            "interface TemplateStringsArray {} declare function tag(strings: TemplateStringsArray, value: Fruit): string; tag`${Fruit.Apple}`; tag`${1}`;",
            "function take(...values: [Fruit, Fruit]): void {} take(Fruit.Apple, Fruit.Apple); take(Fruit.Apple, 1);",
            "function take(value: Fruit): void {} take(...[Fruit.Apple]); take(...[1]);",
            "function take(...values: Fruit[]): void {} declare const safe: Fruit[]; declare const unsafe: number[]; take(...safe); take(...unsafe);",
        ] {
            let source = format!("enum Fruit {{ Apple }} {calls}");
            assert_eq!(linter.lint(std::path::Path::new("/root/a.ts"), &source).len(), 1, "{calls}");
        }
    }

    #[test]
    fn enum_return_contracts_cover_bodies_arrows_and_async_values() {
        let linter = crate::engine::tests::native_linter("rule no-unsafe-enum-assignment");
        for returns in [
            "function safe(): Fruit { return Fruit.Apple; } function unsafe(): Fruit { return 1; }",
            "const safe = (): Fruit => Fruit.Apple; const unsafe = (): Fruit => 1;",
            "async function safe(): Promise<Fruit> { return Fruit.Apple; } async function unsafe(): Promise<Fruit> { return 1; }",
            "declare const safeValue: Promise<Fruit>; declare const unsafeValue: Promise<number>; async function safe(): Promise<Fruit> { return safeValue; } async function unsafe(): Promise<Fruit> { return unsafeValue; }",
        ] {
            let source = format!("interface Promise<T> {{ then(callback: (value: T) => unknown): unknown }} enum Fruit {{ Apple }} {returns}");
            assert_eq!(linter.lint(std::path::Path::new("/root/a.ts"), &source).len(), 1, "{returns}");
        }
    }

    #[test]
    fn enum_call_and_return_containers_report_each_unsafe_value_once() {
        let linter = crate::engine::tests::native_linter("rule no-unsafe-enum-assignment");
        let source = "enum Fruit { Apple } function take(value: { fruit: Fruit }): void {} take({ fruit: Fruit.Apple }); take({ fruit: 1 }); function safe(): { fruit: Fruit } { return { fruit: Fruit.Apple }; } function unsafe(): { fruit: Fruit } { return { fruit: 1 }; }";
        assert_eq!(linter.lint(std::path::Path::new("/root/a.ts"), source).len(), 2);
    }

    fn relation_count(value: &Value<'_>) -> usize {
        1 + value.key("children").items().iter().map(relation_count).sum::<usize>()
    }

    #[test]
    fn receiver_name_reuse_preserves_query_completeness() {
        with_native_file("enum Fruit { Apple } const x = /*@*/Fruit.Apple;", |file, nodes| {
            let ty = Ty::Struct(vec![StructProperty { name: "value".into(), ty: file.type_of(nodes[0]), origin: None }].into());
            let mut direct = Query::new();
            let mut expected = Vec::new();
            enum_names(&file.typing().checker, &ty, &mut Vec::new(), &mut expected, &mut direct);
            for _ in 0..2 {
                let mut query = Query::new();
                assert_eq!(receiver_names(file, &ty, &mut query), expected);
                assert_eq!(query.remaining, direct.remaining);
            }
            assert_eq!(file.cached(|_| NameGraphs::default()).expansions.get(), 1);
            let mut partial = Query { remaining: 1 };
            receiver_names(file, &ty, &mut partial);
            assert_eq!(partial.remaining, 0);
        });
    }

    #[test]
    fn identical_property_bags_share_one_complete_relation() {
        with_native_file("enum Fruit { Apple } const x = /*@*/Fruit.Apple;", |file, nodes| {
            let checker = &file.typing().checker;
            let ty = Ty::Struct((0..128).map(|index| StructProperty { name: format!("p{index}").into(), ty: file.type_of(nodes[0]), origin: None }).collect());
            let mut query = Query::new();
            let graph = relation(checker, &ty, &ty, &mut Vec::new(), &mut query);
            assert_eq!(relation_count(&graph), 1);
            assert_eq!(graph.key("complete"), Value::Bool(true));
        });
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
