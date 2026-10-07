use std::path::Path;

use super::native_linter;

#[test]
fn parameter_property_hook_selects_both_conversion_directions_without_broadening_eligibility() {
    for (prefer, source) in [
        ("class-property", "class C { constructor(public a: number, readonly b: number, private readonly c: number, ordinary: number, ...rest: any[]) {} }"),
        ("parameter-property", "class C { public a: number; readonly b: number; private readonly c: number; wrong: string; initialised: number = 1; late: number; defaulted: number; constructor(a: number, b: number, c: number, wrong: number, initialised: number, late: number, defaulted: number = 1) { this.a = a; this.b = b; this.c = c; this.wrong = wrong; this.initialised = initialised; this.defaulted = defaulted; marker(); this.late = late; } }"),
    ] {
        for (predicate, heads) in [
            (None, vec!["public a:", "readonly b:", "private readonly c:"]),
            (Some("true"), vec!["public a:", "readonly b:", "private readonly c:"]),
            (Some("!contains(['readonly', 'private readonly'], parameter_modifiers)"), vec!["public a:"]),
            (Some("parameter_modifiers == 'private readonly'"), vec!["private readonly c:"]),
            (Some("false"), vec![]),
        ] {
            let hook = predicate.map_or_else(String::new, |predicate| format!("override in ts_parameter, ts_class_property let parameter_properties_checked {predicate}\n"));
            let diagnostics = native_linter(&format!("{hook}rule parameter-properties {{ set prefer '{prefer}' }}")).lint(Path::new("file.ts"), source);
            let starts: Vec<_> = diagnostics.iter().map(|diagnostic| diagnostic.start).collect();
            let expected: Vec<_> = heads.into_iter().map(|head| source.find(head).expect("convertible property in source")).collect();
            assert_eq!(starts, expected, "{prefer}/{predicate:?}: {diagnostics:?}");
            assert!(diagnostics.iter().all(|diagnostic| diagnostic.fix.is_none()));
        }
        let diagnostics = native_linter(&format!("override in ts_parameter, ts_class_property let parameter_properties_checked node is ts_parameter\nrule parameter-properties {{ set prefer '{prefer}' }}")).lint(Path::new("file.ts"), source);
        assert_eq!(diagnostics.len(), if prefer == "class-property" { 3 } else { 0 });
    }
}

#[test]
fn this_alias_hook_selects_declarations_and_assignments_without_changing_name_exemptions() {
    let source = "const self = this; const {a} = this; const [b] = this; target = this; ({c} = this); [d] = this; const nested = foo(this); const literal = 'this'; const paren = (this);";
    for (config, heads) in [
        ("", vec!["self", "target"]),
        ("override in ts_declarator, ts_assignment let no_this_alias_checked true\n", vec!["self", "{a}", "[b]", "target", "{c}", "[d]"]),
        ("override in ts_declarator, ts_assignment let no_this_alias_checked false\n", vec![]),
        ("override in ts_declarator let no_this_alias_checked name != 'self'\noverride in ts_assignment let no_this_alias_checked false\n", vec!["{a}", "[b]"]),
        ("override in ts_declarator, ts_assignment let no_this_alias_checked true\nrule no-this-alias { set allowedNames ['self', 'target'] }\n", vec!["{a}", "[b]", "{c}", "[d]"]),
    ] {
        let diagnostics = native_linter(&format!("{config}rule no-this-alias")).lint(Path::new("file.ts"), source);
        let starts: Vec<_> = diagnostics.iter().map(|diagnostic| diagnostic.start).collect();
        let expected: Vec<_> = heads.into_iter().map(|head| source.find(head).expect("alias target in source")).collect();
        assert_eq!(starts, expected, "{config}: {diagnostics:?}");
        assert!(diagnostics.iter().all(|diagnostic| diagnostic.fix.is_none()));
    }
}

#[test]
fn array_notation_modes_are_independent_for_mutable_and_readonly_types() {
    let source = "type MA = number[]; type MB = Array<number>; type MC = (A | B)[]; type MD = Array<A | B>; type RA = readonly number[]; type RB = ReadonlyArray<number>; type RC = readonly (A | B)[]; type RD = ReadonlyArray<A | B>;";
    let modes = [
        ("array", ["Array<number>", "Array<A | B>"], ["ReadonlyArray<number>", "ReadonlyArray<A | B>"]),
        ("generic", ["number[]", "(A | B)[]"], ["readonly number[]", "readonly (A | B)[]"]),
        ("array-simple", ["Array<number>", "(A | B)[]"], ["ReadonlyArray<number>", "readonly (A | B)[]"]),
    ];
    for (mutable, mutable_heads, _) in modes {
        for (readonly, _, readonly_heads) in modes {
            let linter = native_linter(&format!("rule array-type:mutable set '{mutable}'\nrule array-type:readonly set '{readonly}'"));
            let diagnostics = linter.lint(Path::new("file.ts"), source);
            let starts: Vec<_> = diagnostics.iter().map(|diagnostic| diagnostic.start).collect();
            let expected: Vec<_> = mutable_heads.into_iter().chain(readonly_heads).map(|head| source.find(head).expect("array notation in source")).collect();
            assert_eq!(starts, expected, "{mutable}/{readonly}: {diagnostics:?}");
            assert!(diagnostics.iter().all(|diagnostic| diagnostic.fix.is_none()));
        }
    }
    for (config, heads) in [
        ("rule array-type:mutable set 'generic'", vec!["number[]", "(A | B)[]"]),
        ("rule array-type:readonly set 'generic'", vec!["readonly number[]", "readonly (A | B)[]"]),
        ("rule array-type\nrule array-type:mutable set 'generic'", vec!["number[]", "(A | B)[]", "ReadonlyArray<number>", "ReadonlyArray<A | B>"]),
    ] {
        let diagnostics = native_linter(config).lint(Path::new("file.ts"), source);
        let starts: Vec<_> = diagnostics.iter().map(|diagnostic| diagnostic.start).collect();
        let expected: Vec<_> = heads.into_iter().map(|head| source.find(head).expect("array notation in source")).collect();
        assert_eq!(starts, expected, "{config}: {diagnostics:?}");
    }
}

#[test]
fn array_notation_preserves_wrapper_eligibility_and_readonly_simple_classification() {
    let source = "type A = Readonly<number[]>; type B = Readonly<(X | Y)[]>; type C = Readonly<number>; type D = Array<number, string>; type E = ReadonlyArray<number, string>; type F = readonly [number]; type G = NS.Array<number>;";
    for mode in ["array", "array-simple"] {
        let diagnostics = native_linter(&format!("rule array-type set '{mode}'")).lint(Path::new("file.ts"), source);
        let starts: Vec<_> = diagnostics.iter().map(|diagnostic| diagnostic.start).collect();
        let heads = if mode == "array-simple" { vec!["Readonly<number[]>", "Readonly<(X | Y)[]>", "(X | Y)[]"] } else { vec!["Readonly<number[]>", "Readonly<(X | Y)[]>"] };
        let expected: Vec<_> = heads.into_iter().map(|head| source.find(head).expect("array notation in source")).collect();
        assert_eq!(starts, expected, "{mode}: {diagnostics:?}");
    }
    let source = "type Array<T> = T; type ReadonlyArray<T> = T; type Readonly<T> = T; type A = Array<number>; type B = ReadonlyArray<number>; type C = Readonly<number[]>;";
    assert!(native_linter("rule array-type").lint(Path::new("file.ts"), source).is_empty());
}

#[test]
fn inferrable_annotation_hook_preserves_widening_exceptions_and_parameter_positions() {
    let source = "let variable: number = 1; let inferred = 1; let other: number = f(); class C { public property: number = 1; readonly wide: number = 1; optional?: number = 1; constructor(public promoted: number = 1) {} } function f(parameter: number = 1) {}";
    for (predicate, heads) in [
        (None, vec!["variable:", "public property:", "promoted:", "parameter:"]),
        (Some("true"), vec!["variable:", "public property:", "promoted:", "parameter:"]),
        (Some("node not ts_parameter"), vec!["variable:", "public property:"]),
        (Some("node is ts_class_property && node.key == \"property\""), vec!["public property:"]),
        (Some("false"), vec![]),
    ] {
        let config = predicate.map_or_else(String::new, |predicate| format!("override in inferrable_annotation let no_inferrable_types_checked {predicate}\n"));
        let linter = native_linter(&format!("{config}rule no-inferrable-types"));
        let diagnostics = linter.lint(Path::new("file.ts"), source);
        let starts: Vec<_> = diagnostics.iter().map(|diagnostic| diagnostic.start).collect();
        let expected: Vec<_> = heads.iter().map(|head| source.find(head).expect("declaration in source")).collect();
        assert_eq!(starts, expected, "{predicate:?}: {diagnostics:?}");
    }
}

#[test]
fn inferrable_annotation_group_drops_only_selected_declaration_kinds() {
    let source = "let value: number = 1; class C { property: number = 1; } function f(parameter: number = 1) {}";
    for (kind, omitted) in [
        ("ts_declarator", "value:"),
        ("ts_class_property", "property:"),
        ("ts_parameter", "parameter:"),
    ] {
        let linter = native_linter(&format!("drop group inferrable_annotation {kind}\nrule no-inferrable-types"));
        let diagnostics = linter.lint(Path::new("file.ts"), source);
        assert_eq!(diagnostics.len(), 2, "{kind}: {diagnostics:?}");
        assert!(!diagnostics.iter().any(|diagnostic| diagnostic.start == source.find(omitted).expect("omitted declaration in source")));
    }
}

#[test]
fn explicit_any_hook_selects_rest_array_context_without_hiding_other_annotations() {
    let source = "type Ordinary = any; function arrays(...args: any[]) {} type Callback = (...args: ReadonlyArray<any>) => void; function immutable(...args: readonly any[]) {} function bare(...args: any) {} function nested(...args: any[][]) {} function tuple(...args: [any]) {} let value: Array<any>; let marker: number; function unrelated(...args: Foo<any>) {} function genericNested(...args: Array<Array<any>>) {}";
    for (predicate, expected) in [
        ("true", 10),
        ("parent.rest_array_argument != true", 7),
        ("parent not ts_type_alias", 9),
        ("false", 0),
    ] {
        let linter = native_linter(&format!("override in ts_type_keyword let no_explicit_any_checked {predicate}\nrule no-explicit-any"));
        let diagnostics = linter.lint(Path::new("/root/file.ts"), source);
        assert_eq!(diagnostics.len(), expected, "{predicate}: {diagnostics:?}");
        assert!(diagnostics.iter().all(|diagnostic| &source[diagnostic.start..diagnostic.end] == "any"));
    }
}

#[test]
fn namespace_hook_controls_files_and_declarations_without_enabling_intrinsic_exemptions() {
    let source = "namespace Root { namespace Nested {} } declare namespace Ambient {} declare global { interface Global {} } declare module 'external' {} namespace global {}";
    for (predicate, regular, definition) in [
        (None, 3, 0),
        (Some("true"), 3, 3),
        (Some("namespace_declared != true"), 2, 2),
        (Some("name == \"Root\""), 1, 1),
        (Some("false"), 0, 0),
    ] {
        let config = predicate.map_or_else(String::new, |predicate| format!("override in ts_namespace let no_namespace_checked {predicate}\n"));
        let linter = native_linter(&format!("{config}rule no-namespace"));
        for (file, expected) in [("file.ts", regular), ("file.d.ts", definition), ("file.d.cts", definition), ("file.d.mts", definition)] {
            let diagnostics = linter.lint(Path::new(file), source);
            assert_eq!(diagnostics.len(), expected, "{predicate:?} {file}: {diagnostics:?}");
        }
    }
}
