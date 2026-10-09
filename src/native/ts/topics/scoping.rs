//! Declaration, scope and return analyses (`std/ts/scoping`).

mod bindings;
mod atomic;
mod callbacks;
mod constness;
mod globals;
mod loops;
mod names;
mod receivers;
mod returns;

use crate::expr::facts::Fact;
use crate::expr::Value;
use crate::native::ts::directives;

pub const FACTS: &[Fact] = &[
    Fact { shape: Fact::SCALAR.shape, name: "ecma_version", doc: "selected ECMAScript edition: 3, 5 or a release year", get: |f, n| f.with_file(|file, _| Value::from(file.environment.ecma_version as usize), n) },
    Fact { shape: Fact::SCALAR.shape, name: "source_environment", doc: "source environment inputs: implied strictness and a root function wrapper", get: |f, n| f.with_file(|file, _| Value::record(vec![
        ("implied_strict".into(), Value::Bool(file.environment.implied_strict)),
        ("root_function".into(), Value::Bool(file.root_function_scope())),
    ]), n) },
    Fact { shape: Fact::SCALAR.shape, name: "global_origin", doc: "global origin of a name's spelling: configuration, comment, commonjs, builtin or null", get: |f, n| f.with_file(|file, node|
        file.globals().origin(file.syntax.text(node)).map_or(Value::Null, Value::from), n) },
    Fact { shape: Fact::SCALAR.shape, name: "directive_statement", doc: "expression statement belongs to a string directive prologue", get: |f, n| f.with_file(directives::statement, n) },
    Fact { shape: Fact::SCALAR.shape, name: "directive_scope", doc: "on a program or function: exact use-strict directives, inherited strictness, enclosing functions/classes and parameter simplicity", get: |f, n| f.with_file(directives::scope, n) },
    Fact { shape: Fact::SCALAR.shape, name: "scoping_const_binding", doc: "on a let declaration: unique-writer position, early reads and destructuring group eligibility", get: |f, n| f.with_file(constness::binding, n) },
    Fact { shape: Fact::SCALAR.shape, name: "scoping_atomic_update", doc: "on an assignment: shared bindings written using a pre-suspension read", get: |f, n| f.with_file(atomic::updates, n) },
    Fact { shape: Fact::SCALAR.shape, name: "scoping_this_context", doc: "on this: validity of its lexical receiver, with capitalised constructor exception", get: |f, n| f.with_file(receivers::context, n) },
    Fact { shape: Fact::SCALAR.shape, name: "scoping_loop_captures", doc: "on a function: ordered unique mutable binding names whose captured values can vary across loop iterations", get: |f, n| f.with_file(loops::captures, n) },
    Fact { shape: Fact::SCALAR.shape, name: "scoping_unmodified_condition", doc: "on a condition reference: unmodified with grouped or independent conditional branches", get: |f, n| f.with_file(loops::unmodified_condition, n) },
    Fact { shape: Fact::SCALAR.shape, name: "scoping_array_callback", doc: "on a function: array callback method, returns and fallthrough", get: |f, n| f.with_file(callbacks::array_callback, n) },
    Fact { shape: Fact::SCALAR.shape, name: "param_property_write", doc: "on a parameter reference: a write to a property reached through that parameter", get: |f, n| f.with_file(bindings::param_property_write, n) },
    Fact { shape: Fact::SCALAR.shape, name: "scoping_receiver_use", doc: "on a class method or field: its own body uses this or super", get: |f, n| f.with_file(bindings::scoping_receiver_use, n) },
    Fact { shape: Fact::SCALAR.shape, name: "scoping_global_declarations", doc: "script-global bindings: kind, name, exported-comment flag, nullable global setting and declarator offset", get: |f, n| f.with_file(globals::declarations, n) },
    Fact { shape: Fact::SCALAR.shape, name: "scoping_definition_use", doc: "on a reference: declaration kind, early use and execution-context separation", get: |f, n| f.with_file(bindings::definition_use, n) },
    Fact { shape: Fact::SCALAR.shape, name: "scoping_shadow", doc: "on a declaration: outer declaration and shadowing exceptions", get: |f, n| f.with_file(bindings::shadow, n) },
    Fact { shape: Fact::SCALAR.shape, name: "scoping_redeclaration", doc: "on a declaration: repeated syntax, safe typescript merge and built-in collision", get: |f, n| f.with_file(bindings::redeclaration, n) },
    Fact { shape: Fact::SCALAR.shape, name: "scoping_outside_bindings", doc: "on a var reference: block declarations outside whose context it is used", get: |f, n| f.with_file(bindings::outside_bindings, n) },
    Fact { shape: Fact::SCALAR.shape, name: "scoping_strict_code", doc: "native code has module, class, implied or directive strictness", get: |f, n| f.with_file(|file, n| Value::Bool(directives::strict_code(file, n)), n) },
    Fact { shape: Fact::SCALAR.shape, name: "scoping_consistent_returns", doc: "on a function: its ordered return kinds, fallthrough and report positions", get: |f, n| f.with_file(returns::consistent_returns, n) },
    Fact { shape: Fact::SCALAR.shape, name: "scoping_global_redeclarations", doc: "on the program: explicit-global mentions colliding with declarations or other comments", get: |f, n| f.with_file(globals::redeclarations, n) },
    Fact { shape: Fact::SCALAR.shape, name: "scoping_camelcase", doc: "on a name: local, original, property, import and configured-global roles", get: |f, n| f.with_file(names::camelcase, n) },
    Fact { shape: Fact::SCALAR.shape, name: "scoping_function_context", doc: "on a function expression: inferred name, generator and recursive-name context", get: |f, n| f.with_file(names::function_context, n) },
];
