//! JSX layout and shape (`std/ts/jsx`): @stylistic's jsx-* rules.

use crate::expr::facts::Fact;

mod metadata;
mod curly;
mod tags;

pub const FACTS: &[Fact] = &[
    Fact { name: "jsx_curly", doc: "native JSX expressions/spread attributes: attribute context, object literal, expression lines and both brace gaps with/without comments", get: |f, n| f.with_file(curly::gaps, n) },
    Fact { name: "jsx_tag_layout", doc: "native JSX elements: opening/closing bracket locations, alignment, trailing comments and token spacing measurements", get: |f, n| f.with_file(tags::layout, n) },
    Fact { name: "jsx_closing_tag", doc: "native JSX elements/fragments: closing-tag offset, line isolation and tag/line alignment columns", get: |f, n| f.with_file(tags::closing, n) },
    Fact { name: "jsx_tag", doc: "native JSX elements: canonical name, intrinsic/self-closing/empty status, opening and closing bounds, and attribute line groups", get: |f, n| f.with_file(metadata::tag, n) },
    Fact { name: "jsx_attribute", doc: "native JSX attributes: equals offset and adjacent spacing, and raw string quote/value information", get: |f, n| f.with_file(metadata::attribute, n) },
];
