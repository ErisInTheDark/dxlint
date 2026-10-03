//! Counted loops whose index serves only to read one collection.

use crate::expr::Value;
use crate::native::ts::facts::NativeFile;
use crate::native::ts::kinds::{Field as F, Kind as K};
use crate::native::ts::syntax::Syntax;

fn number(s: &Syntax, node: u32, expected: f64) -> bool {
    s.is(node, K::Number) && super::super::super::statics::number(s.text(node)) == Some(expected)
}

pub(super) fn simple_array_iteration(file: &NativeFile, node: u32) -> Value<'static> {
    Value::Bool(iterates_array(file, node).unwrap_or(false))
}

fn iterates_array(file: &NativeFile, node: u32) -> Option<bool> {
    let s = &file.syntax;
    let init = s.get(node, F::Init)?;
    if !s.is(init, K::Var) || s.word(init, F::Kind) == Some("const") {
        return None;
    }
    let declarations: Vec<_> = s.all(init, F::Decl).collect();
    if declarations.len() != 1 {
        return None;
    }
    let declaration = declarations[0];
    let name = s.get(declaration, F::Name)?;
    if !s.is(name, K::Ident) || !number(s, s.get(declaration, F::Init)?, 0.0) {
        return None;
    }
    let test = s.get(node, F::Test)?;
    if !s.is(test, K::Binary) || s.word(test, F::Op) != Some("<") || s.word(test, F::Left) != Some(s.text(name)) {
        return None;
    }
    let length = s.get(test, F::Right)?;
    let property = s.get(length, F::Property).or_else(|| s.get(length, F::Index))?;
    if !matches!(s.kind(length), Some(K::Member | K::Index)) || !s.is(property, K::Ident) || s.text(property) != "length" {
        return None;
    }
    let array = s.get(length, F::Object)?;
    let update = s.get(node, F::Update)?;
    let increment = if s.is(update, K::Update) {
        s.word(update, F::Op) == Some("++") && s.word(update, F::Arg) == Some(s.text(name))
    } else if s.is(update, K::Assign) && s.word(update, F::Left) == Some(s.text(name)) {
        let right = s.get(update, F::Right)?;
        s.word(update, F::Op) == Some("+=") && number(s, right, 1.0)
            || s.word(update, F::Op) == Some("=") && s.is(right, K::Binary) && s.word(right, F::Op) == Some("+")
                && [(F::Left, F::Right), (F::Right, F::Left)].iter().any(|&(id, literal)| {
                    s.word(right, id) == Some(s.text(name)) && s.get(right, literal).is_some_and(|n| number(s, n, 1.0))
                })
    } else {
        false
    };
    let body = s.get(node, F::Body)?;
    let range = s.tree.node(body);
    Some(increment && file.bindings().refs.iter().all(|(&reference, &declaration)| {
        if declaration != name || s.tree.node(reference).start < range.start || s.tree.node(reference).end > range.end {
            return true;
        }
        s.parent(reference).is_some_and(|parent| {
            matches!(s.kind(parent), Some(K::Index | K::Member))
                && matches!(s.field(reference), Some(F::Index | F::Property))
                && s.get(parent, F::Object).is_some_and(|n| !s.is(n, K::This) && s.text(n) == s.text(array))
                && !file.bindings().write_sites.contains_key(&parent)
        })
    }))
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use crate::engine::tests::native_linter;

    #[test]
    fn array_iteration_rejects_index_reads_and_collection_writes() {
        let linter = native_linter("rule prefer-for-of");
        assert_eq!(linter.lint(Path::new("/root/file.ts"), "for (let i = 0; i < items.length; i += 1) consume(items[i]);").len(), 1);
        for body in ["items[i] = 1;", "consume(i);", "consume(other[i]);"] {
            assert!(linter.lint(Path::new("/root/file.ts"), &format!("for (let i = 0; i < items.length; i++) {{ {body} }}")).is_empty());
        }
    }
}
