//! Declaration, naming and return analyses (`std/ts/scoping`).

mod bindings;
mod atomic;
mod callbacks;
mod conventions;
mod constness;
mod globals;
mod loops;
mod names;
mod receivers;
mod returns;

use crate::expr::facts::Fact;
use crate::expr::Value;

pub const FACTS: &[Fact] = &[
    Fact { name: "scoping_const_binding", doc: "on a let declaration: unique-writer position, early reads and destructuring group eligibility", get: |f, n| f.with_file(constness::binding, n) },
    Fact { name: "scoping_naming_candidate", doc: "on a declaration or member key: naming selector and semantic modifiers", get: |f, n| f.with_file(conventions::candidate, n) },
    Fact { name: "scoping_naming_types", doc: "on a naming candidate: primitive, callable and array types after removing nullish parts", get: |f, n| f.with_file(conventions::types, n) },
    Fact { name: "scoping_atomic_update", doc: "on an assignment: shared bindings written using a pre-suspension read", get: |f, n| f.with_file(atomic::updates, n) },
    Fact { name: "scoping_this_context", doc: "on this: validity of its lexical receiver, with capitalised constructor exception", get: |f, n| f.with_file(receivers::context, n) },
    Fact { name: "scoping_loop_captures", doc: "on a function: unsafe mutable bindings captured across loop iterations", get: |f, n| f.with_file(loops::captures, n) },
    Fact { name: "scoping_unmodified_condition", doc: "on a condition reference: unmodified with grouped or independent conditional branches", get: |f, n| f.with_file(loops::unmodified_condition, n) },
    Fact { name: "scoping_array_callback", doc: "on a function: array callback method, returns and fallthrough", get: |f, n| f.with_file(callbacks::array_callback, n) },
    Fact { name: "param_property_write", doc: "on a parameter reference: a write to a property reached through that parameter", get: |f, n| f.with_file(bindings::param_property_write, n) },
    Fact { name: "scoping_receiver_use", doc: "on a class method or field: its own body uses this or super", get: |f, n| f.with_file(bindings::scoping_receiver_use, n) },
    Fact { name: "scoping_global_declarations", doc: "script-global declarations in a variable, function or class statement", get: |f, n| f.with_file(globals::declarations, n) },
    Fact { name: "scoping_definition_use", doc: "on a reference: declaration kind, early use and execution-context separation", get: |f, n| f.with_file(bindings::definition_use, n) },
    Fact { name: "scoping_shadow", doc: "on a declaration: outer declaration and shadowing exceptions", get: |f, n| f.with_file(bindings::shadow, n) },
    Fact { name: "scoping_redeclaration", doc: "on a declaration: repeated syntax, safe typescript merge and built-in collision", get: |f, n| f.with_file(bindings::redeclaration, n) },
    Fact { name: "scoping_outside_bindings", doc: "on a var reference: block declarations outside whose context it is used", get: |f, n| f.with_file(bindings::outside_bindings, n) },
    Fact { name: "scoping_underscore_names", doc: "on declaration, function or member: names and context for underscore policy", get: |f, n| f.with_file(bindings::underscore_names, n) },
    Fact { name: "scoping_strict_code", doc: "native code is in a module, class or use-strict scope", get: |f, n| f.with_file(|file, n| Value::Bool(bindings::strict_code(file, n)), n) },
    Fact { name: "scoping_consistent_returns", doc: "on a function: its ordered return kinds, fallthrough and report positions", get: |f, n| f.with_file(returns::consistent_returns, n) },
    Fact { name: "scoping_global_redeclarations", doc: "on the program: explicit-global mentions colliding with declarations or other comments", get: |f, n| f.with_file(globals::redeclarations, n) },
    Fact { name: "scoping_camelcase", doc: "on a name: local, original, property, import and configured-global roles", get: |f, n| f.with_file(names::camelcase, n) },
    Fact { name: "scoping_function_context", doc: "on a function expression: inferred name, generator and recursive-name context", get: |f, n| f.with_file(names::function_context, n) },
];
