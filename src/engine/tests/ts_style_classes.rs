use std::path::Path;

use super::native_linter;

#[test]
fn extraneous_class_variants_disable_only_their_own_shape() {
    let source = "class Empty {} class Constructor { constructor() {} } class Static { static value = 1; } class Inherited extends Base {} class Promoted { constructor(public value: number) {} } class Instance { value = 1; }";
    for (disabled, names) in [
        (None, vec!["Empty", "Constructor", "Static"]),
        (Some("empty"), vec!["Constructor", "Static"]),
        (Some("constructor"), vec!["Empty", "Static"]),
        (Some("static"), vec!["Empty", "Constructor"]),
    ] {
        let config = disabled.map_or_else(String::new, |shape| format!("\nrule no-extraneous-class:{shape} off"));
        let diagnostics = native_linter(&format!("rule no-extraneous-class{config}")).lint(Path::new("file.ts"), source);
        let starts: Vec<_> = diagnostics.iter().map(|diagnostic| diagnostic.start).collect();
        assert_eq!(starts, names.iter().map(|name| source.find(name).expect("class name")).collect::<Vec<_>>(), "{disabled:?}: {diagnostics:?}");
        assert!(diagnostics.iter().all(|diagnostic| diagnostic.fix.is_none()));
    }
}

#[test]
fn extraneous_class_hook_preserves_intrinsic_exemptions_and_report_positions() {
    for (source, head, decorated) in [
        ("class Empty {}", Some("Empty"), false),
        ("@dec class Decorated {}", Some("Decorated"), true),
        ("class Constructed { constructor() { work(); } }", Some("Constructed"), false),
        ("class Block { static {} }", Some("Block"), false),
        ("class Mixed { constructor() {} static value = 1; }", Some("Mixed"), false),
        ("const value = class {};", Some("class"), false),
        ("export class Exported {}", Some("Exported"), false),
        ("class Derived extends Base {}", None, false),
        ("class Promoted { constructor(private readonly value: number) {} }", None, false),
        ("abstract class Abstract { abstract method(): void; }", None, false),
        ("class Instance { value = 1; }", None, false),
    ] {
        for (predicate, checked) in [(None, true), (Some("true"), true), (Some("false"), false), (Some("count(decorator) == 0"), !decorated)] {
            let hook = predicate.map_or_else(String::new, |predicate| format!("override in ts_class let no_extraneous_class_checked {predicate}\n"));
            let diagnostics = native_linter(&format!("{hook}rule no-extraneous-class")).lint(Path::new("file.ts"), source);
            let starts: Vec<_> = diagnostics.iter().map(|diagnostic| diagnostic.start).collect();
            let expected: Vec<_> = head.filter(|_| checked).map(|head| source.find(head).expect("class report position")).into_iter().collect();
            assert_eq!(starts, expected, "{predicate:?}/{source}: {diagnostics:?}");
        }
    }
    let diagnostics = native_linter("override in ts_class let no_extraneous_class_checked name == \"Checked\"\nrule no-extraneous-class")
        .lint(Path::new("file.ts"), "class Skipped {} class Checked {}");
    assert_eq!(diagnostics.len(), 1);
    assert_eq!(diagnostics[0].start, "class Skipped {} class ".len());
}
