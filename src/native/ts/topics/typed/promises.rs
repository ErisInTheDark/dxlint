//! Promise-returning function policies.

use super::*;
use crate::native::ts::kinds::{Field as F, Kind as K};
use crate::native::ts::summary::sig;
use crate::native::ts::statics;

fn name_groups(ty: &Ty, ck: &Checker<'_>, depth: usize) -> Vec<Vec<String>> {
    if depth >= 64 { return vec![Vec::new()] }
    match ty {
        Ty::Union(parts) | Ty::Inter(parts) => parts.iter().flat_map(|part| name_groups(part, ck, depth + 1)).collect(),
        Ty::Named(symbols, args) => {
            let mut names = Vec::new();
            let mut groups = Vec::new();
            for &reference in symbols.iter() {
                let Some(summary) = ck.summary_of(reference.file) else { continue };
                let decl = &summary.decls[reference.decl as usize];
                names.push(decl.name.to_string());
                let mut env = ck.decl_env(reference);
                env.tparams = ck.bind_tparams(&decl.type_params, args.clone(), &env);
                for base in ck.bases(decl) { groups.extend(name_groups(&ck.eval(base, &env), ck, depth + 1)); }
            }
            if groups.is_empty() { groups.push(Vec::new()); }
            for group in &mut groups { group.extend(names.iter().cloned()); }
            groups
        }
        _ => vec![Vec::new()],
    }
}

fn head(file: &NativeFile<'_>, node: u32) -> usize {
    let s = &file.syntax;
    let offset = file.function_head(node).unwrap_or(s.tree.node(node).start as usize);
    let holder = s.parent(node).filter(|&parent| s.is(parent, K::ClassProp)).unwrap_or(node);
    if s.all(holder, F::Decorator).next().is_none() { return offset }
    s.kids(holder).filter(|(field, _)| matches!(field, Some(F::Modifier | F::Key | F::Kind))).map(|(_, child)| s.tree.node(child).start as usize).min().unwrap_or(offset)
}

pub(super) fn function(file: &NativeFile<'_>, node: u32) -> Value<'static> {
    let s = &file.syntax;
    if s.get(node, F::Body).is_none() || s.has_word(node, F::Modifier, "async") { return Value::Null }
    let parent = s.parent(node);
    let kind = match s.kind(node) {
        Some(K::Arrow) => "arrow",
        Some(K::Method) if matches!(s.word(node, F::Kind), Some("get" | "set"))
            || parent.is_some_and(|parent| s.is(parent, K::Class)) && !s.has_word(node, F::Modifier, "static")
                && s.get(node, F::Key).is_some_and(|key| !s.is(key, K::Computed)) && statics::property_name(s, node).as_deref() == Some("constructor") => return Value::Null,
        Some(K::Method) if parent.is_some_and(|parent| s.is(parent, K::Class)) => "method",
        Some(K::Method) => "expression",
        Some(K::Function) if parent.is_some_and(|parent| matches!(s.kind(parent), Some(K::Program | K::Block | K::Export | K::Case | K::If | K::Labeled))) => "declaration",
        Some(K::Function) => "expression",
        _ => return Value::Null,
    };
    let ck = &file.typing().checker;
    let signature = sig(s, node);
    let mut returns = Vec::new();
    if let Some(name) = s.get(node, F::Name) {
        let named = file.type_of(name);
        if matches!(&named, Ty::Fn(callable) if callable.sigs.len() > 1) { returns = ck.declared_returns(&named); }
    }
    if returns.is_empty() {
        returns.push(signature.ret.as_ref().map_or_else(
            || file.own_return(&signature, ck.file).unwrap_or(Ty::Unknown),
            |ret| ck.eval(ret, &file.env_at(node)),
        ));
    }
    Value::Record(vec![
        ("kind".into(), Value::from(kind)),
        ("explicit".into(), Value::Bool(s.get(node, F::ReturnType).is_some())),
        ("offset".into(), Value::from(head(file, node))),
        ("returns".into(), Value::List(returns.iter().map(|ty| Value::Record(vec![
            ("unknown".into(), Value::Bool(matches!(ty, Ty::Any | Ty::Unknown | Ty::IntrinsicUnknown))),
            ("union".into(), Value::Bool(matches!(ty, Ty::Union(_)))),
            ("groups".into(), Value::List(name_groups(ty, ck, 0).into_iter().map(|group| Value::List(group.into_iter().map(Value::from).collect())).collect())),
        ])).collect())),
    ])
}

pub(super) fn rejection_handler(file: &NativeFile<'_>, call: u32) -> Option<(u32, &'static str)> {
    let s = &file.syntax;
    let callee = s.get(call, F::Callee)?;
    let object = s.get(callee, F::Object)?;
    let property = statics::property_name(s, callee).or_else(|| s.get(callee, F::Index).and_then(|index| file.static_string(index)));
    let (index, method) = match property.as_deref() {
        Some("catch") => (0, "catch"),
        Some("then") => (1, "then"),
        _ => return None,
    };
    let arguments: Vec<_> = s.all(call, F::Arg).take(index + 1).collect();
    if arguments.len() != index + 1 || arguments.iter().any(|&arg| s.is(arg, K::Spread)) { return None }
    file.typing().checker.is_thenable(&file.type_of(object)).then_some((arguments[index], method))
}

pub(super) fn unsafe_catch(file: &NativeFile<'_>, node: u32) -> Value<'static> {
    let Some((handler, method)) = rejection_handler(file, node) else { return Value::List(Vec::new()) };
    let s = &file.syntax;
    let mut stack = vec![handler];
    let mut findings = Vec::new();
    while let Some(node) = stack.pop() {
        let node = s.unparen(node);
        match s.kind(node) {
            Some(K::Logical) => {
                stack.extend(s.get(node, F::Right));
                stack.extend(s.get(node, F::Left));
            }
            Some(K::Conditional) => {
                stack.extend(s.get(node, F::Alternate));
                stack.extend(s.get(node, F::Consequent));
            }
            Some(K::Sequence) => stack.extend(s.all(node, F::Expr).last()),
            Some(K::Arrow | K::Function) => {
                let Some(parameter) = s.all(node, F::Param).next() else { continue };
                let Some(name) = s.get(parameter, F::Name) else { continue };
                let mut ty = s.get(parameter, F::Type).map_or_else(
                    || file.typing().checker.contextual_parameter(&file.contextual_type_of(node), 0),
                    |annotation| file.type_of(annotation),
                );
                if s.is(name, K::Rest) {
                    ty = match ty {
                        Ty::Array(element) => *element.element,
                        Ty::Tuple(elements) => elements.first().cloned().unwrap_or(Ty::Never),
                        _ => Ty::Never,
                    };
                }
                if matches!(ty, Ty::Unknown | Ty::IntrinsicUnknown) { continue }
                findings.push(Value::Record(vec![
                    ("offset".into(), Value::from(s.tree.node(parameter).start as usize)),
                    ("method".into(), Value::from(method)),
                    ("pattern".into(), Value::from(match s.kind(name) { Some(K::ArrayPattern) => "array", Some(K::ObjectPattern) => "object", _ => "" })),
                ]));
            }
            _ => {}
        }
    }
    Value::List(findings)
}

pub(super) fn missing_await(file: &NativeFile<'_>, node: u32) -> Value<'static> {
    let s = &file.syntax;
    if !s.has_word(node, F::Modifier, "async") { return Value::Null }
    let Some(body) = s.get(node, F::Body) else { return Value::Null };
    if s.is(body, K::Block) && s.all(body, F::Body).next().is_none() { return Value::Null }
    let typed = super::is_typescript(file);
    let generator = s.has_word(node, F::Modifier, "*");
    if !typed && generator { return Value::Null }
    let ck = &file.typing().checker;
    if typed && !s.is(body, K::Block) && ck.is_thenable(&file.type_of(body)) { return Value::Null }
    let mut stack = vec![body];
    while let Some(node) = stack.pop() {
        match s.kind(node) {
            Some(K::Function | K::Arrow | K::Method) => continue,
            Some(K::Await) => return Value::Null,
            Some(K::ForOf) if s.has_word(node, F::Modifier, "await") => return Value::Null,
            Some(K::Var) if s.word(node, F::Kind) == Some("await using") => return Value::Null,
            Some(K::Return) if typed && s.get(node, F::Arg).is_some_and(|arg| ck.is_thenable(&file.type_of(arg))) => return Value::Null,
            Some(K::Yield) if typed && generator => {
                if let Some(arg) = s.get(node, F::Arg) {
                    let delegated = file.tokens(node).get(1) == Some(&"*");
                    if if delegated { super::awaitability::has_async_symbol(file, arg, "asyncIterator") } else { ck.is_thenable(&file.type_of(arg)) } {
                        return Value::Null;
                    }
                }
            }
            _ => {}
        }
        stack.extend(s.kids(node).map(|(_, child)| child));
    }
    let name = file.function_name_with_kind(node).unwrap_or_else(|| "async function".into());
    Value::Record(vec![
        ("name".into(), Value::from(format!("{}{}", name[..1].to_uppercase(), &name[1..]))),
        ("offset".into(), Value::from(head(file, node))),
    ])
}

#[cfg(test)]
mod tests {
    use crate::engine::tests::native_linter;
    use std::path::Path;

    #[test]
    fn explicit_union_returns_and_overloads_differ_from_inferred_hybrids() {
        let linter = native_linter("rule promise-function-async");
        let code = "interface Promise<T> {} declare const p: Promise<number>; function inferred(flag: boolean) { return flag ? p : 1; } function explicit(flag: boolean): Promise<number> | number { return flag ? p : 1; } function overloaded(): Promise<number>; function overloaded(flag: boolean): number; function overloaded(flag?: boolean) { return p; }";
        let diagnostics = linter.lint(Path::new("/root/a.ts"), code);
        assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
        assert_eq!(diagnostics[0].start, code.find("function inferred").unwrap());
    }

    #[test]
    fn function_options_skip_only_the_selected_owner_kind() {
        let linter = native_linter("rule promise-function-async { set checkArrowFunctions false }");
        let code = "interface Promise<T> {} declare const p: Promise<number>; const arrow = () => p; const expression = function () { return p; }; function declared() { return p; } const methods = { method() { return p; }, constructor() { return p; }, get value() { return p; } }; class C { constructor() { return p; } static constructor() { return p; } }";
        assert_eq!(linter.lint(Path::new("/root/a.ts"), code).len(), 5);
    }

    #[test]
    fn catch_callbacks_check_each_inline_branch_but_not_existing_handlers_or_spreads() {
        let linter = native_linter("rule use-unknown-in-catch-callback-variable");
        let code = "interface Like { then(ok: (v: unknown) => void, bad: (e: any) => void): Like; catch(bad: (e: any) => void): Like; } declare const promise: Like; declare const flag: boolean; declare const existing: (e: any) => void; promise.catch(flag ? (e: unknown) => {} : (e: any) => {}); promise.then((e: any) => {}, (e: any) => {}); promise.catch(existing); promise.catch(...[(e: any) => {}]);";
        assert_eq!(linter.lint(Path::new("/root/a.ts"), code).len(), 2);
    }

    #[test]
    fn rest_callbacks_compare_the_first_rest_element_not_the_container() {
        let linter = native_linter("rule use-unknown-in-catch-callback-variable");
        let code = "interface Like { then(ok: (v: unknown) => void, bad: (e: any) => void): Like; catch(bad: (e: any) => void): Like; } declare const promise: Like; promise.catch((...e: unknown[]) => {}); promise.catch((...e: [unknown, any]) => {}); promise.catch((...e: any[]) => {}); promise.catch(({message}: {message: string}) => {});";
        assert_eq!(linter.lint(Path::new("/root/a.ts"), code).len(), 2);
    }

    #[test]
    fn async_functions_do_not_borrow_awaits_or_returns_from_nested_functions() {
        let linter = native_linter("rule require-await");
        let code = "interface Like { then(ok: (v: number) => void): Like; } declare const p: Like; async function returned() { return p; } async function outer() { const inner = async () => { await 1; }; return 1; } async function bare() { return; } async function empty() {} async function iterated(xs: any) { for await (const x of xs) {} } async function disposed() { await using x = {}; }";
        let diagnostics = linter.lint(Path::new("/root/a.ts"), code);
        assert_eq!(diagnostics.len(), 2, "{diagnostics:?}");
        assert_eq!(diagnostics[0].start, code.find("async function outer").unwrap());
        assert_eq!(diagnostics[1].start, code.find("async function bare").unwrap());
    }

    #[test]
    fn javascript_async_generators_are_exempt_but_returned_calls_do_not_replace_awaits() {
        let linter = native_linter("rule require-await");
        let code = "async function* generator() { yield 1; } async function returned() { return Promise.resolve(1); } async function* nested() { async function inner() { return 1; } yield 1; }";
        assert_eq!(linter.lint(Path::new("/root/a.js"), code).len(), 2);
    }
}
