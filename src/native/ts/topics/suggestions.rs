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
mod strict;
mod references;

pub const FACTS: &[Fact] = &[
    Fact { name: "reference_key", doc: "native layers: collision-safe comparable reference key, normalising static property access; null for unsupported expressions", get: |f, n| f.with_file(|file, node| references::key(file, node, false), n) },
    Fact { name: "reference_key_strict", doc: "native layers: comparable reference key preserving member/index syntax and literal key types; null for unsupported expressions", get: |f, n| f.with_file(|file, node| references::key(file, node, true), n) },
    Fact { name: "own_property_call", doc: "native layers: builtin Object/prototype/empty-object hasOwnProperty.call can use the visible global Object.hasOwn", get: |f, n| f.with_file(objects::own_property_call, n) },
    Fact { name: "label_binding", doc: "native layers: labelled statement name collides with a visible value binding or configured global", get: |f, n| f.with_file(statements::label_binding, n) },
    Fact { name: "single_iteration_loop", doc: "native layers: reachable loop body has no normal or correctly targeted continue path for another iteration", get: |f, n| f.with_file(statements::single_iteration, n) },
    Fact { name: "useless_escapes", doc: "native layers: unnecessary string, untagged template and regex identity escapes with source offsets and option-filterable characters", get: |f, n| f.with_file(regex::useless_escapes, n) },
    Fact { name: "unnamed_regex_captures", doc: "native layers: unnamed capturing group source spellings in valid literal or statically known RegExp patterns", get: |f, n| f.with_file(regex::unnamed_captures, n) },
    Fact { name: "regex_literal_preference", doc: "native layers: static-string or redundant literal RegExp construction diagnostic and wrapping policy", get: |f, n| f.with_file(regex::literal_preference, n) },
    Fact { name: "strict_directive_policy", doc: "native layers: explicit strict directive findings for global, function, never and module policies", get: |f, n| f.with_file(strict::policy, n) },
    Fact { name: "restricted_binding", doc: "native layers: declaration shadows a restricted global name, including merged declarations and unsafe undefined bindings", get: |f, n| f.with_file(functions::restricted_binding, n) },
    Fact { name: "nonoctal_decimal_escapes", doc: "native layers: unescaped decimal escape spellings and source offsets in a string literal", get: |f, n| f.with_file(expressions::decimal_escapes, n) },
    Fact { name: "caught_error_preservation", doc: "native layers: thrown constructor, nearest catch parameter status and cause diagnostics by options argument position", get: |f, n| f.with_file(errors::caught_error, n) },
    Fact { name: "curly_body", doc: "native layers: control body name, brace status and all multi/line/nest brace expectations with chain consistency", get: |f, n| f.with_file(statements::curly_body, n) },
    Fact { name: "unicode_regex_flags", doc: "native layers: known literal or global RegExp flags, including missing flags as empty, excluding spread patterns", get: |f, n| f.with_file(regex::unicode_flags, n) },
    Fact { name: "eval_usage", doc: "native layers: direct or indirect eval reference, optional-call status and identifier/property report offset", get: |f, n| f.with_file(expressions::eval_usage, n) },
    Fact { name: "extended_native", doc: "native layers: global builtin prototype being extended by assignment or Object property definition, or null", get: |f, n| f.with_file(expressions::extended_native, n) },
    Fact { name: "empty_statement_block", doc: "native layers: empty non-function block or switch without comments, catch status and report offset", get: |f, n| f.with_file(statements::empty_block, n) },
    Fact { name: "lone_statement_block", doc: "native layers: redundant block diagnostic, preserving lexical bindings except an only nested child", get: |f, n| f.with_file(statements::lone_block, n) },
    Fact { name: "function_style", doc: "native layers: function declaration/variable form, export policy, overload and arrow exceptions, and report offset", get: |f, n| f.with_file(functions::style, n) },
    Fact { name: "implicit_coercion", doc: "native layers: shorthand conversion kind, operator, and explicit recommendation", get: |f, n| f.with_file(coercion::coercion, n) },
    Fact { name: "redundant_boolean_cast", doc: "native layers: cast kind and minimum context policy (0 direct, 1 logical, 2 inner)", get: |f, n| f.with_file(coercion::boolean_cast, n) },
    Fact { name: "global_callee", doc: "native layers: an unshadowed, enabled global identifier called by this call or constructor, through parentheses", get: |f, n| f.with_file(expressions::global_callee, n) },
    Fact { name: "global_method_callee", doc: "native layers: global object and static method called, through parentheses and unwritten aliases", get: |f, n| f.with_file(expressions::global_method_callee, n) },
    Fact { name: "thrown_error", doc: "native layers: diagnostic when a thrown expression cannot produce an error object or is global undefined", get: |f, n| f.with_file(expressions::thrown_error, n) },
    Fact { name: "accessor_pair", doc: "native layers: accessor kind, scope, display name, pairing and unique pair grouping", get: |f, n| f.with_file(accessors::pair, n) },
    Fact { name: "property_descriptor", doc: "native layers: getter/setter presence in a global Object or Reflect property descriptor", get: |f, n| f.with_file(accessors::descriptor, n) },
    Fact { name: "object_shorthand_member", doc: "native layers: object member shorthand eligibility, constructor and annotation exceptions, and arrow lexical safety", get: |f, n| f.with_file(objects::shorthand, n) },
    Fact { name: "object_shorthand_consistency", doc: "native layers: mixed shorthand forms or all-redundant long forms in an object literal", get: |f, n| f.with_file(objects::consistency, n) },
];

#[cfg(test)]
mod tests {
    use std::path::Path;

    use crate::engine::tests::native_linter;

    #[test]
    fn self_comparison_compares_tokens_without_changing_literal_contents() {
        let lint = native_linter("rule no-self-compare");
        for (source, expected) in [
            ("'a b' === 'ab';", 0),
            ("foo /* comment */.bar === foo.bar;", 1),
            ("(foo) === foo;", 1),
        ] {
            assert_eq!(lint.lint(Path::new("/root/a.ts"), source).len(), expected, "{source}");
        }
    }

    #[test]
    fn redundant_renames_follow_binding_defaults_and_cooked_names() {
        let lint = native_linter("rule no-useless-rename:destructuring");
        assert_eq!(lint.lint(Path::new("/root/a.ts"), "let {'foo': foo = 1, bar: baz} = source;").len(), 1);
        assert_eq!(lint.lint(Path::new("/root/a.ts"), "const value = {foo: foo};").len(), 0);
    }

    #[test]
    fn concatenation_checks_adjacent_string_operands() {
        let lint = native_linter("rule no-useless-concat");
        assert_eq!(lint.lint(Path::new("/root/a.ts"), "foo + 'a' + 'b';").len(), 1);
        assert_eq!(lint.lint(Path::new("/root/a.ts"), "'a' + 1 + 'b';").len(), 0);
    }
}
