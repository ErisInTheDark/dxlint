//! Assignment and destructuring owners for unsafe any transfers.

use super::*;
use crate::native::ts::kinds::{Field as F, Kind as K};
use crate::native::ts::statics;

fn finding(file: &NativeFile<'_>, node: u32, mode: &str, error: bool, sender: Option<&Ty>, receiver: Option<&Ty>) -> Value<'static> {
    let ck = &file.typing().checker;
    Value::Record(vec![
        ("offset".into(), Value::from(file.syntax.tree.node(node).start as usize)),
        ("mode".into(), Value::from(mode.to_string())),
        ("sender".into(), sender.map_or_else(|| Value::from(if error { "error typed" } else { "`any`" }), |ty| Value::from(format!("`{}`", ck.print(ty))))),
        ("receiver".into(), receiver.map_or(Value::Null, |ty| Value::from(format!("`{}`", ck.print(ty))))),
    ])
}

pub(super) fn findings(file: &NativeFile<'_>, node: u32) -> Value<'static> {
    let s = &file.syntax;
    if s.is(node, K::Prop) && s.parent(node).is_some_and(|object| file.bindings().write_sites.contains_key(&object)) {
        return Value::Null;
    }
    if s.is(node, K::Spread) && s.parent(node).is_some_and(|parent| s.is(parent, K::Array)) {
        let Some(arg) = s.get(node, F::Arg) else { return Value::Null };
        let ty = file.type_of(arg);
        let category = super::unsafe_types::category(file, arg);
        return if category.is_some() || matches!(ty, Ty::Array(element) if matches!(*element.element, Ty::Any)) {
            Value::List(vec![finding(file, node, "spread", category == Some("error"), None, None)])
        } else { Value::Null };
    }
    let (receiver_node, sender_node, annotation, compare, report) = match s.kind(node) {
        Some(K::Declarator | K::Param) => (s.get(node, F::Name), s.get(node, F::Init), s.get(node, F::Type), s.get(node, F::Type).is_some(), if s.is(node, K::Param) { s.get(node, F::Name).unwrap_or(node) } else { node }),
        Some(K::ClassProp) => (s.get(node, F::Key), s.get(node, F::Init), s.get(node, F::Type), s.get(node, F::Type).is_some(), node),
        Some(K::Assign | K::AssignPattern) if !s.is(node, K::Assign) || s.word(node, F::Op) == Some("=") =>
            (s.get(node, F::Left), s.get(node, F::Right), None, true, node),
        Some(K::Prop) => (s.get(node, F::Key), s.get(node, F::Value).or_else(|| s.get(node, F::Key)), None, true, node),
        Some(K::JsxAttribute) => {
            let expr = s.get(node, F::Value).filter(|&value| s.is(value, K::JsxExpression)).and_then(|value| s.get(value, F::Expr));
            (s.get(node, F::Name), expr, None, true, expr.unwrap_or(node))
        }
        _ => return Value::Null,
    };
    let (Some(receiver_node), Some(sender_node)) = (receiver_node, sender_node) else { return Value::Null };
    if s.is(node, K::Prop) && s.is(sender_node, K::AssignPattern) { return Value::Null }
    let ck = &file.typing().checker;
    let sender = file.type_of(sender_node);
    let mut receiver = annotation.map_or_else(|| file.type_of(receiver_node), |annotation| file.type_of(annotation));
    let mut known_unknown = matches!(receiver, Ty::IntrinsicUnknown) || annotation.is_some() && matches!(receiver, Ty::Unknown);
    if s.is(node, K::Prop) {
        if let Some(object) = s.parent(node) {
            let context = file.contextual_type_of(object);
            if !matches!(context, Ty::Unknown) {
                if let Some(name) = statics::property_name(s, node) {
                    receiver = ck.member(&context, &name);
                    known_unknown = matches!(receiver, Ty::Unknown | Ty::IntrinsicUnknown);
                }
            }
        }
    } else if compare && matches!(receiver, Ty::Unknown | Ty::IntrinsicUnknown) {
        known_unknown = matches!(s.kind(receiver_node), Some(K::Member | K::Index))
            || file.decl_of(receiver_node).and_then(|decl| s.parent(decl)).is_some_and(|owner| s.get(owner, F::Type).is_some());
    }
    let category = super::unsafe_types::category(file, sender_node);
    if category.is_some() && !known_unknown {
        return Value::List(vec![finding(file, report, "any", category == Some("error"), None, None)]);
    }
    if compare && !super::transfers::empty_map(file, sender_node) && sender.unsafe_assignment_to(&receiver) {
        return Value::List(vec![finding(file, report, "generic", false, Some(&sender), Some(&receiver))]);
    }
    let mut out = Vec::new();
    destructure(file, receiver_node, &sender, Some(sender_node), &mut out, 0);
    Value::List(out)
}

fn literal_source(file: &NativeFile<'_>, mut source: u32) -> Option<u32> {
    let s = &file.syntax;
    for _ in 0..32 {
        source = s.unparen(source);
        if !s.is(source, K::Ident) { return Some(source) }
        let owner = s.parent(file.decl_of(source)?)?;
        if s.get(owner, F::Type).is_some() { return None }
        source = s.get(owner, F::Init)?;
    }
    None
}

fn destructure(file: &NativeFile<'_>, receiver: u32, sender: &Ty, source: Option<u32>, out: &mut Vec<Value<'static>>, depth: usize) {
    if depth >= 32 { return }
    let s = &file.syntax;
    let source = source.and_then(|source| literal_source(file, source));
    if matches!(s.kind(receiver), Some(K::ArrayPattern | K::Array)) {
        if matches!(sender, Ty::Array(element) if matches!(&*element.element, Ty::Any)) {
            out.push(finding(file, receiver, "array", false, None, None));
            return;
        }
        let Ty::Tuple(elements) = sender else { return };
        for (index, receiver) in s.all(receiver, F::Element).enumerate() {
            if matches!(s.kind(receiver), Some(K::Hole | K::Rest)) { continue }
            let Some(sender) = elements.get(index) else { continue };
            let source = source.filter(|&source| s.is(source, K::Array)).and_then(|source| s.all(source, F::Element).nth(index));
            if matches!(sender, Ty::Any) {
                out.push(finding(file, receiver, "tuple", source.is_some_and(|source| super::unsafe_types::category(file, source) == Some("error")), None, None));
            } else {
                destructure(file, receiver, sender, source, out, depth + 1);
            }
        }
    } else if matches!(s.kind(receiver), Some(K::ObjectPattern | K::Object)) && !matches!(sender, Ty::Any | Ty::Unknown | Ty::IntrinsicUnknown) {
        for property in s.all(receiver, F::Property) {
            let (Some(key), Some(receiver)) = (statics::property_name(s, property), s.get(property, F::Value).or_else(|| s.get(property, F::Key))) else { continue };
            let sender = file.typing().checker.member(sender, &key);
            let source = source.filter(|&source| s.is(source, K::Object))
                .and_then(|source| s.all(source, F::Property).find(|&property| statics::property_name(s, property).as_deref() == Some(&key)))
                .and_then(|property| s.get(property, F::Value).or_else(|| s.get(property, F::Key)));
            if matches!(sender, Ty::Any) {
                out.push(finding(file, receiver, "object", source.is_some_and(|source| super::unsafe_types::category(file, source) == Some("error")), None, None));
            } else {
                destructure(file, receiver, &sender, source, out, depth + 1);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::engine::tests::native_linter;
    use std::path::Path;

    #[test]
    fn shorthand_and_nested_array_patterns_report_the_unsafe_target() {
        let linter = native_linter("rule no-unsafe-assignment\nrule no-unsafe-assignment:*");
        let code = "const { x } = { x: 1 } as { x: any }; const [[y]] = [1 as any];";
        let diagnostics = linter.lint(Path::new("/root/a.ts"), code);
        assert_eq!(diagnostics.len(), 2);
        assert_eq!(diagnostics[0].start, code.find("x }").unwrap());
        assert_eq!(diagnostics[1].start, code.find("[[y").unwrap() + 1);
    }

    #[test]
    fn unresolved_values_are_error_provenance_not_explicit_unknown() {
        let linter = native_linter("rule no-unsafe-assignment");
        let code = "const [x] = spooky; declare const known: unknown; const y = known;";
        assert_eq!(linter.lint(Path::new("/root/a.ts"), code).len(), 1);
    }

    #[test]
    fn constructor_context_infers_missing_but_not_explicit_any_arguments() {
        let linter = native_linter("rule no-unsafe-assignment:*");
        let code = "interface Box<T> { value: T; } interface Ctor { new<T = any>(): Box<T>; } declare const Maker: Ctor; const b: Box<string> = new Maker(); const c: Box<string> = new Maker<any>();";
        assert_eq!(linter.lint(Path::new("/root/a.ts"), code).len(), 1);
    }

    #[test]
    fn assignment_patterns_do_not_also_report_their_properties_as_value_transfers() {
        let linter = native_linter("rule no-unsafe-assignment\nrule no-unsafe-assignment:*");
        let code = "declare let x: any; ({ x } = { x: 1 } as { x: any }); [{ x }] = [{ x: 1 }] as [{ x: any }];";
        assert_eq!(linter.lint(Path::new("/root/a.ts"), code).len(), 2);
    }

    #[test]
    fn nested_destructuring_keeps_missing_value_provenance() {
        let linter = native_linter("rule no-unsafe-assignment\nrule no-unsafe-assignment:*");
        let code = "const [[[x]]] = [spooky]; const { x: { y: z } } = { x: spooky };";
        let diagnostics = linter.lint(Path::new("/root/a.ts"), code);
        assert_eq!(diagnostics.len(), 3);
        assert!(diagnostics.iter().all(|diagnostic| diagnostic.message.contains("error typed")));
    }
}
