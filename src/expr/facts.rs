//! Facts native front ends answer about a node (`uses`, `type`, ...): one
//! registry entry each, read like built-in variables. On layers without a
//! native front end they are null.

use crate::native::ts::facts::Facts;

use super::Value;

pub struct Fact {
    pub name: &'static str,
    pub doc: &'static str,
    pub get: fn(&dyn Facts, u32) -> Value<'static>,
}

fn opt<T>(v: Option<T>, f: impl FnOnce(T) -> Value<'static>) -> Value<'static> {
    v.map_or(Value::Null, f)
}

fn words(list: Vec<&'static str>) -> Value<'static> {
    Value::List(list.into_iter().map(Value::from).collect())
}

pub const FACTS: &[Fact] = &[
    Fact {
        name: "uses",
        doc: "native layers: how often the name this identifier declares is used (null if it declares nothing)",
        get: |f, n| opt(f.binding(n), |b| Value::from(b.uses as usize)),
    },
    Fact {
        name: "value_uses",
        doc: "native layers: uses that need the runtime value (not type positions, `typeof x` in types)",
        get: |f, n| opt(f.binding(n), |b| Value::from(b.value_uses as usize)),
    },
    Fact {
        name: "self_uses",
        doc: "native layers: those uses inside the declaration itself (a function calling itself)",
        get: |f, n| opt(f.binding(n), |b| Value::from(b.self_uses as usize)),
    },
    Fact {
        name: "writes",
        doc: "native layers: assignments to the name this identifier declares",
        get: |f, n| opt(f.binding(n), |b| Value::from(b.writes as usize)),
    },
    Fact {
        name: "decl_kind",
        doc: "native layers: what this identifier's declaration is (`const`, `param`, `import`, `class`, `type`, ...)",
        get: |f, n| opt(f.binding(n), |b| Value::from(b.kind)),
    },
    Fact { name: "exported", doc: "native layers: the declaration is `export`ed", get: |f, n| opt(f.binding(n), |b| Value::Bool(b.exported)) },
    Fact {
        name: "ambient",
        doc: "native layers: the declaration is inside `declare` (no runtime code)",
        get: |f, n| opt(f.binding(n), |b| Value::Bool(b.ambient)),
    },
    Fact {
        name: "decl_import",
        doc: "native layers: the module an identifier's binding is imported from (null if not imported)",
        get: |f, n| opt(f.import_of(n), |(from, _)| Value::from(from)),
    },
    Fact {
        name: "decl_doc",
        doc: "native layers: the JSDoc of the declaration an identifier is or refers to, followed into imported files",
        get: |f, n| opt(f.decl_doc(n), Value::from),
    },
    Fact {
        name: "type",
        doc: "native layers: the expression's declared type as text (`Promise<void>`, `never`); null when it cannot tell",
        get: |f, n| opt(f.type_text(n), Value::from),
    },
    Fact {
        name: "type_flags",
        doc: "native layers: what the expression's type is made of: `any`, `never`, `string`, `number`, `boolean`, `bigint`, `symbol`, `literal`, `enum`, `array`, `tuple`, `promise`, `function`, `object`, `null`, `undefined`, `nullable`, `union`, and `unknown` for parts zlint cannot read; null when nothing is known",
        get: |f, n| opt(f.type_flags(n), words),
    },
    Fact {
        name: "type_members",
        doc: "native layers: member names of the expression's type, inherited ones included (`then`, `[Symbol.dispose]`)",
        get: |f, n| opt(f.type_members(n), |names| Value::List(names.into_iter().map(|n| Value::from(n.to_string())).collect())),
    },
    Fact {
        name: "type_param_defaults",
        doc: "native layers: on a call, `new`, type reference or class: each type parameter's default, printed (null where none)",
        get: |f, n| opt(f.type_params(n), |(defaults, _)| Value::List(defaults.into_iter().map(|t| opt(t, Value::from)).collect())),
    },
    Fact {
        name: "type_params_inferred",
        doc: "native layers: on a call or `new`: what the arguments would infer for each type parameter (null where unknown)",
        get: |f, n| opt(f.type_params(n), |(_, inferred)| Value::List(inferred.into_iter().map(|t| opt(t, Value::from)).collect())),
    },
    Fact {
        name: "type_arg_redundant",
        doc: "native layers: on the last type argument of a call, `new`, type reference or class: `default` when it is its parameter's default, `inferred` when the arguments infer it anyway (types that are assignable both ways count as the same); null otherwise",
        get: |f, n| opt(f.type_arg_redundant(n), Value::from),
    },
    Fact {
        name: "resolved",
        doc: "native layers: what a referencing identifier resolves to: `local`, `import`, or `global` (declared nowhere in the file); null for declarations and type names",
        get: |f, n| opt(f.resolved(n), Value::from),
    },
    Fact {
        name: "ref_kind",
        doc: "native layers: on a referencing identifier: the kind of the declaration it resolves to (`const`, `param`, `catch`, ...; see `decl_kind`)",
        get: |f, n| opt(f.ref_kind(n), Value::from),
    },
    Fact {
        name: "member_reads",
        doc: "native layers: on a class property, method or parameter property: reads through `this` (or `x.#name`) inside the class",
        get: |f, n| opt(f.member_uses(n), |u| Value::from(u.reads as usize)),
    },
    Fact {
        name: "member_writes",
        doc: "native layers: on a class member: writes through `this` (or `x.#name`) outside the constructor (`=`, `+=`, `++`, `delete`, destructuring)",
        get: |f, n| opt(f.member_uses(n), |u| Value::from(u.writes as usize)),
    },
    Fact {
        name: "member_ctor_writes",
        doc: "native layers: on a class member: writes inside the constructor",
        get: |f, n| opt(f.member_uses(n), |u| Value::from(u.ctor_writes as usize)),
    },
    Fact {
        name: "uses_this",
        doc: "native layers: on a function or method: its own body uses `this` (arrow functions inside count, nested functions do not)",
        get: |f, n| opt(f.function_flags(n), |b| Value::Bool(b & facts_bits::USES_THIS != 0)),
    },
    Fact {
        name: "uses_arguments",
        doc: "native layers: on a function or method: its own body uses `arguments`",
        get: |f, n| opt(f.function_flags(n), |b| Value::Bool(b & facts_bits::USES_ARGUMENTS != 0)),
    },
    Fact {
        name: "has_yield",
        doc: "native layers: on a function or method: its own body contains `yield`",
        get: |f, n| opt(f.function_flags(n), |b| Value::Bool(b & facts_bits::HAS_YIELD != 0)),
    },
    Fact {
        name: "super_calls",
        doc: "native layers: on a constructor: `all`, `some` or `none` of the paths that leave it normally call `super()` (throwing paths need none)",
        get: |f, n| opt(f.super_calls(n), Value::from),
    },
    Fact {
        name: "regex_error",
        doc: "native layers: on the pattern string of `RegExp(...)`: the syntax error, as `Invalid regular expression: /p/: message` (or the bad flags); null when valid",
        get: |f, n| opt(f.regex_error(n), Value::from),
    },
    Fact {
        name: "regex_problems",
        doc: "native layers: on a regex literal or `RegExp(...)` pattern string: its problems, one per line: `empty_class` (literals), `spaces:N`, `backref:<nested|forward|backward|disjunctive|intoNegativeLookaround>:<backref>:<group>`, `misleading:<kind>`; empty when none",
        get: |f, n| opt(f.regex_problems(n), |list| Value::from(list.join("\n"))),
    },
    Fact {
        name: "precision_lost",
        doc: "native layers: on a number literal: its value is not exactly representable as a double",
        get: |f, n| opt(f.precision_lost(n), Value::Bool),
    },
];

use crate::native::ts::facts as facts_bits;

pub fn fact(name: &str) -> Option<usize> {
    FACTS.iter().position(|f| f.name == name)
}
