//! Generic syntax, tokens and gaps; policy lives in .zlint.

use crate::expr::{facts::{Fact, FactFields, Shape}, Value};
use crate::native::ts::{facts::NativeFile, tokens};

const GAP: Shape = Shape::Record(&[
    ("text", Shape::Scalar), ("start", Shape::Scalar), ("end", Shape::Scalar), ("spaced", Shape::Scalar),
    ("line_break", Shape::Scalar), ("blank_lines", Shape::Scalar), ("blank_gaps", Shape::Scalar), ("comments", Shape::Scalar),
]);

const TOKEN_EDGE: Shape = Shape::Record(&[
    ("text", Shape::Scalar), ("start", Shape::Scalar), ("end", Shape::Scalar), ("gap", Shape::Scalar),
    ("spaced", Shape::Scalar), ("same_line", Shape::Scalar), ("line_break", Shape::Scalar),
]);

pub const FACTS: &[Fact] = &[
    Fact {
        shape: GAP,
        name: "space_before",
        doc: "native layers: the gap between the token before this node and the node: `{ text, start, end, spaced, line_break, blank_lines, blank_gaps, comments }` (byte boundaries; `spaced`: whitespace outside comments; `blank_lines`: physical blank lines; `blank_gaps`: blank lines between tokens/comments, excluding comment interiors); null at the start of the file",
        get: |f, n| f.with_file(|file, n| gap_around(file, n, true), n),
    },
    Fact {
        shape: GAP,
        name: "space_after",
        doc: "native layers: the gap between the node and the token after it, as `space_before`; null at the end of the file",
        get: |f, n| f.with_file(|file, n| gap_around(file, n, false), n),
    },
    Fact {
        shape: Fact::SCALAR.shape,
        name: "previous_token",
        doc: "native layers: the layer of the token before this node (`ts_comma`, `ts_open_paren`, `ts_keyword`...; for a name or literal, the layer of its node), for `is` and `not`",
        get: |f, n| f.with_file(|file, n| tokens::neighbour_kind(file, n, true), n),
    },
    Fact {
        shape: Fact::SCALAR.shape,
        name: "next_token",
        doc: "native layers: the layer of the token after this node, as `previous_token`",
        get: |f, n| f.with_file(|file, n| tokens::neighbour_kind(file, n, false), n),
    },
    Fact {
        shape: Shape::Record(&[
            ("before", TOKEN_EDGE), ("after", TOKEN_EDGE), ("parent_start", Shape::Scalar),
        ]),
        name: "token_neighbours",
        doc: "native layers: adjacent tokens as before/after records {text,start,end,gap,spaced,same_line,line_break}; gap retains comments, spaced counts whitespace outside comments, line_break includes comment line breaks; parent_start is the parent's byte offset",
        get: |f, n| f.with_file(tokens::neighbours, n),
    },
];

pub const FACT_FIELDS: &[FactFields] = &[
    FactFields {
        name: "space_before",
        fields: tokens::GAP_FIELDS,
        get: |f, n, field| f.with_field(|file, n, field| gap_projection(file, n, true, field), n, field),
    },
    FactFields {
        name: "space_after",
        fields: tokens::GAP_FIELDS,
        get: |f, n, field| f.with_field(|file, n, field| gap_projection(file, n, false, field), n, field),
    },
];

fn gap_around(file: &NativeFile<'_>, node: u32, before: bool) -> Value<'static> {
    let Some((from, to)) = tokens::gap_bounds(file, node, before) else { return Value::Null };
    tokens::gap_value(file.syntax.src, from, to)
}

fn gap_projection(file: &NativeFile<'_>, node: u32, before: bool, field: Option<usize>) -> Value<'static> {
    let bounds = tokens::gap_bounds(file, node, before);
    let Some(field) = field else { return Value::Bool(bounds.is_some()) };
    let Some((from, to)) = bounds else { return Value::Null };
    tokens::gap_field(file.syntax.src, from, to, tokens::GAP_FIELDS[field])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::tests::with_native_file;

    #[test]
    fn gap_projections_agree_with_complete_records() {
        for source in [
            "",
            "if(x){}",
            "if \t(x)\r\n{}\r\n",
            "if/* x */(x) {}\n",
            "if/* x\r\n\r\n */ \r\n\r\n(x){}",
            "if// x\u{2028}(x)\u{2029}{}",
            "if\u{a0}(x)\u{feff}{}",
            "const x = `raw ${value} end`; /[/]/.test(x);",
        ] {
            with_native_file(source, |file, _| {
                for node in 0..file.syntax.tree.nodes.len() as u32 {
                    for projection in FACT_FIELDS {
                        let fact = FACTS.iter().find(|fact| fact.name == projection.name).expect("registered gap fact");
                        let whole = (fact.get)(file, node);
                        assert_eq!((projection.get)(file, node, None), Value::Bool(!matches!(whole, Value::Null)), "{source:?}");
                        for (field, key) in projection.fields.iter().enumerate() {
                            assert_eq!((projection.get)(file, node, Some(field)), whole.key(key), "{source:?}: {key}");
                        }
                    }
                }
            });
        }
    }
}
