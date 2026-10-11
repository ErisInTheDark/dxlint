//! Runtime value domains and resolved call contracts.

use super::Fact;
use crate::expr::Value;
use crate::native::ts::facts::NativeFile;
use crate::native::ts::types::{LiteralValue, Ty, ValueDomains};

pub const FACTS: &[Fact] = &[
    Fact {
        shape: Fact::SCALAR.shape,
        name: "access_resolution",
        doc: "native members and indexes: index-signature origin, optional declaration and evaluated value domains",
        get: |f, n| f.with_file(access_resolution, n),
    },
    Fact {
        shape: Fact::SCALAR.shape,
        name: "type_domains",
        doc: "native expressions: value domains, type kind and union/intersection constituents; optional constrained argument defaults to true",
        get: |f, n| f.with_file(domains, n),
    },
    Fact {
        shape: Fact::SCALAR.shape,
        name: "evaluated_type_domains",
        doc: "native expressions: value domains before optional-chain short-circuiting, retaining intrinsic member and return nullability",
        get: |f, n| f.with_file(evaluated_domains, n),
    },
    Fact {
        shape: Fact::SCALAR.shape,
        name: "predicate_call",
        doc: "native calls: asserted argument, assertion kind, target union and bidirectional assignability of a resolved predicate",
        get: |f, n| f.with_file(predicate, n),
    },
    Fact {
        shape: Fact::SCALAR.shape,
        name: "callable_return_domains",
        doc: "native expressions: constrained domains of every callable return, including locally inferred function bodies",
        get: |f, n| f.with_file(callable_returns, n),
    },
];

fn access_resolution(file: &NativeFile<'_>, node: u32) -> Value<'static> {
    let Some(resolution) = file.access_resolution(node) else { return Value::Null };
    Value::record(vec![
        ("from_index".into(), Value::Bool(resolution.from_index)),
        ("optional".into(), Value::Bool(resolution.declarations.iter().any(|declaration| declaration.member.optional))),
        ("evaluated".into(), domain_record(file, &resolution.ty)),
    ])
}

fn domains(file: &NativeFile<'_>, node: u32) -> Value<'static> {
    let args = crate::expr::facts::args();
    let ty = if matches!(args.first(), Some(Value::Bool(false))) { file.type_of(node) } else { file.constrained_type_of(node) };
    domain_record(file, &ty)
}

fn domain_record(file: &NativeFile<'_>, ty: &Ty) -> Value<'static> {
    describe_domain(file, ty, 0)
}

fn describe_domain(file: &NativeFile<'_>, ty: &Ty, depth: usize) -> Value<'static> {
    let checker = &file.typing().checker;
    let mut shape = ty;
    let mut depth = depth;
    while depth < 64 {
        let Some(source) = checker.readonly_source(shape) else { break };
        shape = source;
        depth += 1;
    }
    if depth >= 64 {
        return Value::record(vec![("kind".into(), Value::from("recursive")), ("opaque".into(), Value::Bool(true))]);
    }
    let ValueDomains { truthy, falsy, nullish, non_nullish, opaque } = checker.value_domains(ty);
    let parts = match shape {
        Ty::Union(parts) | Ty::Inter(parts) => parts.iter().map(|part| describe_domain(file, part, depth + 1)).collect(),
        _ => Vec::new(),
    };
    let mut flags = Vec::new();
    file.flags_of(ty, &mut flags);
    let primitive = match shape {
        Ty::Prim(primitive) | Ty::Lit(_, primitive) => Some(*primitive),
        Ty::EnumMember(member) => member.primitive(),
        _ => None,
    };
    let literal = checker.literal_value(ty).map_or(Value::Null, |literal| {
        let (kind, value) = match literal {
            LiteralValue::String(value) => ("string", Value::from(value)),
            LiteralValue::Number(value) => ("number", Value::from(value)),
            LiteralValue::Boolean(value) => ("boolean", Value::Bool(value)),
            LiteralValue::BigInt(value) => ("bigint", Value::from(value.to_string())),
            LiteralValue::Null => ("null", Value::Null),
            LiteralValue::Undefined => ("undefined", Value::Null),
        };
        Value::record(vec![("type".into(), Value::from(kind)), ("value".into(), value)])
    });
    Value::record(vec![
        ("kind".into(), Value::from(shape.kind())),
        ("parts".into(), Value::list(parts)),
        ("truthy".into(), Value::Bool(truthy)),
        ("falsy".into(), Value::Bool(falsy)),
        ("nullish".into(), Value::Bool(nullish)),
        ("non_nullish".into(), Value::Bool(non_nullish)),
        ("opaque".into(), Value::Bool(opaque)),
        ("flags".into(), Value::list(flags.into_iter().map(Value::from).collect())),
        ("primitive".into(), primitive.map_or(Value::Null, Value::from)),
        ("literal".into(), literal),
    ])
}

fn evaluated_domains(file: &NativeFile<'_>, node: u32) -> Value<'static> {
    domain_record(file, &file.evaluated_type_of(node))
}

fn predicate(file: &NativeFile<'_>, node: u32) -> Value<'static> {
    let Some((argument, predicate)) = file.resolved_predicate(node) else { return Value::Null };
    let checker = &file.typing().checker;
    let actual = file.constrained_type_of(argument);
    let target = predicate.target;
    let relation = |reverse| target.as_ref().map_or(Value::Null, |target| {
        let answer = if reverse { checker.assignable(target, &actual) } else { checker.assignable(&actual, target) };
        answer.map_or(Value::Null, Value::Bool)
    });
    Value::record(vec![
        ("argument_index".into(), Value::from(predicate.parameter.unwrap_or_default())),
        ("asserts".into(), Value::Bool(predicate.asserts)),
        ("argument".into(), domain_record(file, &actual)),
        ("target".into(), target.as_ref().map_or(Value::Null, |target| domain_record(file, target))),
        ("to_target".into(), relation(false)),
        ("from_target".into(), relation(true)),
        ("target_union".into(), Value::Bool(matches!(target, Some(Ty::Union(_))))),
    ])
}

fn callable_returns(file: &NativeFile<'_>, node: u32) -> Value<'static> {
    Value::list(file.callable_return_types(&file.constrained_type_of(node)).into_iter().map(|ty| {
        domain_record(file, &file.constrained_type(ty, node))
    }).collect())
}

#[cfg(test)]
mod tests {
    use crate::engine::tests::with_native_file;

    #[test]
    fn type_domains_can_keep_open_types_without_applying_constraints() {
        let source = "function f<T extends number>(value: T) { const result = /*@*/value; } declare const concrete: string; const result = /*@*/concrete;";
        with_native_file(source, |file, nodes| {
            let raw = |node| crate::expr::facts::with_args(vec![crate::expr::Value::Bool(false)], || super::domains(file, node));
            let default = super::domains(file, nodes[0]);
            assert_eq!(default.key("kind"), crate::expr::Value::from("number"));
            assert_eq!(raw(nodes[0]).key("kind"), crate::expr::Value::from("parameter"), "raw domains must retain the open type");
            assert!(raw(nodes[1]).loose_eq(&super::domains(file, nodes[1])), "a concrete type must not depend on the constraint switch");
        });
    }

    #[test]
    fn intersection_domains_respect_primitive_and_literal_constraints() {
        for (annotation, truthy, falsy) in [
            ("string & number", false, false),
            ("(string | number) & ('foo' | 123) & { brand: true }", true, false),
            ("((string & { brandA: true }) | (number & { brandB: true })) & 'foo'", true, false),
            ("number & { brand: true }", true, true),
        ] {
            let source = format!("declare const value: {annotation}; const result = /*@*/value;");
            with_native_file(&source, |file, nodes| {
                let ty = file.type_of(nodes[0]);
                let domain = file.typing().checker.value_domains(&ty);
                assert_eq!((domain.truthy, domain.falsy), (truthy, falsy), "{annotation}: {ty:?}");
            });
        }
    }

}
