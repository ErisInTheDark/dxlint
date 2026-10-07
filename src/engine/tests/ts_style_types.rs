use std::path::Path;

use super::native_linter;

#[test]
fn restricted_type_names_preserve_shape_and_reference_routes() {
    for (source, names, expected) in [
        ("let value: Banned;", "[\"Banned\"]", vec!["Banned"]),
        ("let value: Banned;", "[\"banned\"]", vec![]),
        ("let value: NS . Banned;", "[\" NS.Banned \"]", vec!["NS . Banned"]),
        ("let value: Banned<A, B>;", "[\" Banned < A , B > \"]", vec!["Banned<A, B>"]),
        ("let value: Banned<any>;", "[\"Banned\", \"Banned<any>\"]", vec!["Banned", "Banned<any>"]),
        ("let value: number;", "[\"number\"]", vec!["number"]),
        ("let value: [];", "[\"[]\"]", vec!["[]"]),
        ("let value: {};", "[\"{}\"]", vec!["{}"]),
        ("let value: { field: number };", "[\"{field:number}\"]", vec![]),
        ("let value: [number];", "[\"[number]\"]", vec![]),
        ("let value = Banned();", "[\"Banned\"]", vec![]),
    ] {
        let escaped = names.replace('{', "{{").replace('}', "}}");
        let linter = native_linter(&format!("rule no-restricted-types set {escaped}"));
        let diagnostics = linter.lint(Path::new("file.ts"), source);
        let mut spans: Vec<_> = diagnostics.iter().map(|diagnostic| (diagnostic.start, diagnostic.end)).collect();
        spans.sort_unstable();
        let mut expected: Vec<_> = expected.iter().map(|text| {
            let start = source.find(text).expect("restricted type in source");
            (start, start + text.len())
        }).collect();
        expected.sort_unstable();
        assert_eq!(spans, expected, "{names}/{source}: {diagnostics:?}");
        assert!(diagnostics.iter().all(|diagnostic| diagnostic.fix.is_none()));
        assert!(native_linter("rule no-restricted-types").lint(Path::new("file.ts"), source).is_empty());
    }
    let diagnostics = native_linter("rule no-restricted-types set ['Banned', '{{}}']\nrule no-restricted-types warn \"{restricted_name}\"")
        .lint(Path::new("file.ts"), "let a: Banned; let b: {};");
    assert_eq!(diagnostics.iter().map(|diagnostic| diagnostic.message.as_str()).collect::<Vec<_>>(), ["Banned", "{}"]);
}

#[test]
fn void_type_hook_selects_contexts_without_enabling_intrinsic_exemptions() {
    for (source, default, checked) in [
        ("type Value = Promise<void>;", 0, 1),
        ("type Value<T = void> = T;", 0, 1),
        ("function f(this: void) {}", 1, 1),
        ("let value: void;", 1, 1),
        ("type Value = void | string;", 1, 1),
        ("f<void>();", 1, 1),
        ("new C<void>();", 0, 1),
        ("function f(): void {}", 0, 0),
        ("type Callback = () => void;", 0, 0),
        ("type Value = void | never | Promise<void>;", 0, 1),
        ("function f(value: string): void; function f(value: string | void): void {}", 0, 0),
        ("let value: number;", 0, 0),
    ] {
        for (predicate, expected) in [(None, default), (Some("true"), checked), (Some("false"), 0)] {
            let hook = predicate.map_or_else(String::new, |predicate| format!("override in ts_type_keyword let no_invalid_void_type_checked {predicate}\n"));
            let diagnostics = native_linter(&format!("{hook}rule no-invalid-void-type")).lint(Path::new("file.ts"), source);
            assert_eq!(diagnostics.len(), expected, "{predicate:?}/{source}: {diagnostics:?}");
            assert!(diagnostics.iter().all(|diagnostic| &source[diagnostic.start..diagnostic.end] == "void" && diagnostic.fix.is_none()));
        }
    }
    let source = "type Allowed = ns.Promise<void>; type Other = Box<void>; function f(this: void) {} let value: void;";
    let linter = native_linter("override in ts_type_keyword let no_invalid_void_type_checked void_type_context != \"this\" && !(void_type_context == \"generic\" && void_type_generic_name == \"ns.Promise\")\nrule no-invalid-void-type");
    let diagnostics = linter.lint(Path::new("file.ts"), source);
    let starts: Vec<_> = diagnostics.iter().map(|diagnostic| diagnostic.start).collect();
    assert_eq!(starts, [source.find("Box<void>").expect("generic type") + 4, source.rfind("void").expect("variable type")]);
}

#[test]
fn literal_assertion_modes_are_independent_under_both_preferred_notations() {
    let modes = [("allow", false, false), ("allow-as-parameter", true, false), ("never", true, true)];
    for (style, object, array) in [("as", "{} as T", "[] as T[]"), ("angle-bracket", "<T>{}", "<T[]>[]")] {
        let source = format!("const object = {object}; const array = {array}; take({object}, {array});");
        for (object_mode, object_direct, object_argument) in modes {
            for (array_mode, array_direct, array_argument) in modes {
                let linter = native_linter(&format!("rule consistent-type-assertions {{ set assertionStyle '{style}' }}\nrule consistent-type-assertions:object set '{object_mode}'\nrule consistent-type-assertions:array set '{array_mode}'"));
                let diagnostics = linter.lint(Path::new("file.ts"), &source);
                let starts: Vec<_> = diagnostics.iter().map(|diagnostic| diagnostic.start).collect();
                let expected: Vec<_> = [(object_direct, source.find(object)), (array_direct, source.find(array)), (object_argument, source.rfind(object)), (array_argument, source.rfind(array))]
                    .into_iter().filter(|(reported, _)| *reported).map(|(_, offset)| offset.expect("literal assertion in source")).collect();
                assert_eq!(starts, expected, "{style}/{object_mode}/{array_mode}: {diagnostics:?}");
                assert!(diagnostics.iter().all(|diagnostic| diagnostic.fix.is_none()));
            }
        }
    }
}

#[test]
fn literal_assertion_argument_contexts_and_intrinsic_exclusions_remain_intact() {
    let config = "rule consistent-type-assertions\nrule consistent-type-assertions:object set 'allow-as-parameter'";
    for (source, file, expected) in [
        ("const value = ({} as T);", "file.ts", 1),
        ("take(({} as T));", "file.ts", 0),
        ("new C(({} as T));", "file.ts", 0),
        ("throw ({} as T);", "file.ts", 0),
        ("function f(value = ({} as T)) {}", "file.ts", 0),
        ("[value = {} as T] = source;", "file.ts", 1),
        ("const [value = {} as T] = source;", "file.ts", 0),
        ("tag`${{} as T}`;", "file.ts", 0),
        ("const text = `${{} as T}`;", "file.ts", 1),
        ("<C value={{} as T} />;", "file.tsx", 0),
    ] {
        let diagnostics = native_linter(config).lint(Path::new(file), source);
        assert_eq!(diagnostics.len(), expected, "{source}: {diagnostics:?}");
    }
    let source = "const a = {} as any; const b = [] as unknown; const c = {} as const; const d = value as T;";
    assert!(native_linter("rule consistent-type-assertions\nrule consistent-type-assertions:object set 'never'\nrule consistent-type-assertions:array set 'never'").lint(Path::new("file.ts"), source).is_empty());
    let source = "const a = {} as T; const b = [] as T[]; const c = <T>{}; const d = <T[]>[]; const e = {} as const; const f = <const>[];";
    for style in ["as", "angle-bracket", "never"] {
        let diagnostics = native_linter(&format!("rule consistent-type-assertions {{ set assertionStyle '{style}' }}\nrule consistent-type-assertions:object set 'never'\nrule consistent-type-assertions:array set 'never'")).lint(Path::new("file.ts"), source);
        let starts: Vec<_> = diagnostics.iter().map(|diagnostic| diagnostic.start).collect();
        let mut unique = starts.clone();
        unique.sort_unstable();
        unique.dedup();
        assert_eq!(starts.len(), unique.len(), "{style}: {diagnostics:?}");
        assert_eq!(diagnostics.len(), if style == "never" { 4 } else { 5 }, "{style}: {diagnostics:?}");
    }
}

#[test]
fn empty_object_contexts_hooks_and_name_patterns_preserve_intrinsic_exemptions() {
    let source = "interface EmptyInterface {} interface SingleInterface extends Base {} interface KeepInterface {} interface Multiple extends Base, Other {} interface Nonempty { value: string } type EmptyAlias = {}; type KeepAlias = {}; type Intersection = {} & Marker; let inline: {};";
    let targets = [
        ("empty", "EmptyInterface", 0),
        ("single", "SingleInterface", 0),
        ("keep-interface", "KeepInterface", 0),
        ("alias", "EmptyAlias = ", "EmptyAlias = ".len()),
        ("keep-alias", "KeepAlias = ", "KeepAlias = ".len()),
        ("inline", "inline: ", "inline: ".len()),
    ];
    for (config, selected) in [
        ("rule no-empty-object-type", vec!["empty", "single", "keep-interface", "alias", "keep-alias", "inline"]),
        ("rule no-empty-object-type:object", vec!["alias", "keep-alias", "inline"]),
        ("rule no-empty-object-type:interface", vec!["empty", "single", "keep-interface"]),
        ("rule no-empty-object-type { set allowSingleExtends true }", vec!["empty", "keep-interface", "alias", "keep-alias", "inline"]),
        ("override in ts_interface, ts_type_literal let no_empty_object_type_checked true\nrule no-empty-object-type", vec!["empty", "single", "keep-interface", "alias", "keep-alias", "inline"]),
        ("override in ts_interface, ts_type_literal let no_empty_object_type_checked false\nrule no-empty-object-type", vec![]),
        ("override in ts_interface let no_empty_object_type_checked name != 'EmptyInterface'\noverride in ts_type_literal let no_empty_object_type_checked parent not ts_type_alias\nrule no-empty-object-type", vec!["single", "keep-interface", "inline"]),
        ("rule no-empty-object-type { set allowWithName /^Keep/ }", vec!["empty", "single", "alias", "inline"]),
        ("rule no-empty-object-type { set allowWithName '' }", vec!["inline"]),
        ("rule no-empty-object-type { set allowWithName null }", vec!["empty", "single", "keep-interface", "alias", "keep-alias", "inline"]),
    ] {
        let diagnostics = native_linter(config).lint(Path::new("file.ts"), source);
        let starts: Vec<_> = diagnostics.iter().map(|diagnostic| diagnostic.start).collect();
        let expected: Vec<_> = targets.iter().filter(|(key, _, _)| selected.contains(key)).map(|(_, marker, advance)| source.find(marker).expect("empty type in source") + advance).collect();
        assert_eq!(starts, expected, "{config}: {diagnostics:?}");
        assert!(diagnostics.iter().all(|diagnostic| diagnostic.fix.is_none()));
    }
}

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
fn inferrable_annotation_hook_preserves_widening_exceptions_and_annotation_positions() {
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
        let expected: Vec<_> = heads.iter().map(|head| {
            let after_name = source.find(head).expect("declaration in source") + head.len();
            after_name + source[after_name..].find("number").expect("type annotation")
        }).collect();
        assert_eq!(starts, expected, "{predicate:?}: {diagnostics:?}");
        assert!(diagnostics.iter().all(|diagnostic| &source[diagnostic.start..diagnostic.end] == "number"));
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
        let after_name = source.find(omitted).expect("omitted declaration in source") + omitted.len();
        let omitted_type = after_name + source[after_name..].find("number").expect("omitted type annotation");
        assert!(!diagnostics.iter().any(|diagnostic| diagnostic.start == omitted_type));
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
