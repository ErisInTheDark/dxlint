//! Index-signature key domains.

use crate::expr::Value;
use crate::native::ts::facts::NativeFile;

pub(super) fn index_key_types(file: &NativeFile<'_>, node: u32) -> Value<'static> {
    let checker = &file.typing().checker;
    Value::list(checker.index_signature_keys(&file.constrained_type_of(node)).iter().map(|key| {
        Value::record(vec![
            ("text".into(), Value::from(checker.print(key))),
            ("string_like".into(), Value::Bool(super::type_properties::string_like(key, checker))),
        ])
    }).collect())
}
