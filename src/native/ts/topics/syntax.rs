//! Generic syntax, tokens and gaps; policy lives in .zlint.

use crate::expr::{facts::Fact, Value};
use crate::native::ts::{facts::NativeFile, kinds, tokens};

pub const FACTS: &[Fact] = &[
    Fact {
        name: "space_before",
        doc: "native layers: the gap between the token before this node and the node: `{ text, start, end, spaced, line_break, blank_lines, blank_gaps, comments }` (byte boundaries; `spaced`: whitespace outside comments; `blank_lines`: physical blank lines; `blank_gaps`: blank lines between tokens/comments, excluding comment interiors); null at the start of the file",
        get: |f, n| f.with_file(|file, n| gap_around(file, n, true), n),
    },
    Fact {
        name: "space_after",
        doc: "native layers: the gap between the node and the token after it, as `space_before`; null at the end of the file",
        get: |f, n| f.with_file(|file, n| gap_around(file, n, false), n),
    },
    Fact {
        name: "previous_token",
        doc: "native layers: the layer of the token before this node (`ts_comma`, `ts_open_paren`, `ts_keyword`...; for a name or literal, the layer of its node), for `is` and `not`",
        get: |f, n| f.with_file(|file, n| neighbour(file, n, true), n),
    },
    Fact {
        name: "next_token",
        doc: "native layers: the layer of the token after this node, as `previous_token`",
        get: |f, n| f.with_file(|file, n| neighbour(file, n, false), n),
    },
];

/// The index of the code token right before (or after) `node`.
fn neighbour_index(file: &NativeFile<'_>, node: u32, before: bool) -> Option<usize> {
    let tokens = &file.cached(tokens::lex).0;
    let span = file.syntax.tree.node(node);
    if before {
        tokens.partition_point(|t| t.end <= span.start).checked_sub(1)
    } else {
        let at = tokens.partition_point(|t| t.start < span.end);
        (at < tokens.len()).then_some(at)
    }
}

fn gap_around(file: &NativeFile<'_>, node: u32, before: bool) -> Value<'static> {
    let tokens = &file.cached(tokens::lex).0;
    let span = file.syntax.tree.node(node);
    let Some(i) = neighbour_index(file, node, before) else { return Value::Null };
    let (from, to) = if before { (tokens[i].end, span.start) } else { (span.end, tokens[i].start) };
    tokens::gap_value(file.syntax.src, from as usize, to as usize)
}

fn neighbour(file: &NativeFile<'_>, node: u32, before: bool) -> Value<'static> {
    let Some(i) = neighbour_index(file, node, before) else { return Value::Null };
    let token = file.cached(tokens::lex).0[i];
    match tokens::token_layer(file, &token) {
        Some(kind) => Value::from(kinds::KINDS[kind as usize].0),
        None => Value::Null,
    }
}
