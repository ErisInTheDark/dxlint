use std::path::Path;

use super::native_linter;

#[test]
fn enum_assignment_distinguishes_shared_identity_and_non_enum_union_alternatives() {
    let lint = native_linter("import .ts/enums\nrule no-unsafe-enum-assignment");
    let source = "enum Fruit { Apple } enum Vegetable { Carrot } const safe: Fruit = Fruit.Apple; const wide: Fruit | number = 1; declare const uncertain: Missing; const unresolved: Fruit = uncertain; const wrong: Fruit = 1; const other: Fruit = Vegetable.Carrot;";
    let diagnostics = lint.lint(Path::new("/root/a.ts"), source);
    let spans: Vec<_> = diagnostics.iter().map(|d| &source[d.start..d.end]).collect();
    assert_eq!(spans, ["wrong: Fruit = 1", "other: Fruit = Vegetable.Carrot"], "{diagnostics:#?}");
}

#[test]
fn enum_assignment_descends_matching_generic_and_object_positions() {
    let lint = native_linter("import .ts/enums\nrule no-unsafe-enum-assignment");
    let source = "enum Fruit { Apple } interface Box<T> { value: T } declare const numeric: Box<number>; declare const fruit: Box<Fruit>; const safe: Box<Fruit> = fruit; const wrong: Box<Fruit> = numeric; declare const numbers: { nested: { fruit: number } }; const nested: { nested: { fruit: Fruit } } = numbers;";
    let diagnostics = lint.lint(Path::new("/root/a.ts"), source);
    let spans: Vec<_> = diagnostics.iter().map(|d| &source[d.start..d.end]).collect();
    assert_eq!(spans, ["wrong: Box<Fruit> = numeric", "nested: { nested: { fruit: Fruit } } = numbers"], "{diagnostics:#?}");
}

#[test]
fn enum_transfers_keep_own_bitflags_but_reject_unrelated_operands_and_mutations() {
    let lint = native_linter("import .ts/enums\nrule no-unsafe-enum-assignment");
    let source = "enum Flags { Read = 1, Write = 2 } const own: Flags = Flags.Read | Flags.Write; const nested: Flags = Flags.Read | (Flags.Write & Flags.Read); const cast = (Flags.Read | Flags.Write) as Flags; declare let flags: Flags; flags &= ~Flags.Read; flags |= 8; flags += 1; flags++; const asserted = 1 as Flags;";
    let diagnostics = lint.lint(Path::new("/root/a.ts"), source);
    let spans: Vec<_> = diagnostics.iter().map(|d| &source[d.start..d.end]).collect();
    assert_eq!(spans, ["flags |= 8", "flags += 1", "flags++", "1 as Flags"], "{diagnostics:#?}");
}

#[test]
fn enum_object_literals_report_the_property_not_the_containing_declaration() {
    let lint = native_linter("import .ts/enums\nrule no-unsafe-enum-assignment");
    let source = "enum Fruit { Apple } const box: { fruit: Fruit } = { fruit: 1 }; const wrapped: { fruit: Fruit } = ({ fruit: 1 }); const safe: { fruit: Fruit } = { fruit: Fruit.Apple };";
    let diagnostics = lint.lint(Path::new("/root/a.ts"), source);
    let spans: Vec<_> = diagnostics.iter().map(|d| &source[d.start..d.end]).collect();
    assert_eq!(spans, ["fruit: 1", "fruit: 1"], "{diagnostics:#?}");
}

#[test]
fn enum_class_members_require_every_heritage_contract_or_their_own_annotation() {
    let lint = native_linter("import .ts/enums\nrule no-unsafe-enum-assignment");
    let source = "enum Fruit { Apple } interface Basket { fruit: Fruit } interface Numeric { fruit: number } class Bad implements Basket { fruit = 1; } class Mixed implements Basket, Numeric { fruit = 1; } class Own implements Numeric { fruit: Fruit = 1; }";
    let diagnostics = lint.lint(Path::new("/root/a.ts"), source);
    let spans: Vec<_> = diagnostics.iter().map(|d| &source[d.start..d.end]).collect();
    assert_eq!(spans, ["fruit = 1;", "fruit: Fruit = 1;"], "{diagnostics:#?}");
}

#[test]
fn enum_heritage_literal_report_does_not_duplicate_its_nested_property() {
    let lint = native_linter("import .ts/enums\nrule no-unsafe-enum-assignment");
    let source = "enum Fruit { Apple } interface HasBox { box: { fruit: Fruit } } class Bad implements HasBox { box: { fruit: Fruit } = { fruit: 1 }; }";
    let diagnostics = lint.lint(Path::new("/root/a.ts"), source);
    let spans: Vec<_> = diagnostics.iter().map(|d| &source[d.start..d.end]).collect();
    assert_eq!(spans, ["box: { fruit: Fruit } = { fruit: 1 };"], "{diagnostics:#?}");
}
