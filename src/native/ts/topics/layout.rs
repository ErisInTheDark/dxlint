//! @stylistic spacing, punctuation and line breaks: lexical neighbours.

use crate::expr::facts::Fact;
use crate::expr::Value;
use crate::native::ts::facts::NativeFile;
use crate::native::ts::kinds::{Field as F, Kind as K};
use crate::native::ts::lexer::{Tok, Token};

use crate::native::ts::tokens::{gap_has_space, lex, lex_comments};

mod punctuation;
mod operators;
mod delimiters;
mod lists;
mod lines;
mod annotations;
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
        name: "string_quote_context",
        doc: "native string/template: {jsx,non_backtick,template,template_feature}; grammar-safe non-backtick contexts and semantic template features (tag/interpolation/unescaped newline)",
        get: |f, n| f.with_file(quotes::string_quote_context, n),
    },
    Fact {
        name: "property_quote_sites",
        doc: "native properties/signatures/enums and object/interface/type/enum groups: {group,quoted,numeric,redundant,redundant_number,keyword,name,offset}; computed keys, methods and shorthand object properties exempt",
        get: |f, n| f.with_file(quotes::property_quote_sites, n),
    },
    Fact {
        name: "member_delimiter_sites",
        doc: "native interface/type literal: {kind,single_line,last_same_line,last,delimiter,offset}; offset last token end including optional separator, node multiline ignores declaration header",
        get: |f, n| f.with_file(delimiters::member_delimiter_sites, n),
    },
    Fact {
        name: "comment_contents",
        doc: "native program: {kind,body,offset,side} begin/end lexical comment sites, excluding hashbang and opaque string/template/JSX text spans",
        get: |f, n| f.with_file(comment_contents, n),
    },
    Fact {
        name: "empty_line_runs",
        doc: "native program: {kind,count,offsets} runs of empty lines; kind bof/eof/middle, offsets line starts, final editor newline ignored and template quasi lines protected",
        get: |f, n| f.with_file(lines::empty_line_runs, n),
    },
    Fact {
        name: "annotation_spacing",
        doc: "native typed declarations/parameters/members/functions/tuples/mapped types: {category,optional,label,before,after,inner,outer}; gap records {spaced,offset}, optional label includes mapped +/- modifiers",
        get: |f, n| f.with_file(annotations::annotation_spacing, n),
    },
    Fact {
        name: "token_neighbours",
        doc: "native layers: adjacent tokens as before/after records {text,start,end,gap,spaced,same_line,line_break}; gap retains comments, spaced counts whitespace outside comments, line_break includes comment line breaks; parent_start is the parent's byte offset",
        get: |f, n| f.with_file(token_neighbours, n),
    },
    Fact {
        name: "call_spacing",
        doc: "native calls/new/import(): the gap before invocation parentheses {start,end,gap,spaced,line_break,optional,prefix_spaced,suffix_spaced}; null when no argument list",
        get: |f, n| f.with_file(call_spacing, n),
    },
    Fact {
        name: "semicolon_style_sites",
        doc: "native program: statement and for-header semicolons {offset,previous_line,next_line,last_child,for_header}; line flags indicate a break to the adjacent token",
        get: |f, n| f.with_file(punctuation::semicolon_style_sites, n),
    },
    Fact {
        name: "semicolon_spacing_sites",
        doc: "native program: semicolon sides {side,offset,start,end,spaced}; report offset follows ESLint whitespace-vs-token positions, excludes after line-end/closing paren/brace",
        get: |f, n| f.with_file(punctuation::semicolon_spacing_sites, n),
    },
    Fact {
        name: "unary_spacing",
        doc: "native unary/update/new/await/yield/non-null: {operator,word,prefix,start,end,separated,removable,first_bang,override_key}; separation counts comments, removable preserves token boundaries",
        get: |f, n| f.with_file(operators::unary_spacing, n),
    },
    Fact {
        name: "generic_spacing_sites",
        doc: "native generic owners: {side,start,end,spaced}; before/after boundaries plus inner bracket gaps, excludes context-owned outer boundaries",
        get: |f, n| f.with_file(punctuation::generic_spacing_sites, n),
    },
    Fact {
        name: "property_spacing",
        doc: "native member/index/qualified/import-type: {name,spaced}; gap before property includes access punctuation, comments do not count as spaces, expression members on different lines are excluded",
        get: |f, n| f.with_file(delimiters::property_spacing, n),
    },
    Fact {
        name: "trailing_comma_sites",
        doc: "native list owners: {kind,offset,trailing,multiline,allowed,tsx_single}; offset is comma or insertion point, multiline compares last item/comma with closing token",
        get: |f, n| f.with_file(lists::trailing_comma_sites, n),
    },
    Fact {
        name: "class_member_gaps",
        doc: "native class: member pairs {prev,next,offset,padded,single_line,overloads_skipped}; both ordinary and overload-filtered pairs, comments attach to neighbouring members across consecutive lines",
        get: |f, n| f.with_file(lines::class_member_gaps, n),
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
        let owner = active.peek().and_then(|entry| syntax.kind(entry.0.2)).map_or("", spacing_owner);
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

fn spacing_owner(kind: K) -> &'static str {
    match kind {
        K::Program => "Program",
        K::Prop | K::PatternProp => "Property",
        K::Var => "VariableDeclaration",
        K::Declarator => "VariableDeclarator",
        K::Array => "ArrayExpression",
        K::Object => "ObjectExpression",
        K::ArrayPattern => "ArrayPattern",
        K::ObjectPattern => "ObjectPattern",
        K::Import => "ImportDeclaration",
        K::ImportSpec => "ImportSpecifier",
        K::ClassProp => "PropertyDefinition",
        K::Method => "MethodDefinition",
        K::Binary => "BinaryExpression",
        K::Logical => "LogicalExpression",
        K::Assign => "AssignmentExpression",
        K::AssignPattern => "AssignmentPattern",
        K::Return => "ReturnStatement",
        K::ExprStmt => "ExpressionStatement",
        K::Conditional => "ConditionalExpression",
        K::Call => "CallExpression",
        K::New => "NewExpression",
        K::Block => "BlockStatement",
        K::PropSig => "TSPropertySignature",
        K::TypeLit => "TSTypeLiteral",
        K::TypeAlias => "TSTypeAliasDeclaration",
        K::Interface => "TSInterfaceDeclaration",
        K::EnumMember => "TSEnumMember",
        _ => "",
    }
}

fn comment_contents(file: &NativeFile<'_>, node: u32) -> Value<'static> {
    if !file.syntax.is(node, K::Program) {
        return Value::list(Vec::new());
    }
    let lexed = file.cached(lex_comments);
    Value::list(lexed.0.iter().filter_map(|token| {
        let block = match token.tok {
            Tok::P("/*") => true,
            Tok::P("//") => false,
            _ => return None,
        };
        let end = token.end as usize - if block { 2 } else { 0 };
        Some(["begin", "end"].map(|side| Value::record(vec![
            ("kind".into(), Value::from(if block { "block" } else { "line" })),
            ("body".into(), Value::from(file.syntax.src[token.start as usize + 2..end].to_owned())),
            ("offset".into(), Value::from(token.start as usize)),
            ("side".into(), Value::from(side)),
        ])))
    }).flatten().collect())
}
fn edge(src: &str, token: Option<&Token>, start: usize, end: usize) -> Value<'static> {
    let Some(token) = token else { return Value::Null };
    let token_start = token.start as usize;
    let token_end = token.end as usize;
    let gap = &src[start..end];
    let line_break = gap.contains(['\n', '\r', '\u{2028}', '\u{2029}']);
    Value::record(vec![
        ("text".into(), Value::from(src[token_start..token_end].to_owned())),
        ("start".into(), Value::from(token_start)),
        ("end".into(), Value::from(token_end)),
        ("gap".into(), Value::from(gap.to_owned())),
        ("spaced".into(), Value::Bool(gap_has_space(gap))),
        ("same_line".into(), Value::Bool(!line_break)),
        ("line_break".into(), Value::Bool(line_break)),
    ])
}

fn call_spacing(file: &NativeFile<'_>, node: u32) -> Value<'static> {
    let syntax = &file.syntax;
    let from = match syntax.kind(node) {
        Some(K::Call | K::New) => syntax.all(node, F::TypeArg).last().or_else(|| syntax.get(node, F::Callee)).map(|id| syntax.tree.node(id).end),
        Some(K::ImportCall) => Some(syntax.tree.node(node).start + 6),
        _ => None,
    };
    let Some(from) = from else { return Value::Null };
    let lexed = file.cached(lex);
    let limit = syntax.tree.node(node).end;
    let at = lexed.0.partition_point(|token| token.start < from);
    let Some(open) = (at..lexed.0.len()).take_while(|&i| lexed.0[i].start < limit).find(|&i| lexed.0[i].tok == Tok::P("(")) else { return Value::Null };
    let Some(mut previous) = open.checked_sub(1) else { return Value::Null };
    let optional = lexed.0[previous].tok == Tok::P("?.");
    let optional_token = lexed.0[previous];
    if optional {
        let Some(index) = previous.checked_sub(1) else { return Value::Null };
        previous = index;
    }
    let start = lexed.0[previous].end as usize;
    let end = lexed.0[open].start as usize;
    let gap = &syntax.src[start..end];
    let prefix = if optional { &syntax.src[start..optional_token.start as usize] } else { gap };
    let suffix = if optional { &syntax.src[optional_token.end as usize..end] } else { gap };
    Value::record(vec![
        ("start".into(), Value::from(start)),
        ("end".into(), Value::from(end)),
        ("gap".into(), Value::from(gap.to_owned())),
        ("spaced".into(), Value::Bool(gap_has_space(gap))),
        ("line_break".into(), Value::Bool(gap.contains(['\n', '\r', '\u{2028}', '\u{2029}']))),
        ("optional".into(), Value::Bool(optional)),
        ("prefix_spaced".into(), Value::Bool(gap_has_space(prefix))),
        ("suffix_spaced".into(), Value::Bool(gap_has_space(suffix))),
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
    Value::record(vec![
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
