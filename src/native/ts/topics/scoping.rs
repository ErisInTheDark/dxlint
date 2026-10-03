//! Declaration, naming and return analyses (`std/ts/scoping`).

mod bindings;
mod globals;
mod names;
mod returns;

use crate::expr::facts::Fact;
use crate::expr::Value;

pub const FACTS: &[Fact] = &[
    Fact { name: "param_property_write", doc: "on a parameter reference: a write to a property reached through that parameter", get: |f, n| f.with_file(bindings::param_property_write, n) },
    Fact { name: "scoping_receiver_use", doc: "on a class method or field: its own body uses this or super", get: |f, n| f.with_file(bindings::scoping_receiver_use, n) },
    Fact { name: "scoping_head_offset", doc: "on a call target: its reported name start; on an arrow: its operator start", get: |f, n| f.with_file(bindings::scoping_head_offset, n) },
    Fact { name: "scoping_date_utc", doc: "a call target is the global Date.UTC", get: |f, n| f.with_file(bindings::scoping_date_utc, n) },
    Fact { name: "scoping_global_declarations", doc: "script-global declarations in a variable, function or class statement", get: |f, n| f.with_file(globals::declarations, n) },
    Fact { name: "scoping_definition_use", doc: "on a reference: declaration kind, early use and execution-context separation", get: |f, n| f.with_file(bindings::definition_use, n) },
    Fact { name: "scoping_shadow", doc: "on a declaration: outer declaration and shadowing exceptions", get: |f, n| f.with_file(bindings::shadow, n) },
    Fact { name: "scoping_this_alias", doc: "on a declaration or assignment: alias name and whether it captures this", get: |f, n| f.with_file(bindings::this_alias, n) },
    Fact { name: "scoping_redeclaration", doc: "on a declaration: repeated syntax, safe typescript merge and built-in collision", get: |f, n| f.with_file(bindings::redeclaration, n) },
    Fact { name: "scoping_declaration_position", doc: "declaration nesting, nearest permitted body and strict-code status", get: |f, n| f.with_file(bindings::declaration_position, n) },
    Fact { name: "scoping_outside_bindings", doc: "on a var reference: block declarations outside whose context it is used", get: |f, n| f.with_file(bindings::outside_bindings, n) },
    Fact { name: "scoping_executor_return", doc: "on a return or concise arrow: promise-executor result offset and void status", get: |f, n| f.with_file(returns::executor_return, n) },
    Fact { name: "scoping_underscore_names", doc: "on declaration, function or member: names and context for underscore policy", get: |f, n| f.with_file(bindings::underscore_names, n) },
    Fact { name: "scoping_strict_code", doc: "native code is in a module, class or use-strict scope", get: |f, n| f.with_file(|file, n| Value::Bool(bindings::strict_code(file, n)), n) },
    Fact { name: "scoping_consistent_returns", doc: "on a function: its ordered return kinds, fallthrough and report positions", get: |f, n| f.with_file(returns::consistent_returns, n) },
    Fact { name: "scoping_useless_return", doc: "on a bare return: reachable and redundant at its function's end", get: |f, n| f.with_file(returns::useless_return, n) },
    Fact { name: "scoping_global_redeclarations", doc: "on the program: explicit-global mentions colliding with declarations or other comments", get: |f, n| f.with_file(globals::redeclarations, n) },
    Fact { name: "scoping_camelcase", doc: "on a name: local, original, property, import and configured-global roles", get: |f, n| f.with_file(names::camelcase, n) },
    Fact { name: "scoping_function_context", doc: "on a function expression: inferred name, generator and recursive-name context", get: |f, n| f.with_file(names::function_context, n) },
    Fact { name: "scoping_function_match", doc: "on a named function expression: its assigned name and property descriptor context", get: |f, n| f.with_file(names::function_match, n) },
];
