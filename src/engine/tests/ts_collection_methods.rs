use std::path::Path;

use super::native_linter;

fn reports(source: &str) -> Vec<String> {
    let linter = native_linter("rule prefer-find");
    linter.lint(Path::new("/root/a.ts"), source).iter().map(|d| source[d.start..d.end].to_string()).collect()
}

#[test]
fn find_requires_array_constituents_and_nonoptional_consumption() {
    for (source, expected) in [
        ("declare const xs: number[] | string | null; xs?.filter(f)[0];", vec![]),
        ("[].filter?.(f)[0];", vec![]),
        ("[].filter(f)?.at?.(0);", vec![]),
        ("declare const xs: number[] | null; xs?.filter(f)[0];", vec!["xs?.filter(f)[0]"]),
        ("declare const xs: number[] & { brand: true }; xs.filter(f)[0];", vec![]),
        ("declare const xs: number[] & string[]; xs.filter(f)[0];", vec!["xs.filter(f)[0]"]),
        ("[].filter(f).at?.(0);", vec!["[].filter(f).at?.(0)"]),
    ] {
        assert_eq!(reports(source), expected, "{source}");
    }
}

#[test]
fn find_resolves_computed_methods_and_distinguishes_index_coercions() {
    for (source, expected) in [
        ("const filter = 'filter'; const zero = -0n; [1][filter](f)[zero];", vec!["[1][filter](f)[zero]"]),
        ("[1].filter(f)['0'];", vec!["[1].filter(f)['0']"]),
        ("[1].filter(f)[false]; [1].filter(f)[null]; [1].filter(f)['00'];", vec![]),
        ("declare const index: 0; [1].filter(f)[index];", vec![]),
        ("const index = 1 - 1; [1].filter(f)[index];", vec!["[1].filter(f)[index]"]),
        ("const index = -0.2; [1].filter(f)[`at`](index);", vec!["[1].filter(f)[`at`](index)"]),
        ("[1].filter(f).at(false); [1].filter(f).at(null); [1].filter(f).at(undefined); [1].filter(f).at(NaN);", vec!["[1].filter(f).at(false)", "[1].filter(f).at(null)", "[1].filter(f).at(undefined)", "[1].filter(f).at(NaN)"]),
        ("[1].filter(f).at('not a number');", vec!["[1].filter(f).at('not a number')"]),
        ("[1].filter(f).at('1_0'); [1].filter(f).at('inf'); [1].filter(f).at('\\u00852');", vec!["[1].filter(f).at('1_0')", "[1].filter(f).at('inf')", "[1].filter(f).at('\\u00852')"]),
        ("[1].filter(f).at('\\uFEFF2');", vec![]),
        ("[1].filter(f).at('0x2\\u0085');", vec!["[1].filter(f).at('0x2\\u0085')"]),
        ("[1].filter(f).at(1); [1].filter(f).at(-1); [1].filter(f).at(Infinity); [1].filter(f).at(); [1].filter(f).at(0, 1);", vec![]),
        ("let index = 0; index = 1; [1].filter(f)[index];", vec![]),
        ("function use(NaN: number, undefined: number) { [1].filter(f).at(NaN); [1].filter(f).at(undefined); }", vec![]),
    ] {
        assert_eq!(reports(source), expected, "{source}");
    }
}

#[test]
fn find_consumes_only_the_last_sequence_and_requires_both_branches() {
    for (source, expected) in [
        ("(sideEffect(), [1].filter(f))[0];", vec!["(sideEffect(), [1].filter(f))[0]"]),
        ("([1].filter(f), [1])[0];", vec![]),
        ("(choose ? [1].filter(f) : [2].filter(g))[0];", vec!["(choose ? [1].filter(f) : [2].filter(g))[0]"]),
        ("(choose ? [1].filter(f) : [2])[0];", vec![]),
        ("(choose ? [1].filter(f) : (chooseAgain ? [2].filter(g) : (sideEffect(), [3].filter(h)))).at(0.2);", vec!["(choose ? [1].filter(f) : (chooseAgain ? [2].filter(g) : (sideEffect(), [3].filter(h)))).at(0.2)"]),
    ] {
        assert_eq!(reports(source), expected, "{source}");
    }
}
