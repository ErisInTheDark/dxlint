//! @stylistic code shape across lines (`std/ts/structure`): indentation,
//! parentheses, wrapping.

use crate::expr::{facts::{Fact, Shape}, Value};
use crate::native::ts::facts::NativeFile;
use crate::native::ts::kinds::Kind;

mod declarations;
mod operators;
mod lists;
mod indentation;

pub const FACTS: &[Fact] = &[
    Fact { shape: Fact::SCALAR.shape, name: "precedence", doc: "native expressions and types: binding strength, increasing from sequence to primary", get: |f, n| f.with_file(|f, n| Value::from(f64::from(operators::precedence(&f.syntax, n))), n) },
    Fact { shape: Fact::SCALAR.shape, name: "bom", doc: "native ts_program: source starts with a Unicode BOM", get: |f, n| f.with_file(bom, n) },
    Fact { shape: Fact::SCALAR.shape, name: "declaration_groups", doc: "native variable declarations: counts, previous statement and preceding groups in the owning scope", get: |f, n| f.with_file(declarations::groups, n) },
    Fact { shape: Shape::List(&Shape::Record(&[
        ("offset", Shape::Scalar), ("end", Shape::Scalar), ("comments", Shape::Scalar),
        ("prev", Shape::Scalar), ("next", Shape::Scalar),
        ("spaced", Shape::Scalar), ("broken", Shape::Scalar), ("kind", Shape::Scalar),
        ("brackets", Shape::Scalar), ("count", Shape::Scalar), ("single", Shape::Scalar),
        ("node_single", Shape::Scalar), ("first_broken", Shape::Scalar),
        ("empty_comments", Shape::Scalar), ("mode", Shape::Scalar),
    ])), name: "list_layout", doc: "native list owners: boundaries, item counts and spacing or wrapping pairs", get: |f, n| f.with_file(lists::sites, n) },
    Fact { shape: Fact::SCALAR.shape, name: "binary_operand_indents", doc: "native program: operand-line indentation constraints in traversal order", get: |f, n| f.with_file(indentation::operands, n) },
];

fn bom(file: &NativeFile, node: u32) -> Value<'static> {
    if file.syntax.is(node, Kind::Program) {
        Value::Bool(file.syntax.src.starts_with('\u{feff}'))
    } else {
        Value::Null
    }
}
