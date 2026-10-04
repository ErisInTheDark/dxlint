//! Canonical keys for syntactically comparable references.

use crate::expr::Value;
use crate::native::ts::facts::NativeFile;
use crate::native::ts::kinds::{Field as F, Kind as K};
use crate::native::ts::statics;

pub fn key(file: &NativeFile, node: u32, strict: bool) -> Value<'static> {
    reference_key(file, node, strict).map_or(Value::Null, Value::from)
}

fn reference_key(file: &NativeFile, node: u32, strict: bool) -> Option<String> {
    let s = &file.syntax;
    let node = s.unparen(node);
    // Length prefixes keep property text and recursive keys unambiguous.
    let atom = |tag: &str, value: &str| format!("{tag}{}:{value}", value.len());
    Some(match s.kind(node)? {
        K::Ident => atom("id", s.text(node)),
        K::PrivateName => atom("private", s.text(node)),
        K::This => "this".into(),
        K::Super => "super".into(),
        kind @ (K::String | K::Number | K::Bool | K::Null | K::BigInt | K::Regex) => {
            let (ty, value) = statics::literal_value(s, node)?;
            atom(&format!("{kind:?}:{ty}"), &value)
        }
        kind @ (K::Member | K::Index) => {
            let object = reference_key(file, s.get(node, F::Object)?, strict)?;
            let property = if !strict {
                statics::property_name(s, node).map(|name| atom("static", &name))
            } else { None };
            let property = match property {
                Some(property) => property,
                None => {
                    let key = s.get(node, F::Property).or_else(|| s.get(node, F::Index))?;
                    atom(if kind == K::Member { "member" } else { "index" }, &reference_key(file, key, strict)?)
                }
            };
            format!("{}{}", atom("object", &object), atom("property", &property))
        }
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::tests::with_native_file;

    #[test]
    fn keys_preserve_comparator_equivalence_and_reject_unknown_expressions() {
        for (left, right, loose, strict) in [
            ("a.b", "a['b']", true, false),
            ("a[b]", "a[(b)]", true, true),
            ("a['b.c']", "a.b.c", false, false),
            ("a['1']", "a[1]", true, false),
            ("a[b]", "a['b']", false, false),
            ("this.a", "(this).a", true, true),
            ("0x10", "16", true, true),
            ("'16'", "16", false, false),
        ] {
            let source = format!("const left = /*@*/{left}; const right = /*@*/{right};");
            with_native_file(&source, |file, nodes| {
                for (is_strict, expected) in [(false, loose), (true, strict)] {
                    let a = key(file, nodes[0], is_strict);
                    let b = key(file, nodes[1], is_strict);
                    assert!(!matches!(a, Value::Null) && !matches!(b, Value::Null), "{source}");
                    assert_eq!(a == b, expected, "{source}, strict={is_strict}");
                    assert_eq!(a == b, file.same_reference_keyed(nodes[0], nodes[1], is_strict));
                }
            });
        }
        for expression in ["f()", "a[x + y]", "[a]", "a[f()]"] {
            with_native_file(&format!("const value = /*@*/{expression};"), |file, nodes| {
                assert!(matches!(key(file, nodes[0], false), Value::Null), "{expression}");
                assert!(matches!(key(file, nodes[0], true), Value::Null), "{expression}");
            });
        }
    }
}
