//! @stylistic spacing, punctuation and line breaks: lexical neighbours.

use crate::expr::facts::{Fact, Link};
use crate::expr::Value;
use crate::native::ts::facts::NativeFile;
use crate::native::ts::lexer::Token;

use crate::native::ts::tokens::{gap_has_space, lex};

mod punctuation;
mod delimiters;
mod lists;
mod quotes;

pub const FACTS: &[Fact] = &[
    Fact {
        shape: Fact::SCALAR.shape,
        name: "statement_semicolon",
        doc: "native semicolon-owning statements: {present,offset,end,redundant,same_line,hazard_before,hazard_after,class_hazard,class_field,one_line_block,one_line_class}; null for declarations owned elsewhere and for-header variables",
        get: |f, n| f.with_file(punctuation::statement_semicolon, n),
    },
    Fact {
        shape: Fact::SCALAR.shape,
        name: "list_field",
        doc: "native comma: field of its containing syntax list (argument, parameter, type_argument, type_parameter, element, property, member, specifier, declaration, expression, extends, implements); null outside parser lists",
        get: |f, n| f.with_file(lists::list_field, n),
    },
    Fact {
        shape: Fact::SCALAR.shape,
        name: "property_quote_sites",
        doc: "native properties/signatures/enums and object/interface/type/enum groups: {group,quoted,numeric,redundant,redundant_number,keyword,name,offset}; computed keys, methods and shorthand object properties exempt",
        get: |f, n| f.with_file(quotes::property_quote_sites, n),
    },
    Fact {
        shape: Fact::SCALAR.shape,
        name: "token_neighbours",
        doc: "native layers: adjacent tokens as before/after records {text,start,end,gap,spaced,same_line,line_break}; gap retains comments, spaced counts whitespace outside comments, line_break includes comment line breaks; parent_start is the parent's byte offset",
        get: |f, n| f.with_file(token_neighbours, n),
    },
    Fact {
        shape: Fact::SCALAR.shape,
        name: "trailing_comma_sites",
        doc: "native list owners: {kind,offset,trailing,multiline,allowed,tsx_single}; offset is comma or insertion point, multiline compares last item/comma with closing token",
        get: |f, n| f.with_file(lists::trailing_comma_sites, n),
    },
];

pub const LINKS: &[Link] = &[
    Link {
        name: "list_owner",
        doc: "native comma: node owning its syntax list; null outside parser lists",
        many: false,
        get: |f, n| f.nodes_of(lists::list_owner, n),
    },
    Link {
        name: "list_previous",
        doc: "native comma: preceding occupied item of the same syntax list, skipping array holes; null before the first occupied item",
        many: false,
        get: |f, n| f.nodes_of(lists::list_previous, n),
    },
];

fn edge(src: &str, token: Option<&Token>, start: usize, end: usize) -> Value<'static> {
    let Some(token) = token else { return Value::Null };
    let token_start = token.start as usize;
    let token_end = token.end as usize;
    let gap = &src[start..end];
    let line_break = gap.contains(['\n', '\r', '\u{2028}', '\u{2029}']);
    Value::record([
        ("text".into(), Value::text_copy(&src[token_start..token_end])),
        ("start".into(), Value::from(token_start)),
        ("end".into(), Value::from(token_end)),
        ("gap".into(), Value::text_copy(gap)),
        ("spaced".into(), Value::Bool(gap_has_space(gap))),
        ("same_line".into(), Value::Bool(!line_break)),
        ("line_break".into(), Value::Bool(line_break)),
    ])
}

fn token_neighbours(file: &NativeFile<'_>, node: u32) -> Value<'static> {
    let src = file.syntax.src;
    let syntax_node = file.syntax.tree.node(node);
    let start = syntax_node.start as usize;
    let end = syntax_node.end as usize;
    let lexed = file.cached(lex);
    let before = lexed.0.partition_point(|token| token.end as usize <= start).checked_sub(1).and_then(|i| lexed.0.get(i));
    let after = lexed.0.get(lexed.0.partition_point(|token| (token.start as usize) < end));
    Value::record([
        ("before".into(), edge(src, before, before.map_or(start, |token| token.end as usize), start)),
        ("after".into(), edge(src, after, end, after.map_or(end, |token| token.start as usize))),
        ("parent_start".into(), file.syntax.parent(node).map_or(Value::Null, |parent| Value::from(file.syntax.tree.node(parent).start as usize))),
    ])
}
