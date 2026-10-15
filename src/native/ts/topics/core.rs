//! Core facts: bindings, types, constants, regexes and tokens.

use crate::expr::facts::{opt, words, Shape};
use crate::expr::Value;

use super::Fact;

/// A code token record (`first_token`, `last_token`, `paired_token`).
const TOKEN: Shape = Shape::Record(&[("text", Shape::Scalar), ("kind", Shape::Scalar), ("start", Shape::Scalar), ("end", Shape::Scalar)]);

pub const FACTS: &[Fact] = &[
    Fact { shape: Fact::SCALAR.shape, name: "exported", doc: "native layers: the declaration is `export`ed", get: |f, n| opt(f.binding(n), |b| Value::Bool(b.exported)) },
    Fact {
        shape: Fact::SCALAR.shape,
        name: "type",
        doc: "native layers: the expression's declared type as text (`Promise<void>`, `never`); null when it cannot tell",
        get: |f, n| opt(f.type_text(n), Value::from),
    },
    Fact {
        shape: Fact::SCALAR.shape,
        name: "type_flags",
        doc: "native layers: what the expression's type is made of: `any`, `never`, `string`, `number`, `boolean`, `bigint`, `symbol`, `literal`, `enum`, `array`, `tuple`, `promise`, `function`, `object`, `null`, `undefined`, `nullable`, `union`, and `unknown` for parts zlint cannot read; null when nothing is known",
        get: |f, n| opt(f.type_flags(n), words),
    },
    Fact {
        shape: Fact::SCALAR.shape,
        name: "type_members",
        doc: "native layers: member names of the expression's type, inherited ones included (`then`, `[Symbol.dispose]`)",
        get: |f, n| opt(f.type_members(n), |names| Value::List(names.into_iter().map(|n| Value::from(n.to_string())).collect())),
    },
    Fact {
        shape: Fact::SCALAR.shape,
        name: "type_parameter_defaults",
        doc: "native layers: on a call, `new`, type reference or class: each type parameter's default, printed (null where none)",
        get: |f, n| opt(f.type_defaults(n), |defaults| Value::List(defaults.into_iter().map(|t| opt(t, Value::from)).collect())),
    },
    Fact {
        shape: Fact::SCALAR.shape,
        name: "type_argument_redundant",
        doc: "native layers: on the last type argument of a call, `new`, type reference or class: `default` when it is its parameter's default, `inferred` when the arguments infer it anyway (types that are assignable both ways count as the same); null otherwise",
        get: |f, n| opt(f.type_arg_redundant(n), Value::from),
    },
    Fact {
        shape: Fact::SCALAR.shape,
        name: "member_writes",
        doc: "native layers: on a class member: direct reassignments outside its constructor, resolved by receiver type (`=`, `+=`, `++`, `delete`, destructuring)",
        get: |f, n| opt(f.member_uses(n), |u| Value::from(u.writes as usize)),
    },
    Fact {
        shape: Fact::SCALAR.shape,
        name: "member_constructor_writes",
        doc: "native layers: on an instance member: direct reassignments inside its constructor, excluding nested functions",
        get: |f, n| opt(f.member_uses(n), |u| Value::from(u.ctor_writes as usize)),
    },
    Fact {
        shape: Fact::SCALAR.shape,
        name: "member_constructor_indirect_writes",
        doc: "native layers: on an instance member: constructor reassignments through a receiver other than direct `this`",
        get: |f, n| opt(f.member_uses(n), |u| Value::from(u.ctor_indirect_writes as usize)),
    },
    Fact {
        shape: TOKEN,
        name: "paired_token",
        doc: "native layers: on a node starting with a `(`, `[` or `{` token (a bracket token node), or ending with `)`, `]` or `}`: the matching bracket, as `first_token`",
        get: |f, n| f.with_file(crate::native::ts::tokens::paired, n),
    },
    Fact {
        shape: TOKEN,
        name: "first_token",
        doc: "native layers: the node's first code token: `{text, kind, start, end}`; `is` tests its kind (`first_token is ts_identifier`)",
        get: |f, n| f.with_file(|file, n| crate::native::ts::tokens::edge(file, n, false), n),
    },
    Fact {
        shape: TOKEN,
        name: "last_token",
        doc: "native layers: the node's last code token, as `first_token`",
        get: |f, n| f.with_file(|file, n| crate::native::ts::tokens::edge(file, n, true), n),
    },
    Fact {
        shape: Shape::List(&Shape::Scalar),
        name: "token_texts",
        doc: "native layers: the node's code tokens' texts, in order (comments and spacing dropped); equal lists mean the same tokens",
        get: |f, n| f.with_file(|file, n| Value::list(file.tokens(n).into_iter().map(|t| Value::from(t.to_string())).collect()), n),
    },
    Fact {
        shape: Fact::SCALAR.shape,
        name: "compiler_options",
        doc: "native layers: the file's boolean tsconfig compiler options (`compiler_options.strictNullChecks`), `strict` filling in the options it implies; unset ones are null",
        get: |f, n| f.with_file(|file, _| {
            let id = file.project.file_id(file.path);
            Value::Record(file.project.compiler_flags(id).into_iter().map(|(k, v)| (k.into(), Value::Bool(v))).collect())
        }, n),
    },
];
