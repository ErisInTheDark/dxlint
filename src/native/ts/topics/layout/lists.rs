//! Commas whose meaning belongs to a syntax list.

use crate::expr::Value;
use crate::native::ts::facts::NativeFile;
use crate::native::ts::kinds::{Field as F, Kind as K};
use crate::native::ts::lexer::Tok;

use super::lex;

fn assignment_pattern(file: &NativeFile<'_>, mut node: u32) -> bool {
    let syntax = &file.syntax;
    while let Some(parent) = syntax.parent(node) {
        match syntax.kind(parent) {
            Some(K::Assign | K::ForIn | K::ForOf) => return syntax.get(parent, F::Left) == Some(node),
            Some(K::Array | K::Object | K::Paren) => node = parent,
            Some(K::Prop) if syntax.get(parent, F::Value) == Some(node) => node = parent,
            _ => return false,
        }
    }
    false
}

pub(super) fn trailing_comma_sites(file: &NativeFile<'_>, node: u32) -> Value<'static> {
    let syntax = &file.syntax;
    let mut groups: Vec<(&str, Vec<u32>)> = Vec::new();
    let main = match syntax.kind(node) {
        Some(K::Array | K::ArrayPattern) => Some(("arrays", F::Element)),
        Some(K::Object | K::ObjectPattern) => Some(("objects", F::Property)),
        Some(K::Function | K::Arrow | K::Method | K::FnType) => Some(("functions", F::Param)),
        Some(K::Call | K::New) => Some(("functions", F::Arg)),
        Some(K::ImportCall) => Some(("dynamicImports", F::Arg)),
        Some(K::Import) => Some(("imports", F::Specifier)),
        Some(K::Export) => Some(("exports", F::Specifier)),
        Some(K::Enum) => Some(("enums", F::Member)),
        Some(K::TupleType) => Some(("tuples", F::Element)),
        _ => None,
    };
    if let Some((kind, field)) = main {
        let mut items: Vec<_> = syntax.all(node, field).collect();
        if syntax.is(node, K::ObjectPattern) {
            items.extend(syntax.all(node, F::Rest));
            items.sort_unstable_by_key(|&id| syntax.tree.node(id).start);
        }
        groups.push((kind, items));
    }
    for (kind, field) in [("generics", F::TypeParam), ("type-arguments", F::TypeArg)] {
        let items: Vec<_> = syntax.all(node, field).collect();
        if !items.is_empty() {
            groups.push((kind, items));
        }
    }
    let lexed = file.cached(lex);
    let mut sites = Vec::new();
    for (kind, items) in groups {
        let Some(&last) = items.last() else { continue };
        if syntax.is(last, K::Hole) {
            continue;
        }
        let end = syntax.tree.node(last).end;
        let at = lexed.0.partition_point(|token| token.start < end);
        let Some(next) = lexed.0.get(at) else { continue };
        let trailing = next.tok == Tok::P(",");
        let (token, close) = if trailing {
            let Some(close) = lexed.0.get(at + 1) else { continue };
            (*next, *close)
        } else {
            let Some(previous) = at.checked_sub(1).and_then(|i| lexed.0.get(i)) else { continue };
            (*previous, *next)
        };
        if !matches!(close.tok, Tok::P("}" | "]" | ")" | ">")) {
            continue;
        }
        let allowed = !(syntax.is(last, K::Rest)
            || syntax.is(last, K::Spread) && assignment_pattern(file, node)
            || syntax.get(last, F::Name).is_some_and(|name| syntax.is(name, K::Rest)));
        let offset = if trailing { token.start } else { token.end } as usize;
        let multiline = syntax.src[token.end as usize..close.end as usize].contains(['\n', '\r', '\u{2028}', '\u{2029}']);
        let tsx_single = kind == "generics" && items.len() == 1 && file.path.extension().is_some_and(|ext| ext == "tsx");
        sites.push(Value::Record(vec![
            ("kind".into(), Value::from(kind)),
            ("offset".into(), Value::from(offset)),
            ("trailing".into(), Value::Bool(trailing)),
            ("multiline".into(), Value::Bool(multiline)),
            ("allowed".into(), Value::Bool(allowed)),
            ("tsx_single".into(), Value::Bool(tsx_single)),
        ]));
    }
    Value::List(sites)
}
