use std::path::Path;

use super::native_linter;

#[test]
fn addition_reports_invalid_operands_and_incompatible_pairs_at_their_owner() {
    let lint = native_linter("rule restrict-plus-operands");
    for (source, spans) in [
        ("declare const x: unknown; x + 1;", vec!["x"]),
        ("declare const x: {}; x + 1;", vec!["x"]),
        ("interface RegExp {} declare const regexp: RegExp; regexp + 1;", vec!["regexp"]),
        ("declare let x: unknown; x += 1;", vec!["x"]),
        ("1n + 1;", vec!["1n + 1"]),
        ("'s' + true;", vec![]),
        ("'s' + 1;", vec![]),
        ("function f<T extends string>(x: T){ return x + 's'; }", vec![]),
        ("declare const x: Missing; x + 1;", vec![]),
    ] {
        let diagnostics = lint.lint(Path::new("/root/a.ts"), source);
        let actual: Vec<_> = diagnostics.iter().map(|d| &source[d.start..d.end]).collect();
        assert_eq!(actual, spans, "{source}: {diagnostics:#?}");
    }
}

#[test]
fn addition_options_change_only_their_operand_or_assignment_category() {
    for (settings, source, spans) in [
        ("set allowAny false", "declare const x: any; x + 1;", vec!["x"]),
        ("set allowBoolean false", "'s' + true;", vec!["true"]),
        ("set allowNullish false", "'s' + null;", vec!["null"]),
        ("set allowNullish false", "declare const x: number | undefined; 's' + x;", vec!["x"]),
        ("set allowRegExp false", "interface RegExp {} declare const regexp: RegExp; 's' + regexp;", vec!["regexp"]),
        ("set allowNumberAndString false", "'s' + 1;", vec!["'s' + 1"]),
        ("set skipCompoundAssignments true", "declare let x: unknown; x += 1;", vec![]),
        ("set skipCompoundAssignments true", "declare let x: unknown; x + 1;", vec!["x"]),
        ("set allowBoolean false", "'s' + 1;", vec![]),
        ("set allowRegExp false", "'s' + true;", vec![]),
        ("set allowAny false", "'s' + true;", vec![]),
        ("set allowNullish false", "interface RegExp {} declare const regexp: RegExp; 's' + regexp;", vec![]),
    ] {
        let lint = native_linter(&format!("rule restrict-plus-operands {{ {settings} }}"));
        let diagnostics = lint.lint(Path::new("/root/a.ts"), source);
        let actual: Vec<_> = diagnostics.iter().map(|d| &source[d.start..d.end]).collect();
        assert_eq!(actual, spans, "{settings}: {source}: {diagnostics:#?}");
    }
}

#[test]
fn addition_prioritises_individual_errors_and_preserves_compound_type_structure() {
    for (settings, source, spans) in [
        ("set allowBoolean false", "true + false;", vec!["true", "false"]),
        ("set allowBoolean false\nset allowNumberAndString false", "declare const x: boolean | number; x + 's';", vec!["x"]),
        ("", "declare const x: {} & string; x + 's';", vec![]),
        ("", "declare const x: {} | number; x + 's';", vec!["x"]),
        ("", "declare const x: {} & { a: number }; x + 's';", vec!["x"]),
    ] {
        let lint = native_linter(&format!("rule restrict-plus-operands {{ {settings} }}"));
        let diagnostics = lint.lint(Path::new("/root/a.ts"), source);
        let actual: Vec<_> = diagnostics.iter().map(|d| &source[d.start..d.end]).collect();
        assert_eq!(actual, spans, "{source}: {diagnostics:#?}");
    }
}

mod expression_fixes {
    use std::path::Path;
    use super::native_linter;
    use crate::native::ts::kinds::{Field, Kind};

    fn child(nodes: &[crate::grammar::Node], id: u32, field: Field) -> u32 {
        nodes[id as usize].children.iter().find(|edge| edge.label == field as u16).expect("fixture node has the requested field").node
    }

    fn unparen(nodes: &[crate::grammar::Node], mut id: u32) -> u32 {
        while nodes[id as usize].matcher == Kind::Paren as u32 {
            id = child(nodes, id, Field::Expr);
        }
        id
    }

    fn single_pass(rule: &str, source: &str, fixable: bool) -> String {
        let lint = native_linter(rule);
        let reports = lint.lint(Path::new("/root/fixes.js"), source);
        assert!(!reports.is_empty(), "{source}: missing diagnostic");
        assert!(reports.iter().all(|report| report.severity == crate::config::ast::Severity::Warn), "{source}: fix evaluation failed: {reports:#?}");
        assert!(reports.iter().all(|report| report.fix.is_some() == fixable), "{source}: {reports:#?}");
        let (fixed, applied) = crate::engine::apply_fixes(source, &reports);
        assert_eq!(applied > 0, fixable, "{source}");
        assert!(crate::native::ts::parse(&fixed, false).is_ok(), "{source}: {fixed}");
        if fixable {
            assert_ne!(fixed, source, "advertised fix made no change");
        } else {
            assert_eq!(fixed, source, "withheld fix changed source");
        }
        fixed
    }

    fn kind_count(source: &str, kind: Kind) -> usize {
        crate::native::ts::parse(source, false).expect("fixture parses").nodes.iter().filter(|node| node.matcher == kind as u32).count()
    }

    fn calls(source: &str) -> Vec<String> {
        let tree = crate::native::ts::parse(source, false).expect("call fixture parses");
        tree.nodes.iter().enumerate().filter(|(_, node)| node.matcher == Kind::Call as u32).map(|(id, _)| {
            let callee_id = child(&tree.nodes, id as u32, Field::Callee);
            let callee = &tree.nodes[callee_id as usize];
            let text = &source[callee.start as usize..callee.end as usize];
            if callee.matcher == Kind::Member as u32 && text != "Math.pow" {
                let property = &tree.nodes[child(&tree.nodes, callee_id, Field::Property) as usize];
                source[property.start as usize..property.end as usize].to_string()
            } else {
                text.to_string()
            }
        }).collect()
    }

    #[test]
    fn boolean_cast_fixes_keep_operand_calls_and_context_binding() {
        for source in [
            "if (!!value) consume();",
            "if (Boolean(value)) consume();",
            "if (Boolean()) consume();",
            "if (!Boolean()) consume();",
            "if (!!(left = right)) consume();",
            "if (Boolean((left, right))) consume();",
            "if (Boolean(a ? b : c)) consume();",
            "typeof!Boolean();",
            "if (Boolean(read())) consume();",
        ] {
            let fixed = single_pass("rule no-extra-boolean-cast", source, true);
            let retained: Vec<_> = calls(source).into_iter().filter(|callee| callee != "Boolean").collect();
            assert_eq!(calls(&fixed), retained, "{source}: operand calls changed");
        }
        for source in [
            "if (Boolean(...values)) consume();",
            "if (Boolean(a, b)) consume();",
            "if (Boolean(/* keep */value)) consume();",
            "if (!!/* keep */value) consume();",
        ] {
            single_pass("rule no-extra-boolean-cast", source, false);
        }
    }

    #[test]
    fn expression_replacements_keep_conditional_and_exponentiation_binding() {
        let source = "if (Boolean(a ? b : c) ? d : e) consume();";
        let fixed = single_pass("rule no-extra-boolean-cast", source, true);
        let tree = crate::native::ts::parse(&fixed, false).expect("fixed condition parses");
        let condition = tree.nodes.iter().position(|node| node.matcher == Kind::If as u32).expect("fixture retains its if statement") as u32;
        let test = unparen(&tree.nodes, child(&tree.nodes, condition, Field::Test));
        let nested = unparen(&tree.nodes, child(&tree.nodes, test, Field::Test));
        assert_eq!(tree.nodes[nested as usize].matcher, Kind::Conditional as u32, "cast removal rebound ternary arms");
        for (source, field) in [
            ("Math.pow(a ** b, c);", Field::Left),
            ("Math.pow(a, b ** c);", Field::Right),
        ] {
            let fixed = single_pass("rule prefer-exponentiation-operator", source, true);
            let tree = crate::native::ts::parse(&fixed, false).expect("fixed exponentiation parses");
            let statement = tree.nodes.iter().position(|node| node.matcher == Kind::ExprStmt as u32).expect("fixture retains its expression statement") as u32;
            let root = unparen(&tree.nodes, child(&tree.nodes, statement, Field::Expr));
            let nested = unparen(&tree.nodes, child(&tree.nodes, root, field));
            assert_eq!(tree.nodes[nested as usize].matcher, Kind::Binary as u32, "{source}: reassociated exponentiation");
            let op = &tree.nodes[child(&tree.nodes, nested, Field::Op) as usize];
            assert_eq!(&fixed[op.start as usize..op.end as usize], "**");
        }
    }

    #[test]
    fn exponentiation_fixes_keep_evaluation_order_and_statement_boundaries() {
        for source in [
            "const power = () => Math.pow({}, exponent);",
            "const power = () => Math.pow({}.value, exponent);",
            "Math.pow({}.value, exponent);",
            "Math.pow(function () {}.value, exponent);",
            "Math.pow(class {}.value, exponent);",
            "Math.pow({}.value`tag`, exponent);",
            "Math.pow(async function () {}(), exponent);",
            "Math.pow(readBase(), readExponent());",
            "Math.pow(-base, exponent);",
            "Math.pow(base ** first, last);",
            "Math.pow(base, first ** last);",
            "Math.pow(base, exponent).toString();",
            "value ** Math.pow(base, exponent);",
            "Math.pow(base, exponent) ** value;",
            "previous()\nMath.pow(base + other, exponent);",
            "if (ready) Math.pow(base + other, exponent);",
            "Math.pow(function () {}, exponent);",
            "Math.pow(base, exponent)in object;",
        ] {
            let fixed = single_pass("rule prefer-exponentiation-operator", source, true);
            let retained: Vec<_> = calls(source).into_iter().filter(|callee| callee != "Math.pow").collect();
            assert_eq!(calls(&fixed), retained, "{source}: operand evaluation changed");
            assert_eq!(kind_count(&fixed, Kind::ExprStmt), kind_count(source, Kind::ExprStmt), "{source}: ASI merged statements");
        }
        for source in [
            "Math.pow(base);",
            "Math.pow(base, exponent, extra);",
            "Math.pow(...values);",
            "Math.pow(/* keep */base, exponent);",
        ] {
            single_pass("rule prefer-exponentiation-operator", source, false);
        }
    }

    #[test]
    fn logical_assignment_fixes_cover_both_modes_without_repeating_rhs_calls() {
        for source in [
            "value = value || read();",
            "value = value && read();",
            "value = value ?? read();",
            "value = value || read() || fallback();",
            "value || (value = read());",
            "value && (value = read());",
            "value ?? (value = read());",
            "const object = {}; object.value || (object.value = read());",
            "const object = {}; object['value'] || (object['value'] = read());",
            "for (const object of objects) { object.value || (object.value = read()); }",
            "class Owner { method() { this.value || (this.value = read()); } }",
            "consume(value || (value = read()));",
        ] {
            let fixed = single_pass("rule logical-assignment-operators", source, true);
            assert_eq!(calls(&fixed), calls(source), "{source}: RHS calls changed");
        }
        for source in [
            "value ||= read();",
            "value &&= read();",
            "value ??= (left || read());",
        ] {
            let fixed = single_pass("rule logical-assignment-operators { set mode \"never\" }", source, true);
            assert_eq!(calls(&fixed), calls(source), "{source}: expansion changed calls");
        }
    }

    #[test]
    fn logical_expansion_keeps_nullish_and_boolean_groups_separate() {
        let fixed = single_pass("rule logical-assignment-operators { set mode \"never\" }", "value ??= (left || read());", true);
        let tree = crate::native::ts::parse(&fixed, false).expect("fixed assignment parses");
        let assignment = tree.nodes.iter().position(|node| node.matcher == Kind::Assign as u32).expect("fixture retains its assignment") as u32;
        let right = unparen(&tree.nodes, child(&tree.nodes, assignment, Field::Right));
        let op = &tree.nodes[child(&tree.nodes, right, Field::Op) as usize];
        assert_eq!(&fixed[op.start as usize..op.end as usize], "??", "mixed operators escaped their grouping");
    }

    #[test]
    fn logical_property_fixes_withhold_unstable_bases_and_non_literal_keys() {
        let lint = native_linter("rule logical-assignment-operators");
        let outcomes: Vec<_> = [
            "let object = { get value() { object = other; return false; } }; object.value || (object.value = read());",
            "const object = { get value() { key = 'other'; return false; } }; let key = 'value'; object[key] || (object[key] = read());",
            "let count = 0; const object = {}; const key = { [Symbol.toPrimitive]() { return count++ ? 'other' : 'value'; } }; object[key] || (object[key] = read());",
            "declare const object: { value: unknown }; object.value || (object.value = read());",
        ].into_iter().map(|source| {
            let reports = lint.lint(Path::new("/root/references.ts"), source);
            assert!(!reports.is_empty(), "{source}: missing diagnostic");
            assert!(reports.iter().all(|report| report.severity == crate::config::ast::Severity::Warn), "{source}: {reports:#?}");
            let (fixed, applied) = crate::engine::apply_fixes(source, &reports);
            (source, reports.iter().any(|report| report.fix.is_some()), applied, fixed == source)
        }).collect();
        assert!(outcomes.iter().all(|(_, advertised, applied, unchanged)| !advertised && *applied == 0 && *unchanged), "{outcomes:#?}");
    }

    #[test]
    fn logical_assignment_withholds_getter_comment_and_dynamic_scope_changes() {
        for (rule, source) in [
            ("rule logical-assignment-operators", "object.value || (object.value = read());"),
            ("rule logical-assignment-operators", "class Derived extends Base { method() { super.value || (super.value = read()); } }"),
            ("rule logical-assignment-operators", "object.value = object.value || read();"),
            ("rule logical-assignment-operators", "value = value /* keep */ || read();"),
            ("rule logical-assignment-operators", "with (object) { value = value || read(); }"),
            ("rule logical-assignment-operators", "object.deep.value || (object.deep.value = read());"),
            ("rule logical-assignment-operators { set mode \"never\" }", "object.value ||= read();"),
            ("rule logical-assignment-operators { set mode \"never\" }", "value ||= /* keep */read();"),
        ] {
            single_pass(rule, source, false);
        }
    }

    #[test]
    fn logical_if_fixes_keep_rhs_and_preserve_statement_separation() {
        let rule = "rule logical-assignment-operators { set enforceForIfStatements true }";
        for source in [
            "if (!value) value = read();",
            "if (value) { value = read() }",
            "if (value == null) { value = read(); }",
            "if (value === null || value === undefined) value = read();",
            "const object = {}; if (!object.value) object.value = read();",
        ] {
            let fixed = single_pass(rule, source, true);
            assert_eq!(calls(&fixed), calls(source), "{source}: RHS calls changed");
            assert_eq!(kind_count(&fixed, Kind::If), 0, "{source}: condition was not replaced");
        }
        for source in [
            "if (object.value === null || object.value === undefined) object.value = read();",
            "if (!value) { /* keep */ value = read(); }",
            "previous()\nif (value == null) (value) = read();",
        ] {
            single_pass(rule, source, false);
        }
    }
}
