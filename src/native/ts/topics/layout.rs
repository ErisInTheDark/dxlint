//! @stylistic spacing, punctuation and line breaks: lexical neighbours.

use crate::expr::facts::Fact;
use crate::expr::Value;
use crate::native::ts::facts::NativeFile;
use crate::native::ts::lexer::Token;

use crate::native::ts::tokens::{gap_has_space, lex};

mod punctuation;
mod delimiters;
mod lists;
mod quotes;

pub const FACTS: &[Fact] = &[
    Fact {
        name: "statement_semicolon",
        doc: "native semicolon-owning statements: {present,offset,end,redundant,same_line,hazard_before,hazard_after,class_hazard,class_field,one_line_block,one_line_class}; null for declarations owned elsewhere and for-header variables",
        get: |f, n| f.with_file(punctuation::statement_semicolon, n),
    },
    Fact {
        name: "comma_line_sites",
        doc: "native list owners: comma records {owner,offset,before_break,after_break}; parser item boundaries exclude nested commas and initial array holes",
        get: |f, n| f.with_file(lists::comma_line_sites, n),
    },
    Fact {
        name: "property_quote_sites",
        doc: "native properties/signatures/enums and object/interface/type/enum groups: {group,quoted,numeric,redundant,redundant_number,keyword,name,offset}; computed keys, methods and shorthand object properties exempt",
        get: |f, n| f.with_file(quotes::property_quote_sites, n),
    },
    Fact {
        name: "token_neighbours",
        doc: "native layers: adjacent tokens as before/after records {text,start,end,gap,spaced,same_line,line_break}; gap retains comments, spaced counts whitespace outside comments, line_break includes comment line breaks; parent_start is the parent's byte offset",
        get: |f, n| f.with_file(token_neighbours, n),
    },
    Fact {
        name: "trailing_comma_sites",
        doc: "native list owners: {kind,offset,trailing,multiline,allowed,tsx_single}; offset is comma or insertion point, multiline compares last item/comma with closing token",
        get: |f, n| f.with_file(lists::trailing_comma_sites, n),
    },
];

fn edge(src: &str, token: Option<&Token>, start: usize, end: usize) -> Value<'static> {
    let Some(token) = token else { return Value::Null };
    let token_start = token.start as usize;
    let token_end = token.end as usize;
    let gap = &src[start..end];
    let line_break = gap.contains(['\n', '\r', '\u{2028}', '\u{2029}']);
    Value::record([
        ("text".into(), Value::text_copy(&src[token_start..token_end])),
        ("start".into(), Value::from(token_start)),
        ("end".into(), Value::from(token_end)),
        ("gap".into(), Value::text_copy(gap)),
        ("spaced".into(), Value::Bool(gap_has_space(gap))),
        ("same_line".into(), Value::Bool(!line_break)),
        ("line_break".into(), Value::Bool(line_break)),
    ])
}

fn token_neighbours(file: &NativeFile<'_>, node: u32) -> Value<'static> {
    let src = file.syntax.src;
    let syntax_node = file.syntax.tree.node(node);
    let start = syntax_node.start as usize;
    let end = syntax_node.end as usize;
    let lexed = file.cached(lex);
    let before = lexed.0.partition_point(|token| token.end as usize <= start).checked_sub(1).and_then(|i| lexed.0.get(i));
    let after = lexed.0.get(lexed.0.partition_point(|token| (token.start as usize) < end));
    Value::record([
        ("before".into(), edge(src, before, before.map_or(start, |token| token.end as usize), start)),
        ("after".into(), edge(src, after, end, after.map_or(end, |token| token.start as usize))),
        ("parent_start".into(), file.syntax.parent(node).map_or(Value::Null, |parent| Value::from(file.syntax.tree.node(parent).start as usize))),
    ])
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use crate::engine::tests::native_linter;

    #[test]
    fn repeated_whitespace_fixes_preserve_surrounding_unicode_and_opaque_text() {
        let lint = native_linter("rule no-multi-spaces");
        let path = Path::new("/root/file.tsx");
        for (text, expected) in [
            ("const \u{754c} =  1;", "const \u{754c} = 1;"),
            ("foo();\u{feff}  \u{feff}bar();", "foo(); bar();"),
            ("const x = `a  b ${x +  y}`;", "const x = `a  b ${x + y}`;"),
        ] {
            let fixed = lint.fix(path, text);
            assert_eq!(fixed.text, expected);
            assert!(fixed.remaining.is_empty());
            assert_eq!(lint.fix(path, &fixed.text).applied, 0);
            assert!(lint.lint(path, text).iter().all(|diagnostic| {
                text[diagnostic.start..diagnostic.end].chars().all(|ch| ch.is_whitespace() || ch == '\u{feff}')
            }));
        }
    }

    #[test]
    fn repeated_whitespace_selection_uses_gap_parent_and_comment_boundary() {
        let lint = native_linter("
            rule no-multi-spaces
            override in multiple_space_gap let multiple_spaces_checked !(parent is ts_property) && !multiple_space_eol_comment
        ");
        let text = "const x =  {a:  1};  /* inline */  use();  // final\n";
        let fixed = lint.fix(Path::new("/root/file.ts"), text);
        assert_eq!(fixed.text, "const x = {a:  1}; /* inline */ use();  // final\n");
        assert_eq!(fixed.applied, 3);
        assert!(fixed.remaining.is_empty());
    }

    #[test]
    fn neighbours_preserve_parser_template_spans_and_comment_gaps() {
        let lint = native_linter(r#"
            in ts_identifier rule neighbour {
                when text == "probe"
                when starts_with(token_neighbours.before.text, "`left")
                when starts_with(token_neighbours.after.text, "}} right")
                when token_neighbours.before.gap == " /* before */ "
                when token_neighbours.after.gap == " /* after */ "
                error "neighbour"
            }
        "#);
        let reports = lint.lint(Path::new("/root/file.ts"), "`left ${ /* before */ probe /* after */ } right`;");
        assert_eq!(reports.len(), 1);
    }
}
