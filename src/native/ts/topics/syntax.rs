//! Generic syntax, tokens and gaps; policy lives in .zlint.

use crate::expr::{facts::{Fact, FactFields}, Value};
use crate::native::ts::{facts::NativeFile, kinds, tokens};

pub const FACTS: &[Fact] = &[
    Fact {
        shape: Fact::SCALAR.shape,
        name: "space_before",
        doc: "native layers: the gap between the token before this node and the node: `{ text, start, end, spaced, line_break, blank_lines, blank_gaps, comments }` (byte boundaries; `spaced`: whitespace outside comments; `blank_lines`: physical blank lines; `blank_gaps`: blank lines between tokens/comments, excluding comment interiors); null at the start of the file",
        get: |f, n| f.with_file(|file, n| gap_around(file, n, true), n),
    },
    Fact {
        shape: Fact::SCALAR.shape,
        name: "space_after",
        doc: "native layers: the gap between the node and the token after it, as `space_before`; null at the end of the file",
        get: |f, n| f.with_file(|file, n| gap_around(file, n, false), n),
    },
    Fact {
        shape: Fact::SCALAR.shape,
        name: "previous_token",
        doc: "native layers: the layer of the token before this node (`ts_comma`, `ts_open_paren`, `ts_keyword`...; for a name or literal, the layer of its node), for `is` and `not`",
        get: |f, n| f.with_file(|file, n| neighbour(file, n, true), n),
    },
    Fact {
        shape: Fact::SCALAR.shape,
        name: "next_token",
        doc: "native layers: the layer of the token after this node, as `previous_token`",
        get: |f, n| f.with_file(|file, n| neighbour(file, n, false), n),
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

/// The index of the code token right before (or after) `node`.
fn neighbour_index(tokens: &[super::super::lexer::Token], span: &crate::grammar::Node, before: bool) -> Option<usize> {
    if before {
        tokens.partition_point(|t| t.end <= span.start).checked_sub(1)
    } else {
        let at = tokens.partition_point(|t| t.start < span.end);
        (at < tokens.len()).then_some(at)
    }
}

fn gap_around(file: &NativeFile<'_>, node: u32, before: bool) -> Value<'static> {
    let Some((from, to)) = gap_bounds(file, node, before) else { return Value::Null };
    tokens::gap_value(file.syntax.src, from, to)
}

fn gap_bounds(file: &NativeFile<'_>, node: u32, before: bool) -> Option<(usize, usize)> {
    let tokens = &file.cached(tokens::lex).0;
    let span = file.syntax.tree.node(node);
    let i = neighbour_index(tokens, span, before)?;
    let (from, to) = if before { (tokens[i].end, span.start) } else { (span.end, tokens[i].start) };
    Some((from as usize, to as usize))
}

fn gap_projection(file: &NativeFile<'_>, node: u32, before: bool, field: Option<usize>) -> Value<'static> {
    let bounds = gap_bounds(file, node, before);
    let Some(field) = field else { return Value::Bool(bounds.is_some()) };
    let Some((from, to)) = bounds else { return Value::Null };
    tokens::gap_field(file.syntax.src, from, to, tokens::GAP_FIELDS[field])
}

fn neighbour(file: &NativeFile<'_>, node: u32, before: bool) -> Value<'static> {
    let tokens = file.cached(tokens::lex);
    let Some(i) = neighbour_index(&tokens.0, file.syntax.tree.node(node), before) else { return Value::Null };
    let token = tokens.0[i];
    match tokens::token_layer(file, &token) {
        Some(kind) => Value::from(kinds::KINDS[kind as usize].0),
        None => Value::Null,
    }
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
