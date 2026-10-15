//! Core facts: bindings and printed types.

use crate::expr::facts::opt;
use crate::expr::Value;

use super::Fact;

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
];
