//! eslint core suggestion rules (`std/ts/suggestions`, `std/js/problems`).

use crate::expr::facts::Shape;

use super::Fact;

pub(crate) mod expressions;
mod objects;
mod coercion;
mod functions;
mod statements;
mod references;

pub const FACTS: &[Fact] = &[
    Fact { shape: Fact::SCALAR.shape, name: "reference_key", doc: "native layers: collision-safe comparable reference key, normalising static property access; null for unsupported expressions", get: |f, n| f.with_file(|file, node| references::key(file, node, false), n) },
    Fact { shape: Fact::SCALAR.shape, name: "reference_key_strict", doc: "native layers: comparable reference key preserving member/index syntax and literal key types; null for unsupported expressions", get: |f, n| f.with_file(|file, node| references::key(file, node, true), n) },
    Fact { shape: Fact::SCALAR.shape, name: "restricted_binding", doc: "native layers: declaration shadows a restricted global name, including merged declarations and unsafe undefined bindings", get: |f, n| f.with_file(functions::restricted_binding, n) },
    Fact { shape: Fact::SCALAR.shape, name: "nonoctal_decimal_escapes", doc: "native layers: unescaped decimal escape spellings and source offsets in a string literal", get: |f, n| f.with_file(expressions::decimal_escapes, n) },
    Fact { shape: Shape::Record(&[
        ("block", Shape::Scalar), ("name", Shape::Scalar), ("condition", Shape::Scalar),
        ("multi", Shape::Scalar), ("multiline", Shape::Scalar), ("nested", Shape::Scalar),
        ("multi_consistent", Shape::Scalar), ("multiline_consistent", Shape::Scalar), ("nested_consistent", Shape::Scalar),
    ]), name: "curly_body", doc: "native layers: control body name, brace status and all multi/line/nest brace expectations with chain consistency", get: |f, n| f.with_file(statements::curly_body, n) },
    Fact { shape: Shape::Record(&[
        ("declaration", Shape::Scalar), ("exported", Shape::Scalar), ("exempt", Shape::Scalar),
        ("arrow", Shape::Scalar), ("captures", Shape::Scalar), ("annotated", Shape::Scalar), ("offset", Shape::Scalar),
    ]), name: "function_style", doc: "native layers: function declaration/variable form, export policy, overload and arrow exceptions, and report offset", get: |f, n| f.with_file(functions::style, n) },
    Fact { shape: Shape::Record(&[("kind", Shape::Scalar), ("operator", Shape::Scalar), ("recommendation", Shape::Scalar)]), name: "implicit_coercion", doc: "native layers: shorthand conversion kind, operator, and explicit recommendation", get: |f, n| f.with_file(coercion::coercion, n) },
    Fact { shape: Fact::SCALAR.shape, name: "could_be_error", doc: "native expressions: syntax permits an Error-valued result through possible value branches", get: |f, n| f.with_file(|file, node| crate::expr::Value::Bool(expressions::could_be_error(file, node)), n) },
    Fact { shape: Shape::Record(&[
        ("name", Shape::Scalar), ("concise", Shape::Scalar), ("method", Shape::Scalar), ("function", Shape::Scalar),
        ("arrow", Shape::Scalar), ("quoted", Shape::Scalar), ("redundant", Shape::Scalar), ("constructor", Shape::Scalar),
        ("annotated", Shape::Scalar), ("proto", Shape::Scalar), ("computed", Shape::Scalar),
    ]), name: "object_shorthand_member", doc: "native layers: object member shorthand eligibility, constructor and annotation exceptions, and arrow lexical safety", get: |f, n| f.with_file(objects::shorthand, n) },
    Fact { shape: Fact::SCALAR.shape, name: "object_shorthand_consistency", doc: "native layers: mixed shorthand forms or all-redundant long forms in an object literal", get: |f, n| f.with_file(objects::consistency, n) },
];
