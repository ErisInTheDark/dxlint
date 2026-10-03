//! eslint core suggestion rules (`std/ts/suggestions`, `std/js/problems`).

use crate::expr::facts::Fact;
use crate::expr::Value;
use super::super::facts::NativeFile;
use super::super::kinds::{Field as F, Kind as K};

mod expressions;
mod accessors;
mod objects;
mod coercion;
mod functions;
mod statements;
mod regex;
mod errors;
mod strict;

pub const FACTS: &[Fact] = &[
    Fact { name: "redundant_conditional", doc: "native layers: boolean-literal or identical-identifier default conditional category through parentheses", get: |f, n| f.with_file(expressions::redundant_conditional, n) },
    Fact { name: "regex_literal_preference", doc: "native layers: static-string or redundant literal RegExp construction diagnostic and wrapping policy", get: |f, n| f.with_file(regex::literal_preference, n) },
    Fact { name: "strict_directive_policy", doc: "native layers: explicit strict directive findings for global, function, never and module policies", get: |f, n| f.with_file(strict::policy, n) },
    Fact { name: "restricted_binding", doc: "native layers: declaration shadows a restricted global name, including merged declarations and unsafe undefined bindings", get: |f, n| f.with_file(functions::restricted_binding, n) },
    Fact { name: "unnecessary_bind", doc: "native layers: property offset for bind of an arrow or function not using its own this, excluding spread arguments", get: |f, n| f.with_file(functions::unnecessary_bind, n) },
    Fact { name: "nonoctal_decimal_escapes", doc: "native layers: unescaped decimal escape spellings and source offsets in a string literal", get: |f, n| f.with_file(expressions::decimal_escapes, n) },
    Fact { name: "mixed_string_concatenation", doc: "native layers: outermost addition chain mixes string literals and non-string expressions through parentheses", get: |f, n| f.with_file(expressions::mixed_concat, n) },
    Fact { name: "caught_error_preservation", doc: "native layers: thrown constructor, nearest catch parameter status and cause diagnostics by options argument position", get: |f, n| f.with_file(errors::caught_error, n) },
    Fact { name: "curly_body", doc: "native layers: control body name, brace status and all multi/line/nest brace expectations with chain consistency", get: |f, n| f.with_file(statements::curly_body, n) },
    Fact { name: "unicode_regex_flags", doc: "native layers: known literal or global RegExp flags, including missing flags as empty, excluding spread patterns", get: |f, n| f.with_file(regex::unicode_flags, n) },
    Fact { name: "eval_usage", doc: "native layers: direct or indirect eval reference, optional-call status and identifier/property report offset", get: |f, n| f.with_file(expressions::eval_usage, n) },
    Fact { name: "sequence_context", doc: "native layers: comma position and explicit-parenthesis intent outside exempt for init/update sequences", get: |f, n| f.with_file(statements::sequence_context, n) },
    Fact { name: "variadic_apply", doc: "native layers: apply with a variable argument array and unchanged function receiver", get: |f, n| f.with_file(expressions::variadic_apply, n) },
    Fact { name: "extended_native", doc: "native layers: global builtin prototype being extended by assignment or Object property definition, or null", get: |f, n| f.with_file(expressions::extended_native, n) },
    Fact { name: "multiline_string", doc: "native layers: string source spans JavaScript line terminators outside JSX literal contexts", get: |f, n| f.with_file(expressions::multiline_string, n) },
    Fact { name: "arrow_body_style", doc: "native layers: arrow block, single-return and object status, redundant-block message and body offset", get: |f, n| f.with_file(functions::arrow_body, n) },
    Fact { name: "empty_function", doc: "native layers: uncommented empty function category, display name, TypeScript exceptions and body offset", get: |f, n| f.with_file(functions::empty_function, n) },
    Fact { name: "useless_call", doc: "native layers: unnecessary call/apply with an unchanged receiver, or null", get: |f, n| f.with_file(expressions::useless_call, n) },
    Fact { name: "guarded_for_in", doc: "native layers: for-in body is empty, guarded by if, or begins with an if-continue guard", get: |f, n| f.with_file(statements::guarded_for_in, n) },
    Fact { name: "returning_assignment", doc: "native layers: assignment contained in a return or concise arrow, parenthesis exemption and report offset", get: |f, n| f.with_file(statements::returning_assignment, n) },
    Fact { name: "redundant_else", doc: "native layers: unnecessary else report offsets for direct and else-if-chain policies", get: |f, n| f.with_file(statements::redundant_else, n) },
    Fact { name: "missing_switch_default", doc: "native layers: final trailing comment text and presence for a nonempty switch missing default, or null", get: |f, n| f.with_file(statements::switch_default, n) },
    Fact { name: "unnecessary_label", doc: "native layers: redundant break/continue label name and report offset, or null", get: |f, n| f.with_file(statements::extra_label, n) },
    Fact { name: "await_in_loop", doc: "native layers: await evaluated repeatedly by a loop, excluding function and async iteration boundaries", get: |f, n| f.with_file(statements::await_loop, n) },
    Fact { name: "catch_rethrows", doc: "native layers: catch starts by rethrowing its identifier parameter", get: |f, n| f.with_file(statements::rethrow, n) },
    Fact { name: "empty_statement_block", doc: "native layers: empty non-function block or switch without comments, catch status and report offset", get: |f, n| f.with_file(statements::empty_block, n) },
    Fact { name: "lone_statement_block", doc: "native layers: redundant block diagnostic, preserving lexical bindings except an only nested child", get: |f, n| f.with_file(statements::lone_block, n) },
    Fact { name: "lonely_if", doc: "native layers: if is the only statement of an else block", get: |f, n| f.with_file(statements::lonely_if, n) },
    Fact { name: "negated_condition", doc: "native layers: negated conditional expression or if with an unconditional else", get: |f, n| f.with_file(statements::negated_condition, n) },
    Fact { name: "default_parameter_before_required", doc: "native layers: optional or defaulted parameter preceding a required parameter in the same function", get: |f, n| f.with_file(functions::default_parameter, n) },
    Fact { name: "function_style", doc: "native layers: function declaration/variable form, export policy, overload and arrow exceptions, and report offset", get: |f, n| f.with_file(functions::style, n) },
    Fact { name: "implicit_coercion", doc: "native layers: shorthand conversion kind, operator, and explicit recommendation", get: |f, n| f.with_file(coercion::coercion, n) },
    Fact { name: "redundant_boolean_cast", doc: "native layers: cast kind and minimum context policy (0 direct, 1 logical, 2 inner)", get: |f, n| f.with_file(coercion::boolean_cast, n) },
    Fact { name: "self_comparison", doc: "native layers: comparison operands have identical tokens, ignoring comments and surrounding parentheses", get: |f, n| f.with_file(self_comparison, n) },
    Fact { name: "global_callee", doc: "native layers: an unshadowed, enabled global identifier called by this call or constructor, through parentheses", get: |f, n| f.with_file(expressions::global_callee, n) },
    Fact { name: "global_method_callee", doc: "native layers: global object and static method called, through parentheses and unwritten aliases", get: |f, n| f.with_file(expressions::global_method_callee, n) },
    Fact { name: "parse_int_callee", doc: "native layers: the source spelling of an unshadowed parseInt or Number.parseInt callee, through parentheses", get: |f, n| f.with_file(expressions::parse_int_callee, n) },
    Fact { name: "radix_error", doc: "native layers: missing or invalid parseInt argument diagnostic, excluding spread argument uncertainty", get: |f, n| f.with_file(expressions::radix_error, n) },
    Fact { name: "thrown_error", doc: "native layers: diagnostic when a thrown expression cannot produce an error object or is global undefined", get: |f, n| f.with_file(expressions::thrown_error, n) },
    Fact { name: "object_assign_spread", doc: "native layers: Object.assign arguments safely expressible as an object literal or spread, excluding accessors and source prototypes", get: |f, n| f.with_file(objects::assign_spread, n) },
    Fact { name: "useless_rename", doc: "native layers: the cooked name of a redundant import, export or destructuring alias, null otherwise", get: |f, n| f.with_file(expressions::useless_rename, n) },
    Fact { name: "useless_concat", doc: "native layers: operator offset when adjacent string or template operands are concatenated on one line, null otherwise", get: |f, n| f.with_file(expressions::useless_concat, n) },
    Fact { name: "accessor_pair", doc: "native layers: accessor kind, scope, display name, pairing and unique pair grouping", get: |f, n| f.with_file(accessors::pair, n) },
    Fact { name: "property_descriptor", doc: "native layers: getter/setter presence in a global Object or Reflect property descriptor", get: |f, n| f.with_file(accessors::descriptor, n) },
    Fact { name: "object_shorthand_member", doc: "native layers: object member shorthand eligibility, constructor and annotation exceptions, and arrow lexical safety", get: |f, n| f.with_file(objects::shorthand, n) },
    Fact { name: "object_shorthand_consistency", doc: "native layers: mixed shorthand forms or all-redundant long forms in an object literal", get: |f, n| f.with_file(objects::consistency, n) },
    Fact { name: "useless_computed_key", doc: "native layers: unnecessary literal computed key, excluding prototype and constructor semantic changes", get: |f, n| f.with_file(objects::computed_key, n) },
];

fn self_comparison(file: &NativeFile, node: u32) -> Value<'static> {
    let s = &file.syntax;
    let same = s.is(node, K::Binary)
        && s.get(node, F::Left).zip(s.get(node, F::Right))
            .is_some_and(|(left, right)| file.equal_tokens(left, right));
    Value::Bool(same)
}

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
