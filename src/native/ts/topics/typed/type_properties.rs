//! Structural type questions, preserving literal and enum identity.

use crate::expr::Value;
use crate::native::ts::types::{Checker, Ty};

pub(super) fn literal_values(ty: &Ty) -> Value<'static> {
    let mut values = Vec::new();
    collect_literals(ty, &mut values);
    Value::list(values)
}

fn collect_literals(ty: &Ty, values: &mut Vec<Value<'static>>) {
    match ty {
        Ty::EnumMember(member) => collect_literals(&member.base, values),
        // Boolean stores the finite `false | true` domain compactly.
        Ty::Prim("boolean") => {
            for value in [false, true] {
                values.push(Value::record(vec![("type".into(), Value::from("boolean")), ("value".into(), Value::Bool(value))]));
            }
        }
        Ty::Union(parts) | Ty::Inter(parts) => {
            for part in parts {
                collect_literals(part, values);
            }
        }
        Ty::Lit(text, kind) => {
            let value = match *kind {
                "string" => crate::expr::js::strings::decode_literal(text).map(Value::from),
                "number" => crate::native::ts::statics::number(text).map(Value::from),
                "boolean" => Some(Value::Bool(text.as_ref() == "true")),
                "bigint" => Some(Value::from(text.to_string())),
                _ => None,
            };
            if let Some(value) = value {
                values.push(Value::record(vec![("type".into(), Value::from(*kind)), ("value".into(), value)]));
            }
        }
        _ => {}
    }
}

pub(super) fn string_like(ty: &Ty, checker: &Checker<'_>) -> bool {
    match ty {
        Ty::EnumMember(member) => string_like(&member.base, checker),
        Ty::Prim("string") | Ty::Lit(_, "string") => true,
        Ty::Named(symbol, _) => checker.is_enum(symbol) && checker.is_string_enum(symbol),
        Ty::Union(parts) => !parts.is_empty() && parts.iter().all(|part| string_like(part, checker)),
        Ty::Inter(parts) => !parts.iter().any(indeterminate) && parts.iter().any(|part| string_like(part, checker)),
        _ => false,
    }
}

fn indeterminate(ty: &Ty) -> bool {
    match ty {
        Ty::Any | Ty::Unknown => true,
        Ty::Union(parts) | Ty::Inter(parts) => parts.iter().any(indeterminate),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn boolean_domains_keep_both_literal_possibilities() {
        let values = literal_values(&Ty::Union(vec![Ty::Prim("boolean"), Ty::Undefined]));
        for possible in [false, true] {
            assert!(values.items().iter().any(|literal|
                literal.key("type").text() == "boolean" && literal.key("value") == Value::Bool(possible)));
        }
        let narrowed = literal_values(&Ty::Lit("true".into(), "boolean"));
        assert!(narrowed.items().iter().any(|literal| literal.key("value") == Value::Bool(true)));
        assert!(narrowed.items().iter().all(|literal| literal.key("value") == Value::Bool(true)));
    }
}
