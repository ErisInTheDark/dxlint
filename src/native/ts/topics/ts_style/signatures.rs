//! Syntactic overload pairs that can share one signature.

use std::collections::HashMap;
use crate::expr::Value;
use crate::native::ts::facts::NativeFile;
use crate::native::ts::kinds::{Field as F, Kind as K};
use crate::native::ts::lexer::{Lexer, Tok};
use crate::native::ts::syntax::Syntax;

fn parameters(s: &Syntax, node: u32) -> Vec<u32> { s.all(node, F::Param).collect() }

fn rest(s: &Syntax, node: u32) -> bool {
    s.get(node, F::Name).is_some_and(|n| s.is(n, K::Rest))
}

fn optional(s: &Syntax, node: u32) -> bool { s.get(node, F::Optional).is_some() }

fn sigils_equal(s: &Syntax, a: u32, b: u32) -> bool {
    rest(s, a) == rest(s, b) && optional(s, a) == optional(s, b)
}

fn types_equal(s: &Syntax, a: Option<u32>, b: Option<u32>) -> bool {
    a.map(|n| s.text(bare_type(s, n))) == b.map(|n| s.text(bare_type(s, n)))
}

fn bare_type(s: &Syntax, mut node: u32) -> u32 {
    while s.is(node, K::ParenType) {
        let Some(inner) = s.get(node, F::Type) else { break };
        node = inner;
    }
    s.unparen(node)
}

fn parameter_name<'s>(s: &Syntax<'s>, node: u32) -> Option<&'s str> {
    let mut name = s.get(node, F::Name)?;
    if s.is(name, K::Rest) { name = s.get(name, F::Arg)?; }
    matches!(s.kind(name), Some(K::Ident | K::This)).then(|| s.text(name))
}

fn this_void(s: &Syntax, parameters: &[u32]) -> bool {
    parameters.first().is_some_and(|&n| parameter_name(s, n) == Some("this") && s.word(n, F::Type) == Some("void"))
}

fn outer_type_use(s: &Syntax, node: u32, names: &[&str]) -> bool {
    parameters(s, node).into_iter().any(|p| {
        let mut at = s.get(p, F::Type);
        while let Some(n) = at {
            match s.kind(n) {
                Some(K::TypeRef) => return s.word(n, F::Name).is_some_and(|name| names.contains(&name)),
                Some(K::ArrayType | K::ParenType) => at = s.get(n, F::Type),
                _ => return false,
            }
        }
        false
    })
}

fn signature_head(s: &Syntax, node: u32) -> usize {
    if s.is(node, K::Method) {
        if let Some(key) = s.get(node, F::Key) {
            let end = s.tree.node(key).end as usize;
            return end + Lexer::new(&s.src[end..]).next_token().start as usize;
        }
    }
    s.tree.node(node).start as usize
}

fn jsdoc<'s>(s: &Syntax<'s>, node: u32) -> Option<&'s str> {
    let target = s.parent(node).filter(|&n| s.is(n, K::Export)).unwrap_or(node);
    let before = &s.src[..s.tree.node(target).start as usize];
    let end = before.rfind("*/")? + 2;
    let start = before[..end].rfind("/*")?;
    (Lexer::new(&before[end..]).next_token().tok == Tok::Eof).then_some(&before[start + 2..end - 2])
}

fn union_members(s: &Syntax, node: u32, out: &mut Vec<u32>) {
    let node = bare_type(s, node);
    if s.is(node, K::Union) {
        for child in s.all(node, F::Type) { union_members(s, child, out); }
    } else if !out.iter().any(|&n| s.text(n) == s.text(node)) {
        out.push(node);
    }
}

fn union_text(s: &Syntax, a: u32, b: u32) -> String {
    let mut members = Vec::new();
    for node in [s.get(a, F::Type), s.get(b, F::Type)].into_iter().flatten() {
        union_members(s, node, &mut members);
    }
    // Function/conditional members need parentheses in the suggested union.
    members.into_iter().map(|node| {
        let text = s.text(node);
        if matches!(s.kind(node), Some(K::FnType | K::ConditionalType)) {
            format!("({text})")
        } else { text.to_string() }
    }).collect::<Vec<_>>().join(" | ")
}

fn compare(s: &Syntax, a: u32, b: u32, count: usize, outer_names: &[&str]) -> Option<Value<'static>> {
    if !types_equal(s, s.get(a, F::ReturnType), s.get(b, F::ReturnType)) || outer_type_use(s, a, outer_names) != outer_type_use(s, b, outer_names) { return None; }
    let type_a: Vec<_> = s.all(a, F::TypeParam).collect();
    let type_b: Vec<_> = s.all(b, F::TypeParam).collect();
    if type_a.len() != type_b.len() || type_a.iter().zip(&type_b).any(|(&a, &b)| {
        s.word(a, F::Name) != s.word(b, F::Name) || !types_equal(s, s.get(a, F::Constraint), s.get(b, F::Constraint))
    }) { return None; }
    let pa = parameters(s, a);
    let pb = parameters(s, b);
    if this_void(s, &pa) || this_void(s, &pb) { return None; }
    let same_names = pa.iter().zip(&pb).all(|(&a, &b)| {
        rest(s, a) != rest(s, b) || parameter_name(s, a) == parameter_name(s, b)
    });
    let (offset, other, ending) = if pa.len() == pb.len() {
        let differences: Vec<_> = pa.iter().zip(&pb).enumerate().filter(|(_, (a, b))| {
            !sigils_equal(s, **a, **b) || !types_equal(s, s.get(**a, F::Type), s.get(**b, F::Type))
        }).map(|(index, _)| index).collect();
        if differences.is_empty() {
            (signature_head(s, b), signature_head(s, a), " with identical parameters.".to_string())
        } else if differences.len() == 1 {
            let index = differences[0];
            if !sigils_equal(s, pa[index], pb[index]) || rest(s, pa[index]) { return None; }
            (s.tree.node(pb[index]).start as usize, s.tree.node(pa[index]).start as usize, format!(" taking `{}`.", union_text(s, pa[index], pb[index])))
        } else { return None; }
    } else {
        if pa.first().is_some_and(|&n| parameter_name(s, n) == Some("this")) != pb.first().is_some_and(|&n| parameter_name(s, n) == Some("this")) { return None; }
        let (short, long, short_sig) = if pa.len() < pb.len() { (&pa, &pb, a) } else { (&pb, &pa, b) };
        if short.last().is_some_and(|&n| rest(s, n))
            || short.iter().zip(long).any(|(&a, &b)| !types_equal(s, s.get(a, F::Type), s.get(b, F::Type)))
            || long.iter().skip(short.len() + 1).any(|&n| !optional(s, n) && !rest(s, n))
        { return None; }
        let extra = *long.last()?;
        (s.tree.node(extra).start as usize, signature_head(s, short_sig), format!(" with {} parameter.", if rest(s, extra) { "a rest" } else { "an optional" }))
    };
    let start = if count == 2 { "These overloads".to_string() } else { format!("This overload and the one on line {}", s.src[..other].bytes().filter(|&b| b == b'\n').count() + 1) };
    Some(Value::record(vec![
        ("offset".into(), Value::from(offset)),
        ("message".into(), Value::from(format!("{start} can be combined into one signature{ending}"))),
        ("same_names".into(), Value::Bool(same_names)),
        ("same_docs".into(), Value::Bool(jsdoc(s, a) == jsdoc(s, b))),
    ]))
}

pub(super) fn unifiable_signatures(file: &NativeFile, node: u32) -> Value<'static> {
    let s = &file.syntax;
    let mut groups: HashMap<String, Vec<u32>> = HashMap::new();
    let body = if s.is(node, K::Namespace) { s.get(node, F::Body).unwrap_or(node) } else { node };
    for (_, child) in s.kids(body) {
        let signature = if s.is(child, K::Export) { s.get(child, F::Decl).unwrap_or(child) } else { child };
        let key = match s.kind(signature) {
            Some(K::Function) if s.get(signature, F::Body).is_none() => s.word(signature, F::Name).unwrap_or("default").to_string(),
            Some(K::Method | K::MethodSig) if s.get(signature, F::Body).is_none() && !matches!(s.word(signature, F::Kind), Some("get" | "set")) => {
                let Some(key) = s.get(signature, F::Key) else { continue };
                format!("{:?}:{}:{}", s.kind(key), s.has_word(signature, F::Modifier, "static"), s.text(key))
            }
            Some(K::CallSig) => s.word(signature, F::Kind).unwrap_or("call").to_string(),
            _ => continue,
        };
        groups.entry(key).or_default().push(signature);
    }
    let outer_names: Vec<_> = s.all(node, F::TypeParam).filter_map(|n| s.word(n, F::Name)).collect();
    let mut result = Vec::new();
    for signatures in groups.values() {
        for i in 0..signatures.len() {
            for j in i + 1..signatures.len() {
                if let Some(failure) = compare(s, signatures[i], signatures[j], signatures.len(), &outer_names) { result.push(failure); }
            }
        }
    }
    Value::list(result)
}

pub(super) fn nonadjacent_signatures(file: &NativeFile, node: u32) -> Value<'static> {
    let s = &file.syntax;
    let mut last = None;
    let mut seen = Vec::new();
    let mut result = Vec::new();
    for (field, member) in s.kids(node) {
        if !matches!(field, Some(F::Member | F::Body)) { continue; }
        let declaration = if s.is(member, K::Export) { s.get(member, F::Decl).unwrap_or(member) } else { member };
        let info = match s.kind(declaration) {
            Some(K::Function) => s.word(declaration, F::Name).map(|name| (name.to_string(), "normal", None, false)),
            Some(K::Method | K::MethodSig) => {
                let (name, kind) = super::class_styles::member_name(s, declaration);
                let static_ = s.is(declaration, K::Method).then(|| s.has_word(declaration, F::Modifier, "static"));
                Some((name, kind, static_, false))
            }
            Some(K::CallSig) => Some((if s.word(declaration, F::Kind) == Some("new") { "new" } else { "call" }.to_string(), "normal", None, s.word(declaration, F::Kind) != Some("new"))),
            _ => None,
        };
        let Some(info) = info else { last = None; continue };
        if seen.contains(&info) && last.as_ref() != Some(&info) {
            result.push(Value::record(vec![
                ("offset".into(), Value::from(s.tree.node(member).start as usize)),
                ("name".into(), Value::from(format!("{}{}", if info.2 == Some(true) { "static " } else { "" }, info.0))),
            ]));
        } else if !seen.contains(&info) {
            seen.push(info.clone());
        }
        last = Some(info);
    }
    Value::list(result)
}

fn this_type(s: &Syntax, node: u32) -> Option<u32> {
    if s.is(node, K::TypeLit) { return None; }
    if s.is(node, K::TypeKeyword) && s.text(node) == "this" { return Some(node); }
    s.kids(node).find_map(|(_, n)| this_type(s, n))
}

pub(super) fn function_type_candidate(file: &NativeFile, node: u32) -> Value<'static> {
    let s = &file.syntax;
    let Some(parent) = s.parent(node).filter(|&n| matches!(s.kind(n), Some(K::Interface | K::TypeLit))) else { return Value::Null };
    if s.get(node, F::ReturnType).is_none() || s.all(parent, F::Member).count() != 1 { return Value::Null; }
    if s.is(parent, K::Interface) {
        let extends: Vec<_> = s.all(parent, F::Extends).collect();
        if !extends.is_empty() && !(extends.len() == 1 && (s.text(extends[0]) == "Function" || s.word(extends[0], F::Name) == Some("Function"))) { return Value::Null; }
        if let Some(this) = this_type(s, parent) {
            return Value::record(vec![
                ("offset".into(), Value::from(s.tree.node(this).start as usize)),
                ("message".into(), Value::from(format!("`this` refers to the function type '{}', did you intend to use a generic `this` parameter like `<Self>(this: Self, ...) => Self` instead?", s.word(parent, F::Name).unwrap_or("")))),
            ]);
        }
    }
    Value::record(vec![
        ("offset".into(), Value::from(s.tree.node(node).start as usize)),
        ("message".into(), Value::from(format!("{} only has a call signature, you should use a function type instead.", if s.is(parent, K::Interface) { "Interface" } else { "Type literal" }))),
    ])
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use crate::engine::tests::native_linter;

    #[test]
    fn overloads_unify_one_difference_not_rest_or_two_differences() {
        let linter = native_linter("rule unified-signatures");
        assert_eq!(linter.lint(Path::new("/root/file.ts"), "function f(x: string): void; function f(x: number): void;").len(), 1);
        for text in [
            "function f(x: string, y: string): void; function f(x: number, y: number): void;",
            "function f(...x: string[]): void; function f(...x: number[]): void;",
            "function f(this: void, x: string): void; function f(this: void, x: number): void;",
        ] {
            assert!(linter.lint(Path::new("/root/file.ts"), text).is_empty(), "{text}");
        }
    }

    #[test]
    fn function_overloads_cannot_cross_other_statements() {
        let linter = native_linter("rule adjacent-overload-signatures");
        assert_eq!(linter.lint(Path::new("/root/file.ts"), "export function f(x: string): void; const other = 1; export function f(x: number): void;").len(), 1);
    }

    #[test]
    fn function_types_include_construct_signatures_and_report_this_types() {
        let linter = native_linter("rule prefer-function-type");
        assert_eq!(linter.lint(Path::new("/root/file.ts"), "interface A { (): this; } type B = { new (): object; };").len(), 2);
    }
}
