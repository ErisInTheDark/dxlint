//! How a transform's output text relates to the original: piece kinds, and the
//! marks rewrite templates carry around copied text.

use crate::config::ast::{Expr, ExprKind, Name};
use crate::config::source::Span;

/// How a piece of the output relates to the original text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PieceKind {
    /// Original text left in place.
    Kept,
    /// A node's text copied by a rewrite (`{node}`): where tools take you
    /// for that node; errors in it map back.
    Copy,
    /// A further copy whose errors map back, but which tools don't take you
    /// to (`{mirror(node)}`).
    Mirror,
    /// Other text naming the same thing (`{renamed(node, text)}`).
    Renamed,
}

impl PieceKind {
    pub fn name(self) -> &'static str {
        match self {
            PieceKind::Kept => "kept",
            PieceKind::Copy => "copy",
            PieceKind::Mirror => "mirror",
            PieceKind::Renamed => "renamed",
        }
    }
}

/// Rewrite text carries each copied node's provenance between Unicode
/// noncharacters (as fix placeholders do) until the engine strips it:
/// `OPEN kind start,end SEP text CLOSE`.
pub mod copy_mark {
    use super::PieceKind;

    pub const OPEN: char = '\u{FDD2}';
    pub const SEP: char = '\u{FDD3}';
    pub const CLOSE: char = '\u{FDD4}';

    pub fn mark(kind: PieceKind, start: usize, end: usize, text: &str) -> String {
        let kind = match kind {
            PieceKind::Mirror => 'm',
            PieceKind::Renamed => 'r',
            _ => 'c',
        };
        format!("{OPEN}{kind}{start},{end}{SEP}{text}{CLOSE}")
    }

    pub fn kind(c: char) -> PieceKind {
        match c {
            'm' => PieceKind::Mirror,
            'r' => PieceKind::Renamed,
            _ => PieceKind::Copy,
        }
    }
}

/// The internal function rewrite templates call around each interpolation.
pub const MAPPED_TEXT: &str = "mapped_text";

/// `mapped_text(kind, value, value.offset, value.end_offset, text)`: `text`
/// marked as a `kind` piece of `value`'s span; made up when `value` is no
/// node.
pub fn marked(kind: PieceKind, value: &Expr, text: Expr, span: Span) -> Expr {
    let at = |kind: ExprKind| Expr { kind, span };
    let name = |text: &str| Name { text: text.into(), span };
    let span_of = |key: &str| at(ExprKind::Key(Box::new(value.clone()), name(key)));
    at(ExprKind::Call(name(MAPPED_TEXT), vec![at(ExprKind::Str(kind.name().into())), value.clone(), span_of("offset"), span_of("end_offset"), text]))
}

/// The function naming a piece kind, with the arguments it takes:
/// `copy(node)`, `mirror(node)`, `renamed(node, text)`.
pub fn piece(name: &str) -> Option<(&'static str, PieceKind, usize)> {
    [("copy", PieceKind::Copy, 1), ("mirror", PieceKind::Mirror, 1), ("renamed", PieceKind::Renamed, 2)].into_iter().find(|(piece, ..)| *piece == name)
}

/// A piece call (`copy(x)`, `mirror(x)`, `renamed(x, text)`) as the marked
/// text it stands for; `None` for any other expression, or the wrong count
/// of arguments.
pub fn piece_call(e: &Expr) -> Option<Expr> {
    let ExprKind::Call(f, args) = &e.kind else { return None };
    let (_, kind, arity) = piece(&f.text)?;
    if args.len() != arity { return None }
    Some(marked(kind, &args[0], args[arity - 1].clone(), e.span))
}
