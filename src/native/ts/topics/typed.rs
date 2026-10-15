//! typescript-eslint's type-aware rules (`std/ts/typed`).

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
mod constituents;
mod redundant;
mod transfers;
mod arguments;
mod enums;
pub(super) mod specifiers;
mod errors;
mod promises;
mod empty_types;
mod defaults;

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
        name: "callable",
        doc: "native expressions: the type supplies call signatures (every union arm, or an intersection arm)",
        get: |f, n| f.with_file(|file, node| Value::Bool(file.typing().checker.callable_type(&file.type_of(node))), n),
    },
    Fact {
        shape: Fact::SCALAR.shape,
        name: "static_string",
        doc: "native expressions: constant evaluated text, including templates, concatenation and unmodified variables; null when unknown",
        get: |f, n| f.with_file(|file, node| file.static_string(node).map_or(Value::Null, Value::from), n),
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
        name: "return_transfer",
        doc: "native returns and arrow bodies: any carrier, raw types and error provenance",
        get: |f, n| f.with_file(transfers::returned, n),
    },
    Fact {
        shape: Fact::SCALAR.shape,
        name: "redundant_type_constituents",
        doc: "native unions and intersections: top/bottom overrides and primitive/literal dominance",
        get: |f, n| f.with_file(redundant::findings, n),
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
