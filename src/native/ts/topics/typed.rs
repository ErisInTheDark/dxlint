//! typescript-eslint's type-aware rules (`std/ts/typed`).

use crate::expr::facts::Shape;
use crate::expr::Value;
use crate::native::ts::facts::NativeFile;
use crate::native::ts::kinds::{Field, Kind};
use crate::native::ts::types::{Checker, Ty};

use super::Fact;

mod template;
mod type_properties;
mod type_structure;
mod enum_types;
mod properties;
mod unsafe_types;
mod awaitability;
mod callables;
mod assertions;
mod accessors;
mod conversions;
mod constituents;
mod regexp;
mod redundant;
mod this_return;
mod transfers;
mod assignments;
mod arguments;
mod enums;
pub(super) mod specifiers;
mod errors;
mod promises;
mod empty_types;
mod defaults;
mod return_await;

fn is_typescript(file: &NativeFile<'_>) -> bool {
    file.path.extension().and_then(|extension| extension.to_str()).is_some_and(|extension| matches!(extension, "ts" | "tsx" | "mts" | "cts"))
}

pub const FACTS: &[Fact] = &[
    Fact {
        shape: Fact::SCALAR.shape,
        name: "type_awaitability",
        doc: "native expressions: structural thenable union any/all certainty, mutual assignability of awaited and ordinary union arms, and analysis completeness",
        get: |f, n| f.with_file(awaitability::type_awaitability, n),
    },
    Fact {
        shape: Fact::SCALAR.shape,
        name: "callable_contracts",
        doc: "native expressions: callable_contracts(site, source, member_path?, fields?) gives callable return kinds and thenability at the requested member path (default root), through receiver contracts and direct heritage; incomplete rows indicate unresolved signatures or work limits. Opt-in contextual_returns contains raw recursive kind/parts result profiles; opt-in declared_target gives declaration-bound receiver returns before argument inference, retaining explicit type arguments. null means unavailable, opaque or work-limited",
        get: |f, n| f.with_file(callables::contracts, n),
    },
    Fact {
        shape: Fact::SCALAR.shape,
        name: "static_number",
        doc: "native expressions: statically known numeric value, including numeric/boolean/bigint literals, arithmetic and unmodified local bindings; null when unresolved",
        get: |f, n| f.with_file(|file, node| file.static_number(node).map_or(Value::Null, Value::from), n),
    },
    Fact {
        shape: Fact::SCALAR.shape,
        name: "type_flow",
        doc: "native expressions: type_flow(site, source) gives source and expected type relations for declared, contextual and heritage contracts; incomplete records indicate traversal limits",
        get: |f, n| f.with_file(enum_types::flow, n),
    },
    Fact {
        shape: Fact::SCALAR.shape,
        name: "property_info",
        doc: "native expressions: static member or index access declaration presence and winning declaration visibility, even when its value type is unresolved",
        get: |f, n| f.with_file(properties::property_info, n),
    },
    Fact {
        shape: Fact::SCALAR.shape,
        name: "index_key_types",
        doc: "native values and types: constrained index-signature key domains as text and string_like records, inherited domains included",
        get: |f, n| f.with_file(properties::index_key_types, n),
    },
    Fact {
        shape: Fact::SCALAR.shape,
        name: "type_structure",
        doc: "native values and types: recursive structure and queried winning members; optional second argument selects fields, default kind/parts/names/generic_names/aliases/members; generic arguments, declaration/value, call/construct/property/iteration metadata and root binding writes are opt-in; optional third constrained argument defaults true, false retains open parameters and named wrappers",
        get: |f, n| f.with_file(|file, node| {
            let args = crate::expr::facts::args();
            let names = args.first().map(|value| value.items().iter().map(ToString::to_string).collect::<Vec<_>>()).unwrap_or_default();
            type_structure::structure(file, node, &names)
        }, n),
    },
    Fact {
        shape: Fact::SCALAR.shape,
        name: "type_has_void",
        doc: "native values and types: a union or intersection constituent is void, distinct from undefined",
        get: |f, n| f.with_file(|file, node| Value::Bool(type_properties::has_void(&file.type_of(node))), n),
    },
    Fact {
        shape: Fact::SCALAR.shape,
        name: "type_literal_values",
        doc: "native values and types: literal union/intersection constituents as type/value records; bigint values retain literal text",
        get: |f, n| f.with_file(|file, node| type_properties::literal_values(&file.type_of(node)), n),
    },
    Fact {
        shape: Fact::SCALAR.shape,
        name: "type_is_string_like",
        doc: "native values and types: every constrained union arm is provably string-like, excluding any and unresolved intersections",
        get: |f, n| f.with_file(|file, node| Value::Bool(type_properties::string_like(&file.constrained_type_of(node), &file.typing().checker)), n),
    },
    Fact {
        shape: Fact::SCALAR.shape,
        name: "type_is_parameter",
        doc: "native values and types: the type is an open type parameter, before applying its constraint",
        get: |f, n| f.with_file(|file, node| Value::Bool(matches!(file.type_of(node), Ty::Param(_))), n),
    },
    Fact {
        shape: Fact::SCALAR.shape,
        name: "type_has_enum",
        doc: "native values and types: a constrained union or intersection constituent has enum identity",
        get: |f, n| f.with_file(|file, node| Value::Bool(type_properties::has_enum(&file.constrained_type_of(node), &file.typing().checker)), n),
    },
    Fact {
        shape: Fact::SCALAR.shape,
        name: "callable",
        doc: "native expressions: the type supplies call signatures (every union arm, or an intersection arm)",
        get: |f, n| f.with_file(|file, node| Value::Bool(file.typing().checker.callable_type(&file.type_of(node))), n),
    },
    Fact {
        shape: Fact::SCALAR.shape,
        name: "type_iterable",
        doc: "native expressions: constrained type supports synchronous iteration; null when type analysis is unresolved",
        get: |f, n| f.with_file(|file, node| file.typing().checker.iterable_type(&file.constrained_type_of(node)).map_or(Value::Null, Value::Bool), n),
    },
    Fact {
        shape: Fact::SCALAR.shape,
        name: "type_opaque",
        doc: "native expressions: type computation is unsupported or unresolved, distinct from explicit unknown and open type parameters",
        get: |f, n| f.with_file(|file, node| Value::Bool(file.type_of(node).is_opaque()), n),
    },
    Fact {
        shape: Fact::SCALAR.shape,
        name: "builtin_type",
        doc: "native expressions: builtin_type(name) asks whether the type is the named standard-library type or derives from it",
        get: |f, n| f.with_file(|file, node| {
            let args = crate::expr::facts::args();
            args.first().map_or(Value::Null, |name| Value::Bool(file.typing().checker.is_builtin_type_or_base(&file.type_of(node), &name.to_string())))
        }, n),
    },
    Fact {
        shape: Fact::SCALAR.shape,
        name: "builtin_constructor",
        doc: "native expressions: builtin_constructor(name) asks whether the value is a standard-library constructor or derives from it",
        get: |f, n| f.with_file(|file, node| {
            let args = crate::expr::facts::args();
            args.first().map_or(Value::Null, |name| Value::Bool(file.typing().checker.is_builtin_constructor_or_base(&file.type_of(node), &name.to_string())))
        }, n),
    },
    Fact {
        shape: Fact::SCALAR.shape,
        name: "static_string",
        doc: "native expressions: constant evaluated text, including templates, concatenation and unmodified variables; null when unknown",
        get: |f, n| f.with_file(|file, node| file.static_string(node).map_or(Value::Null, Value::from), n),
    },
    Fact {
        shape: Shape::List(&Shape::Record(&[
            ("expression", Shape::Scalar), ("offset", Shape::Scalar), ("awaited", Shape::Scalar), ("certainty", Shape::Scalar), ("handling", Shape::Scalar),
        ])),
        name: "returned_awaitables",
        doc: "native async functions: returned expression nodes, offsets, awaited state, branch certainty and error/resource handling context",
        get: |f, n| f.with_file(return_await::findings, n),
    },
    Fact {
        shape: Fact::SCALAR.shape,
        name: "useless_default",
        doc: "native defaults: source property or contextual parameter cannot use the fallback",
        get: |f, n| f.with_file(defaults::finding, n),
    },
    Fact {
        shape: Fact::SCALAR.shape,
        name: "unsafe_assertion",
        doc: "native assertions: unsafe generic any transfers, narrowing and type parameter constraints",
        get: |f, n| f.with_file(assertions::unsafe_assertion, n),
    },
    Fact {
        shape: Fact::SCALAR.shape,
        name: "generated_empty_object",
        doc: "native type operations: provably resolved empty object, excluding unresolved mapped keys",
        get: |f, n| f.with_file(empty_types::generated, n),
    },
    Fact {
        shape: crate::expr::facts::Shape::Record(&[
            ("name", crate::expr::facts::Shape::Scalar), ("offset", crate::expr::facts::Shape::Scalar),
        ]),
        name: "missing_await",
        doc: "native async functions: no own await, awaited loop/using, thenable return or async yield",
        get: |f, n| f.with_file(promises::missing_await, n),
    },
    Fact {
        shape: Fact::SCALAR.shape,
        name: "unsafe_catch_callbacks",
        doc: "native calls: inline rejection handlers whose first parameter is not unknown",
        get: |f, n| f.with_file(promises::unsafe_catch, n),
    },
    Fact {
        shape: Fact::SCALAR.shape,
        name: "function_return_info",
        doc: "native functions: explicit annotation, return-type name clauses, unknown and union kinds, function-head offset",
        get: |f, n| f.with_file(promises::return_info, n),
    },
    Fact {
        shape: Fact::SCALAR.shape,
        name: "error_value",
        doc: "native values: error/any/unknown/undefined category and caught-binding provenance",
        get: |f, n| f.with_file(errors::value, n),
    },
    Fact {
        shape: Fact::SCALAR.shape,
        name: "type_specifier_groups",
        doc: "native values: allowed-type candidates per union arm, with named file/lib/package provenance",
        get: |f, n| f.with_file(specifiers::groups, n),
    },
    Fact {
        shape: Fact::SCALAR.shape,
        name: "mixed_enum_offset",
        doc: "native enums: first mixed string/number member, including merged local declarations",
        get: |f, n| f.with_file(enums::mixed, n),
    },
    Fact {
        shape: Fact::SCALAR.shape,
        name: "argument_transfers",
        doc: "native calls, constructions and tagged templates: any transfer sites, raw types and error provenance",
        get: |f, n| f.with_file(arguments::findings, n),
    },
    Fact {
        shape: Fact::SCALAR.shape,
        name: "assignment_transfers",
        doc: "native assignments, declarations, properties and spreads: any transfer sites, raw types and error provenance",
        get: |f, n| f.with_file(assignments::findings, n),
    },
    Fact {
        shape: Fact::SCALAR.shape,
        name: "return_transfer",
        doc: "native returns and arrow bodies: any carrier, raw types and error provenance",
        get: |f, n| f.with_file(transfers::returned, n),
    },
    Fact {
        shape: Fact::SCALAR.shape,
        name: "this_return_type_offset",
        doc: "native class functions: class annotation offset when only this (not the class instance type) is returned",
        get: |f, n| f.with_file(this_return::offset, n),
    },
    Fact {
        shape: Fact::SCALAR.shape,
        name: "redundant_type_constituents",
        doc: "native unions and intersections: top/bottom overrides and primitive/literal dominance",
        get: |f, n| f.with_file(redundant::findings, n),
    },
    Fact {
        shape: Fact::SCALAR.shape,
        name: "regexp_exec_offset",
        doc: "native calls: match property offset when its pattern is provably nonglobal",
        get: |f, n| f.with_file(regexp::exec, n),
    },
    Fact {
        shape: Shape::List(&Shape::Record(&[
            ("node", Shape::Scalar),
            ("group", Shape::Scalar),
            ("previous_syntax", Shape::Scalar),
            ("previous_identity", Shape::Scalar),
            ("complete", Shape::Scalar),
        ])),
        name: "type_components",
        doc: "native unions and intersections: same-kind components in source order, their immediate group, prior syntax/type equivalents and whether names resolve",
        get: |f, n| f.with_file(constituents::components, n),
    },
    Fact {
        shape: Fact::SCALAR.shape,
        name: "unnecessary_type_conversion",
        doc: "native calls and operators: primitive type, conversion idiom and report offset",
        get: |f, n| f.with_file(conversions::finding, n),
    },
    Fact {
        shape: Fact::SCALAR.shape,
        name: "non_null_assertion",
        doc: "native assertions: removes only null and undefined from an otherwise identical type",
        get: |f, n| f.with_file(assertions::non_null, n),
    },
    Fact {
        shape: Fact::SCALAR.shape,
        name: "reduce_type_assertion",
        doc: "native assertions: array reduce initialiser safely accepts a type parameter",
        get: |f, n| f.with_file(assertions::reduce, n),
    },
    Fact {
        shape: Fact::SCALAR.shape,
        name: "accessor_type_mismatch",
        doc: "native getter return annotations: incompatible with the paired setter parameter",
        get: |f, n| f.with_file(accessors::mismatch, n),
    },
    Fact {
        shape: Fact::SCALAR.shape,
        name: "nonasync_for_of",
        doc: "native for-of: awaited loop lacks an async iterator",
        get: |f, n| f.with_file(awaitability::for_of, n),
    },
    Fact {
        shape: Fact::SCALAR.shape,
        name: "nonasync_using",
        doc: "native initialisers: awaited using lacks an async dispose symbol",
        get: |f, n| f.with_file(awaitability::using, n),
    },
    Fact {
        shape: Fact::SCALAR.shape,
        name: "nonawaitable_type",
        doc: "native expressions: constrained type has no possibly awaitable union arm",
        get: |f, n| f.with_file(awaitability::nonawaitable, n),
    },
    Fact {
        shape: Fact::SCALAR.shape,
        name: "nonawaitable_iterable",
        doc: "native expressions: an iterable has a definitely nonawaitable element type",
        get: |f, n| f.with_file(awaitability::iterable, n),
    },
    Fact {
        shape: Fact::SCALAR.shape,
        name: "unsafe_type",
        doc: "native expressions: any or unresolved annotation provenance, distinct from unknown",
        get: |f, n| f.with_file(unsafe_types::kind, n),
    },
    Fact {
        shape: Fact::SCALAR.shape,
        name: "unsafe_call_type",
        doc: "native call targets: any, unresolved annotation or unspecialised Function",
        get: |f, n| f.with_file(unsafe_types::call, n),
    },
    Fact {
        shape: Fact::SCALAR.shape,
        name: "unsafe_this_type",
        doc: "native expressions: unsafe category of the root this expression",
        get: |f, n| f.with_file(unsafe_types::this_kind, n),
    },
    Fact {
        shape: Fact::SCALAR.shape,
        name: "unsafe_member_key",
        doc: "native computed keys: unsafe type, unparenthesised text and report offset",
        get: |f, n| f.with_file(unsafe_types::key, n),
    },
    Fact {
        shape: Fact::SCALAR.shape,
        name: "template_type_checks",
        doc: "native expressions: interpolation requirements, every clause needs an enabled option or allowed named type",
        get: |f, n| f.with_file(template::checks, n),
    },
    Fact {
        shape: Fact::SCALAR.shape,
        name: "template_type_display",
        doc: "native expressions: the constrained interpolation type for diagnostic messages",
        get: |f, n| f.with_file(template::display, n),
    },
    Fact {
        shape: Fact::SCALAR.shape,
        name: "array_type",
        doc: "native layers: every union arm (or an intersection arm) is an array or tuple",
        get: |f, n| f.with_file(array_type, n),
    },
    Fact {
        shape: Fact::SCALAR.shape,
        name: "string_array_type",
        doc: "native layers: an array or tuple whose every element is a string",
        get: |f, n| f.with_file(string_array_type, n),
    },
    Fact {
        shape: Fact::SCALAR.shape,
        name: "void_argument_kind",
        doc: "on a void unary expression: the effective argument is a call, assignment or other expression",
        get: |f, n| f.with_file(void_argument_kind, n),
    },
];

fn is_array(t: &Ty, ck: &Checker<'_>) -> bool {
    match t {
        Ty::Array(_) | Ty::Tuple(_) => true,
        Ty::Named(sym, _) => {
            ck.is_builtin_symbol(sym, "Array") || ck.is_builtin_symbol(sym, "ReadonlyArray")
        }
        Ty::Union(parts) => !parts.is_empty() && parts.iter().all(|t| is_array(t, ck)),
        Ty::Inter(parts) => parts.iter().any(|t| is_array(t, ck)),
        _ => false,
    }
}

fn string_only(t: &Ty) -> bool {
    match t {
        Ty::Prim("string") | Ty::Lit(_, "string") => true,
        Ty::Union(parts) => !parts.is_empty() && parts.iter().all(string_only),
        _ => false,
    }
}

fn is_string_array(t: &Ty, ck: &Checker<'_>) -> bool {
    match t {
        Ty::Array(element) => string_only(&element.element),
        Ty::Tuple(elements) => elements.iter().all(string_only),
        Ty::Named(sym, args) if ck.is_builtin_symbol(sym, "Array") || ck.is_builtin_symbol(sym, "ReadonlyArray") => {
            args.first().is_some_and(string_only)
        }
        _ => false,
    }
}

fn array_type(file: &NativeFile<'_>, node: u32) -> Value<'static> {
    Value::Bool(is_array(&file.constrained_type_of(node), &file.typing().checker))
}

fn string_array_type(file: &NativeFile<'_>, node: u32) -> Value<'static> {
    Value::Bool(is_string_array(&file.constrained_type_of(node), &file.typing().checker))
}

fn void_argument_kind(file: &NativeFile<'_>, node: u32) -> Value<'static> {
    let s = &file.syntax;
    if !s.is(node, Kind::Unary) || s.word(node, Field::Op) != Some("void") {
        return Value::Null;
    }
    let Some(mut expr) = s.get(node, Field::Arg) else {
        return Value::Null;
    };
    loop {
        match s.kind(expr) {
            Some(Kind::Paren | Kind::As | Kind::Satisfies | Kind::NonNull | Kind::TypeAssertion) => {
                let Some(inner) = s.get(expr, Field::Expr) else { break };
                expr = inner;
            }
            Some(Kind::Sequence) => {
                let Some(last) = s.all(expr, Field::Expr).last() else { break };
                expr = last;
            }
            _ => break,
        }
    }
    Value::from(match s.kind(expr) {
        Some(Kind::Call | Kind::New) => "call",
        Some(Kind::Assign) => "assignment",
        _ => "other",
    })
}
