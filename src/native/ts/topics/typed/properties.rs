//! Property resolution and index domains, independent of access policy.

use crate::expr::Value;
use crate::native::ts::facts::NativeFile;
use crate::native::ts::kinds::{Field as F, Kind as K};
use crate::native::ts::summary::Visibility;

pub(super) fn property_info(file: &NativeFile<'_>, node: u32) -> Value<'static> {
    let s = &file.syntax;
    if !matches!(s.kind(node), Some(K::Member | K::Index)) { return Value::Null }
    let Some(object) = s.get(node, F::Object) else { return Value::Null };
    let name = crate::native::ts::statics::property_name(s, node)
        .or_else(|| s.get(node, F::Index).and_then(|index| file.static_string(index)));
    let Some(name) = name else { return Value::Null };
    let resolved = file.typing().checker.member_resolution(&file.constrained_type_of(object), &name);
    let visibility = resolved.declarations.first().map(|declaration| match declaration.member.visibility {
        Visibility::Public => "public",
        Visibility::Private => "private",
        Visibility::Protected => "protected",
    });
    Value::record(vec![
        ("declared".into(), Value::Bool(resolved.declared)),
        ("visibility".into(), visibility.map_or(Value::Null, Value::from)),
    ])
}

pub(super) fn index_key_types(file: &NativeFile<'_>, node: u32) -> Value<'static> {
    let checker = &file.typing().checker;
    Value::list(checker.index_signature_keys(&file.constrained_type_of(node)).iter().map(|key| {
        Value::record(vec![
            ("text".into(), Value::from(checker.print(key))),
            ("string_like".into(), Value::Bool(super::type_properties::string_like(key, checker))),
        ])
    }).collect())
}
