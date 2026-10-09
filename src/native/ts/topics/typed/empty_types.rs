//! Type operations producing the resolved empty object, not unresolved keys.

use super::*;
use crate::native::ts::kinds::{Field as F, Kind as K};

pub(super) fn generated(file: &NativeFile<'_>, node: u32) -> Value<'static> {
    let s = &file.syntax;
    if s.is(node, K::TypeRef) {
        if s.all(node, F::TypeArg).next().is_none() { return Value::Bool(false) }
        let mut parent = s.parent(node);
        while parent.is_some_and(|parent| s.is(parent, K::ParenType)) { parent = parent.and_then(|parent| s.parent(parent)); }
        if parent.is_some_and(|parent| s.is(parent, K::Intersection)) { return Value::Bool(false) }
    } else if !s.is(node, K::Intersection) { return Value::Bool(false) }
    Value::Bool(file.typing().checker.generated_empty_object(&file.type_of(node)))
}
