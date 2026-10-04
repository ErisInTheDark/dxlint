//! Configured limits and bans (`std/ts/restrictions`): sizes, restricted
//! names and imports, identifier shapes, sort orders.

use crate::expr::facts::Fact;

mod comments;
#[cfg(test)]
mod limits;
mod names;
mod imports;
mod numbers;

pub const FACTS: &[Fact] = &[
    Fact { name: "restriction_number", doc: "native numeric literals: signed raw/value and offset, constant, object, array, radix, JSX, defaults and TypeScript contexts", get: |f, n| f.with_file(numbers::context, n) },
    Fact { name: "restriction_duplicate_imports", doc: "native imports and exports: duplicate mergeable sources, separated by type-import option and import/export origin", get: |f, n| f.with_file(imports::duplicates, n) },
    Fact { name: "restriction_import_order", doc: "native imports: declaration syntax and alphabetic order against the previous import, and first unsorted named member", get: |f, n| f.with_file(imports::order, n) },
    Fact { name: "restriction_comments", doc: "native layers: file comments with bodies, offsets, inline and consecutive status and initial letter", get: |f, n| f.with_file(comments::comments, n) },
    Fact { name: "restriction_lines", doc: "native layers: lines covered by a file or function: offset, blank and comment-only for each line", get: |f, n| f.with_file(comments::lines, n) },
    Fact { name: "restriction_name", doc: "native layers: identifier eligibility for id-denylist, id-length and id-match, with name and property/declaration/destructuring roles", get: |f, n| f.with_file(names::name, n) },
];
