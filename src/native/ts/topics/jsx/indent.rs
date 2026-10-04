//! Prop indentation follows the tag's line, with one ternary continuation step.

use super::metadata::{line_starts, space};
use super::super::super::facts::NativeFile;
use super::super::super::kinds::Field as F;
use crate::expr::Value;

fn counts(text: &str) -> (usize, usize) {
    (text.chars().take_while(|&ch| ch == ' ').count(), text.chars().take_while(|&ch| ch == '\t').count())
}

fn operator(text: &str) -> bool {
    text.trim_start_matches([' ', '\t']).starts_with(['?', ':'])
}

pub fn props(file: &NativeFile, node: u32) -> Value<'static> {
    let s = &file.syntax;
    let attrs: Vec<_> = s.all(node, F::Property).collect();
    let Some(&first) = attrs.first() else { return Value::Null };
    let lines = file.cached(line_starts);
    let own = s.tree.node(node);
    let head = s.src[lines.start(own.start as usize)..].split('\n').next().unwrap_or("");
    let (spaces, tabs) = counts(head);
    let mut using_operator = operator(head);
    let mut extra = false;
    let props = attrs.into_iter().map(|id| {
        let attr = s.tree.node(id);
        let start = attr.start as usize;
        let prefix = &s.src[lines.start(start)..start];
        let text = s.src[lines.start(start)..attr.end as usize].split('\n').next().unwrap_or("");
        let current_operator = operator(text);
        if current_operator { using_operator = true; }
        else if text.contains('<') { using_operator = false; }
        if using_operator && !current_operator { extra = true; using_operator = false; }
        let (spaces, tabs) = counts(prefix);
        Value::record(vec![
            ("offset".into(), Value::from(start)),
            ("first_in_line".into(), Value::Bool(prefix.chars().all(space))),
            ("spaces".into(), Value::from(spaces)),
            ("tabs".into(), Value::from(tabs)),
            ("extra".into(), Value::Bool(extra)),
        ])
    }).collect();
    Value::record(vec![
        ("spaces".into(), Value::from(spaces)),
        ("tabs".into(), Value::from(tabs)),
        ("first_column".into(), Value::from(lines.column(s.src, s.tree.node(first).start as usize))),
        ("props".into(), Value::list(props)),
    ])
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use crate::engine::tests::native_linter;

    #[test]
    fn ternary_continuation_indentation_stays_with_its_opening_tag() {
        let text = "const x = flag\n  ? <Box\n      a\n      b\n    />\n  : <Box\n      c\n      d\n    />;\nconst y = <Box\n  e\n  f\n/>;";
        let l = native_linter("rule jsx-indent-props { set mode 2 }");
        assert!(l.lint(Path::new("/root/file.tsx"), text).is_empty());
        let ignored = native_linter("rule jsx-indent-props {\nset mode 2\nset ignoreTernaryOperator true\n}");
        assert_eq!(ignored.lint(Path::new("/root/file.tsx"), text).len(), 4);
    }
}
