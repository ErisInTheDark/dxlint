//! Core facts: bindings, types, constants, regexes and tokens.

use crate::expr::facts::{opt, words, Shape};
use crate::expr::Value;

use super::super::facts as facts_bits;
use super::Fact;

/// A code token record (`first_token`, `last_token`, `paired_token`).
const TOKEN: Shape = Shape::Record(&[("text", Shape::Scalar), ("kind", Shape::Scalar), ("start", Shape::Scalar), ("end", Shape::Scalar)]);

pub const FACTS: &[Fact] = &[
    Fact {
        shape: Fact::SCALAR.shape,
        name: "uses",
        doc: "native layers: how often the name this identifier declares is used (null if it declares nothing)",
        get: |f, n| opt(f.binding(n), |b| Value::from(b.uses as usize)),
    },
    Fact {
        shape: Fact::SCALAR.shape,
        name: "value_uses",
        doc: "native layers: uses that need the runtime value (not type positions, `typeof x` in types)",
        get: |f, n| opt(f.binding(n), |b| Value::from(b.value_uses as usize)),
    },
    Fact {
        shape: Fact::SCALAR.shape,
        name: "self_uses",
        doc: "native layers: those uses inside the declaration itself (a function calling itself)",
        get: |f, n| opt(f.binding(n), |b| Value::from(b.self_uses as usize)),
    },
    Fact {
        shape: Fact::SCALAR.shape,
        name: "writes",
        doc: "native layers: assignments to the name this identifier declares",
        get: |f, n| opt(f.binding(n), |b| Value::from(b.writes as usize)),
    },
    Fact { shape: Fact::SCALAR.shape, name: "exported", doc: "native layers: the declaration is `export`ed", get: |f, n| opt(f.binding(n), |b| Value::Bool(b.exported)) },
    Fact {
        shape: Fact::SCALAR.shape,
        name: "ambient",
        doc: "native layers: the declaration is inside `declare` (no runtime code)",
        get: |f, n| opt(f.binding(n), |b| Value::Bool(b.ambient)),
    },
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
        name: "resolved",
        doc: "native layers: what a referencing identifier resolves to: `local`, `import`, or `global` (declared nowhere in the file); null for declarations and type names",
        get: |f, n| opt(f.resolved(n), Value::from),
    },
    Fact {
        shape: Fact::SCALAR.shape,
        name: "static_name",
        doc: "native layers: on a property, method, class or enum member, `a.b` or `a[\"b\"]`: its name when known without running anything (`a`, `\"a\"` -> `a`, `0x1` -> `1`, `[`k`]` -> `k`; null for `#private` names), as eslint's `getStaticPropertyName`; on a literal: its value as text (`'\\x41'` -> `A`); null otherwise",
        get: |f, n| opt(f.static_name(n), Value::from),
    },
    Fact {
        shape: Shape::Record(&[("type", Shape::Scalar), ("value", Shape::Scalar)]),
        name: "literal_value",
        doc: "native layers: on a literal (also `-1`, `+1`, parenthesised): `{ type, value }` with the JavaScript type (`string`, `number`, `bigint`, `boolean`, `null`, `regex`) and the value as text (`-0` for negative zero); null otherwise",
        get: |f, n| opt(f.literal_value(n), |(t, v)| Value::record(vec![("type".into(), Value::from(t)), ("value".into(), Value::from(v))])),
    },
    Fact {
        shape: Fact::SCALAR.shape,
        name: "leaves_finally",
        doc: "native layers: on `return`, `throw`, `break` or `continue`: it jumps out of a `finally` block (not just out of a loop, switch or label inside it)",
        get: |f, n| Value::Bool(f.leaves_finally(n)),
    },
    Fact {
        shape: Fact::SCALAR.shape,
        name: "contains_yield",
        doc: "native layers: a `yield` inside the node, outside nested functions (on a `for` loop: outside its initializer)",
        get: |f, n| Value::Bool(f.contains_yield(n)),
    },
    Fact {
        shape: Fact::SCALAR.shape,
        name: "source_type",
        doc: "native layers: the file's eslint `sourceType`: `commonjs` (`.cjs`, `.cts`), `module` (`.mjs`, `.mts`, or any file that imports or exports) or `script`; the same on every node",
        get: |f, n| f.with_file(|file, _| Value::from(file.source_type()), n),
    },
    Fact {
        shape: Fact::SCALAR.shape,
        name: "unparen_offset",
        doc: "native layers: the offset of the node with the parentheses around it removed (where eslint, which has no parenthesis nodes, reports it), for `report at`",
        get: |f, n| opt(f.unparen_offset(n), Value::from),
    },
    Fact {
        shape: Fact::SCALAR.shape,
        name: "unexpected_multiline",
        doc: "native layers: on a call, `a[b]`, tagged template or `a / b / g`: when a line break before its `(`, `[`, template or `/` lets the next line continue the expression (eslint's no-unexpected-multiline): `{ offset, kind }` with kind `function`, `property`, `taggedTemplate` or `division`",
        get: |f, n| opt(f.unexpected_multiline(n), |(at, kind)| Value::record(vec![("offset".into(), Value::from(at)), ("kind".into(), Value::from(kind))])),
    },
    Fact {
        shape: Shape::List(&Shape::Record(&[("offset", Shape::Scalar), ("arithmetic", Shape::Scalar)])),
        name: "optional_chain_risks",
        doc: "native layers: the optional chains whose short-circuit `undefined` this node would trip on (eslint's no-unsafe-optional-chaining: called, destructured, spread, `in`, `extends`, `for of`...; also through `&&`, `||`, `??`, `?:`, `await`), as records `{ offset, arithmetic }` (`arithmetic` for `+`, `-`, `*`, ... that make `NaN`)",
        get: |f, n| {
            Value::List(
                f.optional_chain_risks(n)
                    .into_iter()
                    .map(|(at, arith)| Value::record(vec![("offset".into(), Value::from(at)), ("arithmetic".into(), Value::Bool(arith))]))
                    .collect(),
            )
        },
    },
    Fact {
        shape: Shape::List(&Shape::Record(&[("offset", Shape::Scalar), ("name", Shape::Scalar), ("member", Shape::Scalar)])),
        name: "self_assignments",
        doc: "native layers: on an assignment (`=`, `&&=`, `||=`, `??=`): what it assigns to itself, also inside destructuring (`[a, b] = [a, b]`), as records `{ offset, name, member }` (`member` for `a.b = a.b`)",
        get: |f, n| {
            opt(f.self_assignments(n), |list| {
                Value::List(
                    list.into_iter()
                        .map(|(at, name, member)| {
                            Value::record(vec![("offset".into(), Value::from(at)), ("name".into(), Value::from(name)), ("member".into(), Value::Bool(member))])
                        })
                        .collect(),
                )
            })
        },
    },
    Fact {
        shape: Fact::SCALAR.shape,
        name: "duplicate_condition",
        doc: "native layers: on an `if`: an earlier branch of its `else if` chain already covers its test (eslint's no-dupe-else-if: by tokens, `&&`/`||` in any order, `a` covers `a && b`)",
        get: |f, n| opt(f.duplicate_condition(n), Value::Bool),
    },
    Fact {
        shape: Fact::SCALAR.shape,
        name: "duplicate_case",
        doc: "native layers: on a `case`: an earlier case of its switch has the same test (by tokens, whatever the spacing and comments)",
        get: |f, n| opt(f.duplicate_case(n), Value::Bool),
    },
    Fact {
        shape: Fact::SCALAR.shape,
        name: "wrong_direction",
        doc: "native layers: on a `for`: its update moves the counter away from where its test ends the loop (eslint's for-direction; steps are evaluated statically: `i -= -1`, `i += 1n`)",
        get: |f, n| opt(f.wrong_direction(n), Value::Bool),
    },
    Fact {
        shape: Fact::SCALAR.shape,
        name: "duplicate_parameters",
        doc: "native layers: on a function, arrow or method: the names its parameters declare more than once",
        get: |f, n| opt(f.duplicate_parameters(n), |names| Value::List(names.into_iter().map(Value::from).collect())),
    },
    Fact {
        shape: Fact::SCALAR.shape,
        name: "imported_name",
        doc: "native layers: the name an identifier's binding imports (`default`, `*` for a namespace, or the export's name); null if not imported",
        get: |f, n| opt(f.import_of(n), |(_, name)| Value::from(name)),
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
        shape: Fact::SCALAR.shape,
        name: "uses_this",
        doc: "native layers: on a function or method: its own body uses `this` (arrow functions inside count, nested functions do not)",
        get: |f, n| opt(f.function_flags(n), |b| Value::Bool(b & facts_bits::USES_THIS != 0)),
    },
    Fact {
        shape: Fact::SCALAR.shape,
        name: "uses_arguments",
        doc: "native layers: on a function or method: its own body uses `arguments`",
        get: |f, n| opt(f.function_flags(n), |b| Value::Bool(b & facts_bits::USES_ARGUMENTS != 0)),
    },
    Fact {
        shape: Fact::SCALAR.shape,
        name: "function_name",
        doc: "native layers: on a function, arrow or method: how eslint names it in messages (`function 'f'`, `arrow function`, `static getter 'x'`, `private method #y`, `constructor`)",
        get: |f, n| opt(f.function_name_with_kind(n), Value::from),
    },
    Fact {
        shape: Fact::SCALAR.shape,
        name: "enclosing_function_name",
        doc: "native layers: `function_name` of the nearest function, arrow or method around the node",
        get: |f, n| opt(f.enclosing_function_name(n), Value::from),
    },
    Fact {
        shape: Fact::SCALAR.shape,
        name: "precision_lost",
        doc: "native layers: on a number literal: its value is not exactly representable as a double",
        get: |f, n| opt(f.precision_lost(n), Value::Bool),
    },
    Fact {
        shape: Fact::SCALAR.shape,
        name: "identifier_name",
        doc: "native layers: on an identifier or private name, the name as JavaScript reads it (`\\u0066oo` is `foo`)",
        get: |f, n| f.with_file(|file, n| {
            let s = &file.syntax;
            match s.kind(n) {
                Some(crate::native::ts::kinds::Kind::Ident | crate::native::ts::kinds::Kind::PrivateName) => {
                    Value::from(crate::native::ts::statics::identifier_name(s.text(n)))
                }
                _ => Value::Null,
            }
        }, n),
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
