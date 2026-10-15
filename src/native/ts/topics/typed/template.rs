//! Interpolation type display for template diagnostics.

use crate::expr::Value;
use crate::native::ts::facts::NativeFile;
use crate::native::ts::kinds::{Field, Kind};
use crate::native::ts::types::Ty;

pub(super) fn display(file: &NativeFile<'_>, node: u32) -> Value<'static> {
    let ty = file.constrained_type_of(node);
    if matches!(ty, Ty::Unknown | Ty::IntrinsicUnknown) {
        let s = &file.syntax;
        if let Some(annotation) = file.bindings().refs.get(&s.unparen(node)).filter(|_| matches!(file.type_of(node), Ty::Unknown | Ty::IntrinsicUnknown)).and_then(|&decl| s.parent(decl)).and_then(|parent| s.get(parent, Field::Type)) {
            if s.is(annotation, Kind::TypeRef) { return Value::from(s.text(annotation).to_string()) }
        }
        return Value::from("unknown");
    }
    Value::from(file.typing().checker.print(&ty))
}
