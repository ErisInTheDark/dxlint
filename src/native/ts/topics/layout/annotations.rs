//! Token boundaries around native type annotations.

use crate::expr::Value;
use crate::native::ts::facts::NativeFile;
use crate::native::ts::kinds::{Field as F, Kind as K};
use crate::native::ts::lexer::Tok;

use super::{gap_has_space, lex};

pub(super) fn annotation_spacing(file: &NativeFile<'_>, node: u32) -> Value<'static> {
    let syntax = &file.syntax;
    let (field, category) = match syntax.kind(node) {
        Some(K::Declarator) => (F::Type, "variable"),
        Some(K::Param) => (F::Type, "parameter"),
        Some(K::ClassProp | K::PropSig | K::IndexSig) => (F::Type, "property"),
        Some(K::Function | K::Arrow | K::Method) => (F::ReturnType, "returnType"),
        Some(K::MethodSig | K::CallSig) => (F::ReturnType, "property"),
        Some(K::FnType) => (F::ReturnType, "colon"),
        Some(K::TupleMember | K::Mapped) => (F::Type, "colon"),
        _ => return Value::Null,
    };
    let Some(ty) = syntax.get(node, field) else { return Value::Null };
    let lexed = file.cached(lex);
    let type_start = syntax.tree.node(ty).start;
    let mut colon = lexed.0.partition_point(|token| token.start < type_start);
    while colon > 0 && lexed.0[colon - 1].tok == Tok::P("(") {
        colon -= 1;
    }
    let Some(colon) = colon.checked_sub(1).filter(|&i| lexed.0[i].tok == Tok::P(":")) else { return Value::Null };
    let Some(previous) = colon.checked_sub(1) else { return Value::Null };
    let optional = lexed.0[previous].tok == Tok::P("?");
    let mut optional_start = previous;
    let mut label = "?:";
    if optional && previous > 0 {
        match lexed.0[previous - 1].tok {
            Tok::P("+") => { optional_start -= 1; label = "+?:"; }
            Tok::P("-") => { optional_start -= 1; label = "-?:"; }
            _ => {}
        }
    }
    let gap = |start: u32, end: u32, offset: u32| Value::record(vec![
        ("spaced".into(), Value::Bool(gap_has_space(&syntax.src[start as usize..end as usize]))),
        ("offset".into(), Value::from(offset as usize)),
    ]);
    let colon_token = lexed.0[colon];
    let optional_token = lexed.0[optional_start];
    Value::record(vec![
        ("category".into(), Value::from(category)),
        ("optional".into(), Value::Bool(optional)),
        ("label".into(), Value::from(label)),
        ("before".into(), gap(lexed.0[previous].end, colon_token.start, colon_token.start)),
        ("after".into(), gap(colon_token.end, type_start, colon_token.start)),
        ("inner".into(), gap(lexed.0[previous].end, colon_token.start, colon_token.start)),
        ("outer".into(), optional_start.checked_sub(1).map_or(Value::Null, |before| gap(lexed.0[before].end, optional_token.start, optional_token.start))),
    ])
}
