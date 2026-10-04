//! Immediate function invocation syntax.

use crate::native::ts::{kinds::{Field as F, Kind as K}, statics, syntax::Syntax};

pub(super) fn iife_function(s: &Syntax, node: u32) -> Option<(u32, bool)> {
    if !s.is(node, K::Call) {
        return None;
    }
    let callee = s.unparen(s.get(node, F::Callee)?);
    if s.is(callee, K::Function) {
        return Some((callee, false));
    }
    if matches!(s.kind(callee), Some(K::Member | K::Index)) {
        let object = s.unparen(s.get(callee, F::Object)?);
        if s.is(object, K::Function) && statics::property_name(s, callee).is_some_and(|name| matches!(name.as_str(), "call" | "apply")) {
            return Some((object, true));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use crate::engine::tests::native_linter;

    #[test]
    fn construct_parens_count_but_argument_separators_do_not() {
        for (source, outside, inside) in [
            ("import(function (){}());", 0, 1),
            ("consume(function (){}());", 0, 1),
            ("consume(function (){}(), other);", 1, 1),
            ("consume(function (){}(),);", 1, 1),
            ("consume((function (){})());", 0, 0),
        ] {
            for (mode, count) in [("outside", outside), ("inside", inside)] {
                let lint = native_linter(&format!("rule wrap-iife {{ set mode \"{mode}\" }}"));
                assert_eq!(lint.lint(Path::new("/root/a.ts"), source).len(), count, "{mode}: {source}");
            }
        }
    }

    #[test]
    fn chain_depth_tracks_receiver_invocations_through_parentheses() {
        let lint = native_linter("rule newline-per-chained-call");
        for (source, count) in [
            ("a().b().c();", 1),
            ("((a().b())).c();", 1),
            ("((a().b))().c();", 1),
            ("new A().b().c();", 1),
            ("a(foo()).b();", 0),
            ("a().b.c();", 0),
            ("((a().b())\n).c();", 0),
        ] {
            assert_eq!(lint.lint(Path::new("/root/a.ts"), source).len(), count, "{source}");
        }
    }

    #[test]
    fn chain_breaks_use_code_positions_after_unicode() {
        let lint = native_linter("rule newline-per-chained-call");
        let source = "\u{03b1}\u{03b1}; a().b()\n.c();";
        let reports = lint.lint(Path::new("/root/a.ts"), source);
        assert!(reports.is_empty(), "{reports:?}");
    }
}
