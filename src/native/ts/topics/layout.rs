//! @stylistic spacing, punctuation and line breaks: lexical neighbours.

use crate::expr::facts::Shape;

use super::{Fact, Link};
use crate::native::ts::tokens::lex;

mod punctuation;
mod delimiters;
mod lists;
mod quotes;

pub const FACTS: &[Fact] = &[
    Fact {
        shape: Shape::Record(&[
            ("present", Shape::Scalar), ("offset", Shape::Scalar), ("end", Shape::Scalar),
            ("redundant", Shape::Scalar), ("same_line", Shape::Scalar), ("hazard_before", Shape::Scalar),
            ("hazard_after", Shape::Scalar), ("class_hazard", Shape::Scalar), ("class_field", Shape::Scalar),
            ("one_line_block", Shape::Scalar), ("one_line_class", Shape::Scalar),
        ]),
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
