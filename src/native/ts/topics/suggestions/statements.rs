//! Statement structure preferences.

use super::super::super::facts::NativeFile;
use super::super::super::kinds::{Field as F, Kind as K};
use crate::expr::Value;

pub fn rethrow(file: &NativeFile, node: u32) -> Value<'static> {
    let s = &file.syntax;
    let pair = s.get(node, F::Param).zip(s.get(node, F::Handler)
        .and_then(|block| s.all(block, F::Body).next()).filter(|&n| s.is(n, K::Throw))
        .and_then(|throw| s.get(throw, F::Arg)).map(|n| s.unparen(n)));
    Value::Bool(pair.is_some_and(|(param, arg)| s.is(param, K::Ident) && s.is(arg, K::Ident) && s.text(param) == s.text(arg)))
}
