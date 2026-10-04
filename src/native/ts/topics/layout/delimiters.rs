//! Opening brace boundaries of declaration bodies.

use crate::native::ts::facts::NativeFile;
use crate::native::ts::kinds::{Field as F, Kind as K};
use crate::native::ts::lexer::Tok;

use super::lex;

pub(super) fn brace_start(file: &NativeFile<'_>, node: u32) -> Option<u32> {
    let syntax = &file.syntax;
    let span = syntax.tree.node(node);
    let from = match syntax.kind(node) {
        Some(K::Switch) => syntax.get(node, F::Discriminant).map_or(span.start, |id| syntax.tree.node(id).end),
        Some(K::Class | K::Interface | K::Enum) => [F::Name, F::TypeParam, F::Extends, F::TypeArg, F::Implements].into_iter()
            .flat_map(|field| syntax.all(node, field)).map(|id| syntax.tree.node(id).end).max().unwrap_or(span.start),
        Some(K::Block | K::StaticBlock | K::TypeLit) => span.start,
        _ => return None,
    };
    let lexed = file.cached(lex);
    let at = lexed.0.partition_point(|token| token.start < from);
    lexed.0[at..].iter().take_while(|token| token.start < span.end).find(|token| token.tok == Tok::P("{")).map(|token| token.start)
}
