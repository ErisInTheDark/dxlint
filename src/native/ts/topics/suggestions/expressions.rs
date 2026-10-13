//! Expression identity and literal preferences.

use super::super::super::facts::NativeFile;
use super::super::super::kinds::{Field as F, Kind as K};
use crate::expr::Value;

pub fn decimal_escapes(file: &NativeFile, node: u32) -> Value<'static> {
    let text = file.syntax.text(node);
    let bytes = text.as_bytes();
    let mut findings = Vec::new();
    let mut i = 0;
    while i + 1 < bytes.len() {
        if bytes[i] == b'\\' {
            if matches!(bytes[i + 1], b'8' | b'9') {
                findings.push(Value::record(vec![
                    ("text".into(), Value::from(text[i..i + 2].to_owned())),
                    ("offset".into(), Value::Num(f64::from(file.syntax.tree.node(node).start) + i as f64)),
                ]));
            }
            i += 2;
        } else {
            i += 1;
        }
    }
    Value::list(findings)
}

/// Whether an expression can produce an error object, following possible value branches.
pub(crate) fn could_be_error(file: &NativeFile, node: u32) -> bool {
    let s = &file.syntax;
    let node = s.unparen(node);
    let recurse = |field| s.get(node, field).is_some_and(|n| could_be_error(file, n));
    match s.kind(node) {
        Some(K::Ident | K::Call | K::New | K::Member | K::Index | K::TaggedTemplate | K::Yield | K::Await) => true,
        Some(K::Assign) if matches!(s.word(node, F::Op), Some("=" | "&&=")) => recurse(F::Right),
        Some(K::Assign) if matches!(s.word(node, F::Op), Some("||=" | "??=")) => recurse(F::Left) || recurse(F::Right),
        Some(K::Logical) if s.word(node, F::Op) == Some("&&") => recurse(F::Right),
        Some(K::Logical) => recurse(F::Left) || recurse(F::Right),
        Some(K::Conditional) => recurse(F::Consequent) || recurse(F::Alternate),
        Some(K::Sequence) => s.all(node, F::Expr).last().is_some_and(|n| could_be_error(file, n)),
        _ => false,
    }
}
