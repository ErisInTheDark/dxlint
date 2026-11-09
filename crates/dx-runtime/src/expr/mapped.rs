//! How a transform's output text relates to the original: piece kinds, and the
//! marks rewrite templates carry around copied text.

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
