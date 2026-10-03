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

pub const FACTS: &[Fact] = &[
    Fact { name: "catch_rethrows", doc: "native layers: catch starts by rethrowing its identifier parameter", get: |f, n| f.with_file(statements::rethrow, n) },
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
