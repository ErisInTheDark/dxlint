//! @stylistic spacing, punctuation and line breaks: lexical neighbours.

use crate::expr::facts::Fact;
use crate::expr::Value;
use crate::native::ts::facts::NativeFile;
use crate::native::ts::kinds::Kind as K;
use crate::native::ts::lexer::{Tok, Token};

use crate::native::ts::tokens::{gap_has_space, lex, lex_comments};

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
        name: "multiple_space_sites",
        doc: "native program: same-line token/comment gaps {start,end,gap,display,owner,eol_comment}; opaque spans excluded, owner is the smallest node containing the last gap byte",
        get: |f, n| f.with_file(multiple_space_sites, n),
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

fn multiple_space_sites(file: &NativeFile<'_>, node: u32) -> Value<'static> {
    if !file.syntax.is(node, K::Program) {
        return Value::Null;
    }
    let syntax = &file.syntax;
    let lexed = file.cached(lex_comments);
    let mut nodes: Vec<_> = (0..syntax.tree.nodes.len() as u32).collect();
    nodes.sort_unstable_by_key(|&id| syntax.tree.node(id).start);
    let mut at = 0;
    let mut active = std::collections::BinaryHeap::new();
    let mut sites = Vec::new();
    for (index, pair) in lexed.0.windows(2).enumerate() {
        let [left, right] = pair else { unreachable!() };
        let gap = &syntax.src[left.end as usize..right.start as usize];
        if gap.contains(['\n', '\r', '\u{2028}', '\u{2029}']) || !gap.as_bytes().windows(2).any(|p| p.iter().all(|b| matches!(b, b' ' | b'\t'))) {
            continue;
        }
        let point = right.start - 1;
        while let Some(&id) = nodes.get(at).filter(|&&id| syntax.tree.node(id).start <= point) {
            let span = syntax.tree.node(id);
            active.push(std::cmp::Reverse((span.end - span.start, span.end, id)));
            at += 1;
        }
        while active.peek().is_some_and(|entry| entry.0.1 <= point) {
            active.pop();
        }
        let owner = active.peek().and_then(|entry| syntax.kind(entry.0.2)).map_or("", super::super::estree::spacing_owner);
        let comment = matches!(right.tok, Tok::P("/*" | "//"));
        let raw = &syntax.src[right.start as usize..right.end as usize];
        let display = if comment {
            let block = right.tok == Tok::P("/*");
            let body = &raw[2..raw.len() - if block { 2 } else { 0 }];
            let first = body.split('\n').next().unwrap_or("");
            let value = if body.contains('\n') || first.chars().count() > 12 {
                format!("{}...", first.chars().take(12).collect::<String>())
            } else {
                first.to_owned()
            };
            format!("{}{}{}", if block { "/*" } else { "//" }, value, if block { "*/" } else { "" })
        } else {
            raw.to_owned()
        };
        let eol_comment = comment && lexed.0.get(index + 2).is_none_or(|next| syntax.src[right.end as usize..next.start as usize].contains(['\n', '\r', '\u{2028}', '\u{2029}']));
        sites.push(Value::record(vec![
            ("start".into(), Value::from(left.end as usize)),
            ("end".into(), Value::from(right.start as usize)),
            ("gap".into(), Value::from(gap.to_owned())),
            ("display".into(), Value::from(display)),
            ("owner".into(), Value::from(owner)),
            ("eol_comment".into(), Value::Bool(eol_comment)),
        ]));
    }
    Value::list(sites)
}

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
