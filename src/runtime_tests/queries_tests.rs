//! Type-query tests: trait, member, module-class, name and syntax-tree queries.

use crate::engine::tests::{errors, lint, linter};

#[test]
fn trait_queries_quantify_unions_and_intersections_and_combine_trait_masks() {
    let linter = linter(r#"
        match good /a/ { fits good }
        match other /b/ { fits other }
        in .t {
            match either left:good /\|/ right:other { unwraps left | right }
            match both left:good /&/ right:other { unwraps left & right }
        }
        in either, both {
            rule possible warn "possible" {
                let x node
                when x could good
            }
            rule certain warn "certain" { when node always good }
            rule masked warn "masked" {
                when node
                    always (good, other)
            }
            rule nonnode warn "nonnode" { when (text could good) == null }
        }
    "#);
    let reports = lint(&linter, "a|b a&b");
    let count = |rule| reports.iter().filter(|report| report.rule == rule).count();
    assert_eq!(count("possible"), 2);
    assert_eq!(count("certain"), 1);
    assert_eq!(count("masked"), 1);
    assert_eq!(count("nonnode"), 2);
}

#[test]
fn unknown_trait_answers_are_false_for_rules() {
    let linter = linter(r#"
        in str fits good
        in paren {
            unwraps it
            rule unknown warn "unknown" {
                when node could good || node always good
            }
        }
    "#);
    assert!(lint(&linter, "()").is_empty());
}

#[test]
fn unknown_trait_names_report_the_trait_and_suggest_a_known_one() {
    let errors = errors(r#"
        in paren fits callable
        in paren rule typo warn "typo" { when node could callabl }
    "#);
    assert!(errors.iter().any(|error| error.contains("nothing fits trait") && error.contains("callabl") && error.contains("callable")), "{errors:#?}");
}

#[test]
fn semantic_result_subjects_use_declared_outputs_without_exposing_nodes() {
    let linter = linter(r#"
        match good /a/ { fits good }
        match other /b/ { fits other }
        in .t {
            match producer "f" parameter:"()"? output:(good or other) {
                signature parameter returns output
            }
        }
        in producer {
            rule returned warn "returned" { when node.result always good }
            rule nonnode warn "nonnode" { when (text.result could good) == null }
        }
    "#);
    let reports = lint(&linter, "fa fb");
    assert_eq!(reports.iter().filter(|report| report.rule == "returned").count(), 1);
    assert_eq!(reports.iter().filter(|report| report.rule == "nonnode").count(), 2);
    assert!(!errors(r#"
        in paren rule escaped warn node.result
    "#).is_empty());
    assert!(!errors(r#"
        in paren fits good
        in paren rule escaped warn "escaped" { when node.result.node could good }
    "#).is_empty());
}

#[test]
fn member_subjects_query_declared_values_without_proving_absent_members() {
    let linter = linter(r#"
        match good /a/ { fits good }
        match other /b/ { fits other }
        in .t {
            match container "[" content:(good or other) "]" {
                scope properties
                members properties
                member properties "key" unwraps content
            }
        }
        in container {
            rule member warn "member" { when node.member "key" always good }
            rule absent warn "absent" {
                when node.member "missing" could good || node.member "missing" always good
            }
        }
    "#);
    let reports = lint(&linter, "[a] [b]");
    assert_eq!(reports.iter().filter(|report| report.rule == "member").count(), 1);
    assert_eq!(reports.iter().filter(|report| report.rule == "absent").count(), 0);
}

#[test]
fn class_filters_keep_selected_answers_and_reject_invalid_requests() {
    let linter = linter(r#"
        match value_name /x/
        match type_name /y/
        in .t match document /[\s\S]+/ { scope outer }
        in document match declaration name:value_name /;/ {
            declares value name in nearest outer hoisted
        }
        in document match type_declaration name:type_name /;/ {
            declares type name in nearest outer hoisted
        }
        in document match reference name:value_name /!/ {
            resolves value name
            let selected name_classes(name.node, "x", null, ["value", "value"])
            let all_classes name_classes(name.node, "x", null, null)
            let empty name_classes(name.node, "x", null, [])
            rule selected {
                when selected.value == true && count(keys(selected)) == 1
                warn "selected"
            }
            rule all_classes {
                when all_classes.value == true && count(keys(all_classes)) == 2
                warn "all classes"
            }
            rule empty {
                when count(keys(empty)) == 0
                warn "empty"
            }
            rule invalid_class {
                when name_classes(name.node, "x", null, ["missing"]) == null
                warn "invalid class"
            }
            rule invalid_filter {
                when name_classes(name.node, "x", null, 1) == null
                warn "invalid filter"
            }
        }
    "#);
    let reports = lint(&linter, "x;y;x!");
    assert_eq!(reports.len(), 5, "{reports:#?}");
}

#[test]
fn scope_queries_keep_dual_roles_logical_scope_ids_and_indexed_merged_sites() {
    let linter = linter(r#"
        in .t match document /[\s\S]+/ { scope outer; scope inner }
        in document match declaration name:/x/ /;/ {
            declares value name in nearest outer hoisted
        }
        in document match dual name:/x/ /\?/ {
            scope members
            declares member name in nearest outer
            resolves value name
            let decl_scope { value name.node; name_scopes "declaration"; first }
            let referenced name_scopes(name.node, "reference")
            let member_scope first(referenced)
            let inner { value referenced; slice 1; first }
            let outer last(referenced)
            let sites {
                value name_scope_declarations(name.node, outer.id, "x")
                filter (site) contains(site.classes, "value")
            }
            let declared_classes name_role_classes(name.node, "declaration")
            let asked_classes name_role_classes(name.node, "reference")
            rule observed warn "observed"
            rule invalid warn "invalid" {
                when member_scope.owner != node || inner.owner != outer.owner || inner.id == outer.id
                    || member_scope.parent != inner.id || inner.parent != outer.id || outer.parent != null
                    || decl_scope.id != outer.id || count(sites) != 2
                    || first(sites).node.offset >= last(sites).node.offset
                    || !all(sites, site => site.hoisted && contains(site.classes, "value"))
                    || !contains(declared_classes, "member") || contains(declared_classes, "value")
                    || !contains(asked_classes, "value") || contains(asked_classes, "member")
                    || outer.owner.text != source
                    || name_scope_declarations(name.node, "not a scope", "x") != null
                    || name_scope_declarations(name.node, outer.id, "missing") != []
            }
        }
    "#);
    let reports = lint(&linter, "x;x;x?");
    assert_eq!(reports.len(), 1, "{reports:#?}");
    assert_eq!(reports[0].rule, "observed");
}

#[test]
fn binding_queries_keep_merged_sites_and_ordered_references() {
    let linter = linter(r#"
        in .t match document /[\s\S]+/ { scope outer }
        in document match declaration name:/x/ /;/ {
            declares value name in nearest outer hoisted
            let binding name_binding(name.node)
            let uses name_references(name.node)
            rule observed warn "observed"
            rule invalid-declaration warn "invalid" {
                when binding.node != first(binding.declarations) || binding.key != "x"
                    || count(binding.declarations) != 2 || count(uses) != 2
                    || first(binding.declarations).offset >= last(binding.declarations).offset
                    || first(uses).offset >= last(uses).offset
                    || binding.node.text != "x" || binding.scope.owner.text != source
            }
        }
        in document match reference name:/x/ /!/ {
            resolves value name
            let binding name_binding(name.node)
            rule invalid-reference warn "invalid" {
                when binding.node != first(binding.declarations) || count(binding.declarations) != 2
                    || count(name_references(name.node)) != 2
            }
        }
    "#);
    let reports = lint(&linter, "x;x;x!x!");
    assert_eq!(reports.len(), 2, "{reports:#?}");
    assert!(reports.iter().all(|report| report.rule == "observed"));
}

const PARSER: &str = r#"
    parser toy in t {
        token character /./
        line_break /[\r\n]/
        goal code { character }
        node root { item: text* }
        root entry = emit(root, item:character+ end)
    }
"#;

#[test]
fn node_ordinals_ignore_fields_named_index() {
    let rules = r#"
        import .ts
        in ts_call.argument rule ordinal {
            warn "{node_ordinal(node)}:{node.index.text}"
        }
        in ts_program rule missing {
            when node_ordinal(node) != null || node_ordinal("not a node") != null
            warn "not a child"
        }
    "#;
    let diagnostics = linter(rules).lint(std::path::Path::new("/root/file.ts"), "f(a[x],b[y]);");
    assert_eq!(diagnostics.iter().map(|diagnostic| diagnostic.message.as_str()).collect::<Vec<_>>(), ["0:x", "1:y"]);
}

#[test]
fn mapped_value_trees_drive_real_vm_fixes_at_the_written_escape() {
    let rules = format!(r#"
        import .ts/javascript
        {PARSER}
        in .t match literal /'[^']*'/
        in literal rule mapped {{
            let decoded js_string_parts(text)
            let input {{ text: decoded.text, spans: decoded.spans, offset: offset + 1, end_offset: end_offset - 1 }}
            let parsed syntax_tree(node, "toy", input)
            let parsed_nodes get(parsed, "nodes")
            let tree_root {{ value parsed_nodes; filter (entry) entry.id == parsed.root; first }}
            let first_child first(tree_root.children)
            each part in parsed_nodes
            when part.kind == "TEXT" && part.text == "a"
            when part.parent == parsed.root && part.id == first_child
            when tree_root.children == tree_root.fields.item && count(tree_root.children) == 2
            fix at {{ start: part.offset, end: part.end_offset }} as "A"
            warn "mapped value"
        }}
    "#);
    let linter = linter(&rules);
    let fixed = linter.fix(std::path::Path::new("/root/file.t"), r"'\x61b'");
    assert_eq!(fixed.applied, 1);
    assert_eq!(fixed.text, "'Ab'");
}

#[test]
fn unknown_parsers_and_invalid_mappings_return_errors_not_partial_successes() {
    let rules = format!(r#"
        {PARSER}
        in .t match word /[a-z]+/
        in word rule errors {{
            let input {{ text: text, offset: offset, end_offset: end_offset, spans: [] }}
            let missing syntax_tree(node, "missing", input)
            let invalid syntax_tree(node, "toy", input)
            let missing_nodes get(missing, "nodes")
            let invalid_nodes get(invalid, "nodes")
            when missing.ok == false && invalid.ok == false
            when count(missing_nodes) == 0 && count(invalid_nodes) == 0
            warn "explicit errors"
        }}
    "#);
    assert_eq!(lint(&linter(&rules), "ab").len(), 1);
}

#[test]
fn expected_presence_does_not_expand_values_or_turn_failed_context_into_absence() {
    let linter = linter(r#"
        match unreadable /u/ { unwraps unknown }
        match container "c" argument:unreadable { carries argument using it as {} }
        in .t match present "p" context:container {
            in it expects context
            fits checked when value_has_expected(node)
            rule present warn "present" { when node always checked }
        }
        match absent /a/ {
            in it expects context
            fits checked when !value_has_expected(node)
            rule absent warn "absent" { when node always checked }
        }
        in .t match host "h" content:absent { fits context }
        in .t match failed /f/ {
            in it expects unknown
            fits invented when !value_has_expected(node)
            rule invented warn "invented" { when node always invented }
            rule unknown warn "unknown" { when value_has_expected(node) == null }
        }
    "#);
    let reports = lint(&linter, "pcu ha f");
    assert_eq!(reports.len(), 3, "{reports:#?}");
    for rule in ["present", "absent", "unknown"] {
        assert_eq!(reports.iter().filter(|report| report.rule == rule).count(), 1);
    }
}

#[test]
fn member_paths_follow_each_value_and_keep_missing_answers_unknown() {
    let linter = linter(r#"
        match good /a/ { fits good }
        match other /b/ { fits other }
        match cell "i" element:(good or other) { indexed element }
        in .t match container "c" element:cell {
            indexed element
            in it expects element
            let nested value_at(node, [0, 0])
            let contextual value_expected_at(node, [0])
            let missing value_at(node, [0, 1])
            let invalid value_at(node, [true])
            rule nested warn "nested" {
                when any(nested, arm => any(arm, part => contains(part.traits, "good")))
            }
            rule contextual warn "contextual" {
                when any(contextual, arm => any(arm, part => contains(part.traits, "good")))
            }
            rule unknown warn "unknown" { when missing == null && invalid == null }
        }
    "#);
    let reports = lint(&linter, "cia cib");
    assert_eq!(reports.iter().filter(|report| report.rule == "nested").count(), 1);
    assert_eq!(reports.iter().filter(|report| report.rule == "contextual").count(), 1);
    assert_eq!(reports.iter().filter(|report| report.rule == "unknown").count(), 2);
}

#[test]
fn parameter_checks_do_not_require_recursive_result_classification() {
    let linter = linter(r#"
        match callable /a/ { fits callable }
        in .t match producer "p" parameter:callable {
            fits callable
            signature parameter returns it
            let parameters call_parameters(node)
            fits checked when any(parameters, arm => any(arm, header => header.parameters_known))
            rule parameters warn "parameters" { when node always checked }
        }
    "#);
    assert_eq!(lint(&linter, "pa").len(), 1);
}

#[test]
fn known_parameter_traits_survive_unrelated_recursive_uncertainty_on_first_query() {
    let linter = linter(r#"
        match parameter /a/ {
            fits callable
            fits incidental when parent always checked
        }
        in .t match producer "p" parameter:parameter {
            fits callable
            signature parameter returns it
            let parameters call_parameters(node)
            fits checked when any(parameters, arm => any(arm, header =>
                header.parameters_known && any(header.parameters, parameter =>
                    any(parameter, parts => any(parts, part => contains(part.traits, "callable"))))))
            let missing if node could checked then null else { name: "missing" }
            rule missing warn missing.name { when missing != null }
            rule checked warn "checked" { when node always checked }
        }
    "#);
    let reports = lint(&linter, "pa");
    assert_eq!(reports.len(), 1, "{reports:#?}");
    assert_eq!(reports[0].rule, "checked");
}

#[test]
fn known_constituents_survive_uncertain_traits_without_inventing_negative_proofs() {
    let linter = linter(r#"
        match known /a/ {
            fits good
            fits incidental when node always incidental
        }
        in .t match producer "p" parameter:known {
            fits checked when any(value_constituents(parameter.node), arm =>
                any(arm, part => contains(part.traits, "good")
                    && contains(part.uncertain_traits, "incidental")))
            fits invented when !(parameter.node could incidental)
            rule checked warn "checked" { when node always checked }
            rule invented warn "invented" { when node always invented }
        }
    "#);
    let reports = lint(&linter, "pa");
    assert_eq!(reports.len(), 1, "{reports:#?}");
    assert_eq!(reports[0].rule, "checked");
}

#[test]
fn unreadable_carrier_arguments_cannot_be_known_signature_parameters() {
    let linter = linter(r#"
        match unreadable /u/ { unwraps unknown }
        match container "c" argument:unreadable { carries argument using it as {} }
        in .t match producer "p" parameter:container {
            fits callable
            signature parameter returns it
            rule unknown warn "unknown" {
                when any(call_parameters(node), arm => any(arm, header =>
                    !header.parameters_known && header.parameters == null))
            }
        }
    "#);
    assert_eq!(lint(&linter, "pcu").len(), 1);
}

#[test]
fn result_checks_do_not_require_resolving_parameters_and_keep_unknown_results() {
    let linter = linter(r#"
        match good /b/ { fits good }
        match parameter /a/ { fits demanded when parent always checked }
        match unreadable /u/ { unwraps unknown }
        in .t match producer "p" parameter:parameter output:(good or unreadable) {
            fits callable
            signature parameter returns output
            let results call_results(node)
            fits checked when any(results, arm => any(arm, header => header.result_known
                && any(header.result, parts => any(parts, part => contains(part.traits, "good")))))
            rule known warn "known" { when node always checked }
            rule unknown warn "unknown" {
                when any(results, arm => any(arm, header => !header.result_known && header.result == null))
            }
        }
    "#);
    let reports = lint(&linter, "pab pau");
    assert_eq!(reports.iter().filter(|report| report.rule == "known").count(), 1);
    assert_eq!(reports.iter().filter(|report| report.rule == "unknown").count(), 1);
}

#[test]
fn receiver_values_keep_declared_absence_distinct_from_unreadable_metadata() {
    let linter = linter(r#"
        match good /a/ { fits good }
        match missing /m/ { fits missing }
        match callable "f" parameter:"()"? context:good {
            fits callable
            signature parameter receiver context returns it
        }
        match absent "e" (parameter:good or "n") {
            fits callable
            signature parameter receiver parameter returns it
        }
        match unreadable "u" parameter:"()"? {
            fits callable
            signature parameter returns it
        }
        in .t match probe "q" value:(callable or absent or unreadable) {
            unwraps value.receiver ?? trait missing
            rule receiver warn "receiver" { when node always good }
            rule absent warn "absent" { when node always missing }
            rule unknown warn "unknown" { when value_constituents(node) == null }
        }
    "#);
    let reports = lint(&linter, "qfa qen qu");
    for rule in ["receiver", "absent", "unknown"] {
        assert_eq!(reports.iter().filter(|report| report.rule == rule).count(), 1, "{rule}");
    }
}

#[test]
fn carrier_arguments_are_projected_after_value_expansion() {
    let linter = linter(r#"
        match good /a/ { fits good; widens trait widened }
        match other /b/ { fits other }
        match wide /w/ { fits widened }
        in .t {
            match container "c" argument:(good or other) {
                carries argument using it.widened as { readonly: true }
            }
            match alias "r" value:container { unwraps value }
        }
        in alias {
            let parts value_constituents(node)
            rule argument warn "argument" {
                when any(parts, arm => any(arm, part =>
                    part.carrier_known && part.carrier.description.readonly
                    && any(part.carrier.arguments, argument =>
                        any(argument, row => any(row, item => contains(item.traits, "widened"))))))
            }
        }
    "#);
    let reports = lint(&linter, "rca rcb");
    assert_eq!(reports.len(), 1);
}

#[test]
fn projected_constituents_preserve_disjunction_and_conjunction() {
    let linter = linter(r#"
        match good /a/ { fits good }
        match other /b/ { fits other }
        in .t {
            match either left:good "|" right:other { unwraps left | right }
            match both left:good "&" right:other { unwraps left & right }
        }
        in either, both {
            let parts value_constituents(node)
            let first_arm first(parts)
            rule disjunction warn "union" { when len(parts) == 2 }
            rule conjunction warn "intersection" {
                when (len(parts) == 1) && (len(first_arm) == 2)
            }
            rule traits warn "traits" {
                when any(parts, arm => any(arm, part => contains(part.traits, "good")))
            }
        }
    "#);
    let reports = lint(&linter, "a|b a&b");
    assert_eq!(reports.iter().filter(|report| report.rule == "disjunction").count(), 1);
    assert_eq!(reports.iter().filter(|report| report.rule == "conjunction").count(), 1);
    assert_eq!(reports.iter().filter(|report| report.rule == "traits").count(), 2);
}

#[test]
fn callable_projection_preserves_results_and_unknown_receivers() {
    let linter = linter(r#"
        match good /a/ { fits good }
        match other /b/ { fits other }
        in .t {
            match producer "f" parameter:"()"? output:(good or other) {
                fits callable
                signature parameter returns output
            }
        }
        in producer {
            let signatures call_signatures(node)
            rule result warn "result" {
                when any(signatures, arm => any(arm, signature =>
                    signature.result_known
                    && any(signature.result, parts => any(parts, part => contains(part.traits, "good")))))
            }
            rule receiver warn "receiver" {
                when any(signatures, arm => any(arm, signature =>
                    signature.receiver_known == false && signature.has_receiver == null))
            }
        }
    "#);
    let reports = lint(&linter, "fa fb");
    assert_eq!(reports.iter().filter(|report| report.rule == "result").count(), 1);
    assert_eq!(reports.iter().filter(|report| report.rule == "receiver").count(), 2);
}
