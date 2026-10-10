use super::*;

pub(super) fn finding(file: &NativeFile<'_>, node: u32) -> Value<'static> {
    let s = &file.syntax;
    let ck = &file.typing().checker;
    let start = |id| s.tree.node(id).start as usize;
    let plain = |id, primitive| matches_type(&file.type_of(id), primitive, ck);
    let constrained = |id, primitive| matches_type(&file.constrained_type_of(id), primitive, ck);
    let empty = |id| s.is(id, Kind::String) && crate::native::ts::statics::literal_text(s, id).as_deref() == Some("");
    let result = match s.kind(node) {
        Some(Kind::Assign | Kind::Binary) if matches!(s.word(node, Field::Op), Some("+" | "+=")) => {
            let (Some(left), Some(right)) = (s.get(node, Field::Left), s.get(node, Field::Right)) else { return Value::Null };
            if empty(right) && plain(left, "string") {
                Some(("string", "Concatenating a string with ''".to_owned(), if s.is(node, Kind::Assign) { start(node) } else { s.tree.node(left).end as usize }))
            } else if s.is(node, Kind::Binary) && empty(left) && plain(right, "string") {
                Some(("string", "Concatenating '' with a string".to_owned(), start(node)))
            } else { None }
        }
        Some(Kind::Call) => {
            let Some(callee) = s.get(node, Field::Callee) else { return Value::Null };
            if s.is(callee, Kind::Ident) && file.decl_of(callee).is_none() {
                let primitive = match s.text(callee) {
                    "String" => "string", "Boolean" => "boolean", "Number" => "number", "BigInt" => "bigint",
                    _ => return Value::Null,
                };
                s.all(node, Field::Arg).next().filter(|&arg| constrained(arg, primitive))
                    .map(|_| (primitive, format!("Passing a {primitive} to {}()", s.text(callee)), start(callee)))
            } else if s.is(callee, Kind::Member) && s.word(callee, Field::Property) == Some("toString") {
                s.get(callee, Field::Object).filter(|&object| constrained(object, "string") && !has_enum(&file.constrained_type_of(object), ck))
                    .map(|_| ("string", "Calling a string's .toString() method".to_owned(), start(s.get(callee, Field::Property).expect("checked property"))))
            } else { None }
        }
        Some(Kind::Unary) => {
            let Some(arg) = s.get(node, Field::Arg) else { return Value::Null };
            match s.word(node, Field::Op) {
                Some("+") if plain(arg, "number") => Some(("number", "Using the unary + operator on a number".to_owned(), start(node))),
                Some("!" | "~") => {
                    let arg = s.unparen(arg);
                    if !s.is(arg, Kind::Unary) || s.word(arg, Field::Op) != s.word(node, Field::Op) { return Value::Null }
                    let Some(inner) = s.get(arg, Field::Arg) else { return Value::Null };
                    if s.word(node, Field::Op) == Some("!") && plain(inner, "boolean") {
                        Some(("boolean", "Using !! on a boolean".to_owned(), start(node)))
                    } else if s.word(node, Field::Op) == Some("~") && integers(&file.type_of(inner)) {
                        Some(("number", "Using ~~ on an integer".to_owned(), start(node)))
                    } else { None }
                }
                _ => None,
            }
        }
        _ => None,
    };
    result.map_or(Value::Null, |(primitive, violation, offset)| Value::record(vec![
        ("type".into(), Value::from(primitive)), ("violation".into(), Value::from(violation)), ("offset".into(), Value::from(offset)),
    ]))
}

fn matches_type(ty: &Ty, primitive: &str, ck: &Checker<'_>) -> bool {
    match ty {
        Ty::EnumMember(member) => matches_type(&member.base, primitive, ck),
        Ty::Prim(name) | Ty::Lit(_, name) => *name == primitive,
        Ty::Named(sym, _) if ck.is_enum(sym) => primitive == "number" && !ck.is_string_enum(sym) || primitive == "string" && ck.is_string_enum(sym),
        Ty::Union(parts) => !parts.is_empty() && parts.iter().all(|part| matches_type(part, primitive, ck)),
        _ => false,
    }
}

fn integers(ty: &Ty) -> bool {
    match ty {
        Ty::EnumMember(member) => integers(&member.base),
        Ty::Lit(text, "number") => crate::native::ts::statics::number(text).is_some_and(|number| number.fract() == 0.0),
        Ty::Union(parts) => !parts.is_empty() && parts.iter().all(integers),
        _ => false,
    }
}

fn has_enum(ty: &Ty, ck: &Checker<'_>) -> bool {
    match ty {
        Ty::EnumMember(_) => true,
        Ty::Named(sym, _) => ck.is_enum(sym),
        Ty::Union(parts) => parts.iter().any(|part| has_enum(part, ck)),
        _ => false,
    }
}
