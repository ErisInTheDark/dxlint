//! Unsafe call arguments, including consumption of tuple and array rest parameters.

use super::*;
use crate::native::ts::kinds::{Field as F, Kind as K};

struct Parameters {
    types: Vec<Ty>,
    rest: bool,
    at: usize,
}

impl Parameters {
    fn next(&mut self) -> Option<Ty> {
        let at = self.at;
        self.at += 1;
        let rest_at = self.types.len().saturating_sub(1);
        if self.rest && at >= rest_at {
            match self.types.last()? {
                Ty::Array(element) => Some((*element.element).clone()),
                Ty::Tuple(elements) => {
                    let ty = elements.at(at - rest_at);
                    (!ty.is_opaque()).then_some(ty)
                }
                _ => None,
            }
        } else {
            self.types.get(at).cloned()
        }
    }
}

fn finding(file: &NativeFile<'_>, node: u32, mode: &str, sender: &Ty, receiver: Option<&Ty>, error: bool) -> Value<'static> {
    let ck = &file.typing().checker;
    let sender = if error { if mode == "array" { "error" } else { "error typed" }.to_string() } else if mode == "tuple" { format!("of type `{}`", ck.print(sender)) } else { format!("`{}`", ck.print(sender)) };
    Value::record(vec![
        ("offset".into(), Value::from(file.syntax.tree.node(node).start as usize)),
        ("mode".into(), Value::from(mode.to_string())),
        ("sender".into(), Value::from(sender)),
        ("receiver".into(), receiver.map_or(Value::Null, |ty| Value::from(format!("`{}`", ck.print(ty))))),
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
    let types: Vec<_> = args.iter().map(|&arg| file.type_of(arg)).collect();
    let targs = s.all(node, F::TypeArg).map(|arg| file.type_of(arg)).collect();
    let ck = &file.typing().checker;
    let Some((types, rest)) = ck.argument_parameters(&callee, &types, targs, s.is(node, K::New)) else { return Value::Null };
    let mut parameters = Parameters { types, rest, at: usize::from(tagged) };
    let mut out = Vec::new();
    for arg in args {
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
            let sender = file.type_of(arg);
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
    fn readonly_arrays_keep_unsafe_element_transfers() {
        let linter = crate::engine::tests::native_linter("rule no-unsafe-assignment:*\nrule no-unsafe-argument");
        let code = "declare const mutable: any[]; declare const immutable: readonly any[]; const first: readonly number[] = mutable; const second: readonly number[] = immutable; function takes(values: readonly number[]) {} takes(mutable); takes(immutable);";
        let findings = linter.lint(std::path::Path::new("/root/a.ts"), code);
        assert_eq!(findings.len(), 2, "{findings:?}");
    }

    use crate::engine::tests::native_linter;
    use std::path::Path;

    #[test]
    fn rest_parameters_consume_each_tuple_member_without_losing_the_next_argument() {
        let linter = native_linter("rule no-unsafe-argument\nrule no-unsafe-argument:*");
        let code = "declare function f(a: string, ...rest: [number, boolean]): void; declare const tuple: [any, any]; f('x', ...tuple); declare function safe(x: unknown): void; safe(1 as any); f(1 as any, 2, false);";
        assert_eq!(linter.lint(Path::new("/root/a.ts"), code).len(), 3);
    }

    #[test]
    fn constant_tuple_spreads_consume_safe_members_before_the_unsafe_argument() {
        let linter = native_linter("rule no-unsafe-argument\nrule no-unsafe-argument:*");
        let code = "declare function f(a: string, b: number, c: boolean): void; const tuple = ['x', 2] as const; f(...tuple, 1 as any); const unsafeTuple = ['x', 1 as any] as const; f(...unsafeTuple, true);";
        let diagnostics = linter.lint(Path::new("/root/a.ts"), code);
        assert_eq!(diagnostics.len(), 2);
        assert_eq!(diagnostics[0].start, code.find("1 as any").unwrap());
    }
}
