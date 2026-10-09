//! Unsafe call arguments, including consumption of tuple and array rest parameters.

use super::*;
use crate::native::ts::kinds::{Field as F, Kind as K};
use crate::native::ts::types::Parameters;

#[cfg(test)]
thread_local! {
    static PARAMETER_WORK: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

fn finding(file: &NativeFile<'_>, node: u32, mode: &str, sender: &Ty, receiver: Option<&Ty>, error: bool) -> Value<'static> {
    let ck = &file.typing().checker;
    Value::record(vec![
        ("offset".into(), Value::from(file.syntax.tree.node(node).start as usize)),
        ("mode".into(), Value::from(mode.to_string())),
        ("error".into(), Value::Bool(error)),
        ("sender".into(), if error { Value::Null } else { Value::from(ck.print(sender)) }),
        ("receiver".into(), receiver.map_or(Value::Null, |ty| Value::from(ck.print(ty)))),
    ])
}

pub(super) fn findings(file: &NativeFile<'_>, node: u32) -> Value<'static> {
    let s = &file.syntax;
    let tagged = s.is(node, K::TaggedTemplate);
    let Some(callee) = s.get(node, if tagged { F::Tag } else { F::Callee }) else { return Value::Null };
    let callee = file.type_of(callee);
    if matches!(callee, Ty::Any) { return Value::Null }
    let args: Vec<_> = if tagged {
        s.get(node, F::Quasi).map_or_else(Vec::new, |quasi| s.all(quasi, F::Expr).collect())
    } else { s.all(node, F::Arg).collect() };
    let actuals: Vec<_> = args.iter().map(|&arg| file.type_of(arg)).collect();
    if !args.iter().any(|&arg| s.is(arg, K::Spread)) && !actuals.iter().any(Ty::carries_transfer_any) {
        return Value::list(Vec::new());
    }
    let targs = s.all(node, F::TypeArg).map(|arg| file.type_of(arg)).collect();
    let ck = &file.typing().checker;
    #[cfg(test)]
    PARAMETER_WORK.with(|work| work.set(work.get() + 1));
    let Some((types, rest)) = ck.argument_parameters(&callee, &actuals, targs, s.is(node, K::New)) else { return Value::Null };
    let mut parameters = Parameters::new(types, rest, usize::from(tagged));
    let mut out = Vec::new();
    for (arg, sender) in args.into_iter().zip(actuals) {
        if s.is(arg, K::Spread) {
            let Some(value) = s.get(arg, F::Arg) else { continue };
            let ty = file.type_of(value);
            let category = super::unsafe_types::category(file, value);
            match &ty {
                Ty::Any => out.push(finding(file, arg, "spread", &ty, None, category == Some("error"))),
                Ty::Array(element) if matches!(&*element.element, Ty::Any) || super::unsafe_types::element_error(file, value, None) =>
                    out.push(finding(file, arg, "array", &ty, None, super::unsafe_types::element_error(file, value, None))),
                Ty::Tuple(elements) => {
                    for (index, element) in elements.iter().enumerate() {
                        if let Some(receiver) = parameters.next() {
                            if element.unsafe_assignment_to(&receiver) {
                                out.push(finding(file, arg, "tuple", element, Some(&receiver), super::unsafe_types::element_error(file, value, Some(index))));
                            }
                        }
                    }
                }
                _ => {}
            }
        } else if let Some(receiver) = parameters.next() {
            if !super::transfers::empty_map(file, arg) && sender.unsafe_assignment_to(&receiver) {
                out.push(finding(file, arg, "argument", &sender, Some(&receiver), super::unsafe_types::category(file, arg) == Some("error")));
            }
        }
    }
    Value::list(out)
}

#[cfg(test)]
mod tests {
    #[test]
    fn safe_calls_do_not_bind_parameter_types_for_transfer_queries() {
        let code = "declare function f<T>(value: T): void; declare class C { constructor(value: number); } declare function tag(strings: TemplateStringsArray, value: number): void; /*@*/f(1); /*@*/new C(1); /*@*/tag`value ${1}`;";
        crate::engine::tests::with_native_file(code, |file, nodes| {
            super::PARAMETER_WORK.with(|work| work.set(0));
            for &marked in nodes {
                let start = file.syntax.tree.node(marked).start;
                let node = file.syntax.tree.order.iter().copied().find(|&node| {
                    file.syntax.tree.node(node).start == start
                        && matches!(file.syntax.kind(node), Some(super::K::Call | super::K::New | super::K::TaggedTemplate))
                }).expect("marked call");
                assert!(!super::findings(file, node).truthy());
            }
            let work = super::PARAMETER_WORK.with(|work| work.get());
            assert_eq!(work, 0, "safe transfer queries bound {work} signatures");
        });
    }
}
