use super::*;

pub(super) fn mismatch(file: &NativeFile<'_>, node: u32) -> Value<'static> {
    let s = &file.syntax;
    let Some(getter) = s.parent(node).filter(|&parent| matches!(s.kind(parent), Some(Kind::Method | Kind::MethodSig)) && s.word(parent, Field::Kind) == Some("get")) else { return Value::Bool(false) };
    let Some(owner) = s.parent(getter).filter(|&parent| matches!(s.kind(parent), Some(Kind::Class | Kind::Interface | Kind::TypeLit))) else { return Value::Bool(false) };
    let key = |method| s.get(method, Field::Key).map(|key| {
        crate::native::ts::statics::property_name(s, method).unwrap_or_else(|| file.tokens(key).join(""))
    });
    let name = key(getter);
    let Some(setter) = s.all(owner, Field::Member).filter(|&member| s.word(member, Field::Kind) == Some("set") && key(member) == name).last() else { return Value::Bool(false) };
    let params: Vec<_> = s.all(setter, Field::Param).collect();
    if params.len() != 1 {
        return Value::Bool(false);
    }
    let Some(annotation) = s.get(params[0], Field::Type) else { return Value::Bool(false) };
    Value::Bool(file.typing().checker.assignable(&file.type_of(node), &file.type_of(annotation)) == Some(false))
}
