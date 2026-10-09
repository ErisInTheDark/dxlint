//! eslint core suggestion rules (`std/ts/suggestions`, `std/js/problems`).

use crate::expr::facts::Fact;

pub(crate) mod expressions;
mod accessors;
mod objects;
mod coercion;
mod functions;
mod statements;
mod regex;
mod errors;
mod references;

pub const FACTS: &[Fact] = &[
    Fact { name: "reference_key", doc: "native layers: collision-safe comparable reference key, normalising static property access; null for unsupported expressions", get: |f, n| f.with_file(|file, node| references::key(file, node, false), n) },
    Fact { name: "reference_key_strict", doc: "native layers: comparable reference key preserving member/index syntax and literal key types; null for unsupported expressions", get: |f, n| f.with_file(|file, node| references::key(file, node, true), n) },
    Fact { name: "own_property_call", doc: "native layers: builtin Object/prototype/empty-object hasOwnProperty.call can use the visible global Object.hasOwn", get: |f, n| f.with_file(objects::own_property_call, n) },
    Fact { name: "label_binding", doc: "native layers: labelled statement name collides with a visible value binding or configured global", get: |f, n| f.with_file(statements::label_binding, n) },
    Fact { name: "single_iteration_loop", doc: "native layers: reachable loop body has no normal or correctly targeted continue path for another iteration", get: |f, n| f.with_file(statements::single_iteration, n) },
    Fact { name: "useless_escapes", doc: "native layers: unnecessary string, untagged template and regex identity escapes with source offsets and option-filterable characters", get: |f, n| f.with_file(regex::useless_escapes, n) },
    Fact { name: "unnamed_regex_captures", doc: "native layers: unnamed capturing group source spellings in valid literal or statically known RegExp patterns", get: |f, n| f.with_file(regex::unnamed_captures, n) },
    Fact { name: "restricted_binding", doc: "native layers: declaration shadows a restricted global name, including merged declarations and unsafe undefined bindings", get: |f, n| f.with_file(functions::restricted_binding, n) },
    Fact { name: "nonoctal_decimal_escapes", doc: "native layers: unescaped decimal escape spellings and source offsets in a string literal", get: |f, n| f.with_file(expressions::decimal_escapes, n) },
    Fact { name: "catch_throw", doc: "thrown call/new inside a catch: callee name and global status, catch binding, argument/property syntax, value bindings and source offsets", get: |f, n| f.with_file(errors::catch_throw, n) },
    Fact { name: "curly_body", doc: "native layers: control body name, brace status and all multi/line/nest brace expectations with chain consistency", get: |f, n| f.with_file(statements::curly_body, n) },
    Fact { name: "unicode_regex_flags", doc: "native layers: known literal or global RegExp flags, including missing flags as empty, excluding spread patterns", get: |f, n| f.with_file(regex::unicode_flags, n) },
    Fact { name: "eval_usage", doc: "native layers: direct or indirect eval reference, optional-call status and identifier/property report offset", get: |f, n| f.with_file(expressions::eval_usage, n) },
    Fact { name: "extended_native", doc: "native layers: global builtin prototype being extended by assignment or Object property definition, or null", get: |f, n| f.with_file(expressions::extended_native, n) },
    Fact { name: "function_style", doc: "native layers: function declaration/variable form, export policy, overload and arrow exceptions, and report offset", get: |f, n| f.with_file(functions::style, n) },
    Fact { name: "implicit_coercion", doc: "native layers: shorthand conversion kind, operator, and explicit recommendation", get: |f, n| f.with_file(coercion::coercion, n) },
    Fact { name: "redundant_boolean_cast", doc: "native layers: cast kind and minimum context policy (0 direct, 1 logical, 2 inner)", get: |f, n| f.with_file(coercion::boolean_cast, n) },
    Fact { name: "global_callee", doc: "native layers: an unshadowed, enabled global identifier called by this call or constructor, through parentheses", get: |f, n| f.with_file(expressions::global_callee, n) },
    Fact { name: "global_method_callee", doc: "native layers: global object and static method called, through parentheses and unwritten aliases", get: |f, n| f.with_file(expressions::global_method_callee, n) },
    Fact { name: "could_be_error", doc: "native expressions: syntax permits an Error-valued result through possible value branches", get: |f, n| f.with_file(|file, node| crate::expr::Value::Bool(expressions::could_be_error(file, node)), n) },
    Fact { name: "accessor_pair", doc: "native layers: accessor kind, scope, display name, pairing and unique pair grouping", get: |f, n| f.with_file(accessors::pair, n) },
    Fact { name: "property_descriptor", doc: "native layers: getter/setter presence in a global Object or Reflect property descriptor", get: |f, n| f.with_file(accessors::descriptor, n) },
    Fact { name: "object_shorthand_member", doc: "native layers: object member shorthand eligibility, constructor and annotation exceptions, and arrow lexical safety", get: |f, n| f.with_file(objects::shorthand, n) },
    Fact { name: "object_shorthand_consistency", doc: "native layers: mixed shorthand forms or all-redundant long forms in an object literal", get: |f, n| f.with_file(objects::consistency, n) },
];
