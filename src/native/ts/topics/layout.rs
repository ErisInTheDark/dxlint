//! @stylistic spacing, punctuation and line breaks: lexical neighbours.

use std::collections::{HashMap, HashSet};

use crate::expr::facts::Fact;
use crate::expr::Value;
use crate::native::ts::facts::NativeFile;
use crate::native::ts::kinds::{Field as F, Kind as K};
use crate::native::ts::lexer::{Lexer, Tok, Token};

mod punctuation;
mod operators;
mod delimiters;
mod lists;
mod lines;
mod annotations;
mod quotes;

pub const FACTS: &[Fact] = &[
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
        name: "brace_style_sites",
        doc: "native controlled block/static/switch/class bodies: {kind,offset,same_line,single_line} open/body/close/keyword boundaries, bare statement-list blocks exempt",
        get: |f, n| f.with_file(delimiters::brace_style_sites, n),
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
        name: "comma_spacing_sites",
        doc: "native program: eligible comma sides {offset,side,start,end,spaced,line_comment}; start/end bound the whitespace-only gap; excludes array holes and trailing type-parameter commas",
        get: |f, n| f.with_file(comma_spacing_sites, n),
    },
    Fact {
        name: "switch_colon_sites",
        doc: "native switch case: colon sides {offset,side,start,end,spaced}; only adjacent tokens on the same line, gap retains comments",
        get: |f, n| f.with_file(punctuation::switch_colon_sites, n),
    },
    Fact {
        name: "extra_semicolons",
        doc: "native program: redundant statement and class-member semicolon byte offsets; excludes control-statement empty bodies",
        get: |f, n| f.with_file(punctuation::extra_semicolons, n),
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
        name: "spread_spacing",
        doc: "native rest/spread: {kind,start,end,spaced}; gap ends at the first token or comment after ...",
        get: |f, n| f.with_file(operators::spread_spacing, n),
    },
    Fact {
        name: "generic_spacing_sites",
        doc: "native generic owners: {side,start,end,spaced}; before/after boundaries plus inner bracket gaps, excludes context-owned outer boundaries",
        get: |f, n| f.with_file(punctuation::generic_spacing_sites, n),
    },
    Fact {
        name: "star_spacing_sites",
        doc: "native generator/yield: {kind,side,offset,start,end,separated,spaced}; generator separation includes comments, yield spaced counts whitespace outside comments",
        get: |f, n| f.with_file(operators::star_spacing_sites, n),
    },
    Fact {
        name: "delimiter_spacing_sites",
        doc: "native blocks and computed properties: {side,token,offset,start,end,spaced,class_member,line_comment}; whitespace gaps beside delimiters and comments on the same line",
        get: |f, n| f.with_file(delimiters::delimiter_spacing_sites, n),
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
        name: "function_paren_spacing",
        doc: "native functions/methods/async arrows/try-catch: {kind,start,end,spaced,generic_anonymous}; gap immediately before the parameter-list opener",
        get: |f, n| f.with_file(delimiters::function_paren_spacing, n),
    },
    Fact {
        name: "block_prefix_spacing",
        doc: "native block/switch/class/interface/enum: {kind,offset,spaced}; same-line gap before the opening brace, excludes arrow/keyword/case-colon boundaries owned by other rules",
        get: |f, n| f.with_file(delimiters::block_prefix_spacing, n),
    },
    Fact {
        name: "infix_spacing_sites",
        doc: "native infix/conditional/initializer/type operators: {operator,offset,spaced,types,int32_hint}; space on both sides outside comments, int32_hint preserves terminal |0",
        get: |f, n| f.with_file(operators::infix_spacing_sites, n),
    },
    Fact {
        name: "block_padding_sites",
        doc: "native block/static/switch/class: nonempty boundary records {kind,side,offset,padded,single_line}; adjacent same-line comments belong to the brace boundary",
        get: |f, n| f.with_file(lines::block_padding_sites, n),
    },
    Fact {
        name: "class_member_gaps",
        doc: "native class: member pairs {prev,next,offset,padded,single_line,overloads_skipped}; both ordinary and overload-filtered pairs, comments attach to neighbouring members across consecutive lines",
        get: |f, n| f.with_file(lines::class_member_gaps, n),
    },
];

struct Lexed(Vec<Token>);

fn comment_contents(file: &NativeFile<'_>, node: u32) -> Value<'static> {
    if !file.syntax.is(node, K::Program) {
        return Value::List(Vec::new());
    }
    let lexed = file.cached(lex_comments);
    Value::List(lexed.0.iter().filter_map(|token| {
        let block = match token.tok {
            Tok::P("/*") => true,
            Tok::P("//") => false,
            _ => return None,
        };
        let end = token.end as usize - if block { 2 } else { 0 };
        Some(["begin", "end"].map(|side| Value::Record(vec![
            ("kind".into(), Value::from(if block { "block" } else { "line" })),
            ("body".into(), Value::from(file.syntax.src[token.start as usize + 2..end].to_owned())),
            ("offset".into(), Value::from(token.start as usize)),
            ("side".into(), Value::from(side)),
        ])))
    }).flatten().collect())
}
struct LexedComments(Vec<Token>);

fn lex_comments(file: &NativeFile<'_>) -> LexedComments {
    let lexed = file.cached(lex);
    let mut tokens = lexed.0.clone();
    tokens.extend(file.comments().iter().filter(|comment| {
        let at = lexed.0.partition_point(|token| token.end <= comment.start);
        lexed.0.get(at).is_none_or(|token| token.start >= comment.end)
    }).map(|comment| Token {
        tok: Tok::P(if comment.block { "/*" } else { "//" }),
        start: comment.start,
        end: comment.end,
        nl: false,
    }));
    tokens.sort_unstable_by_key(|token| token.start);
    LexedComments(tokens)
}

fn lex(file: &NativeFile<'_>) -> Lexed {
    let syntax = &file.syntax;
    let mut opaque = HashMap::new();
    for id in 0..syntax.tree.nodes.len() as u32 {
        if matches!(syntax.kind(id), Some(K::Regex | K::JsxText)) {
            let node = syntax.tree.node(id);
            opaque.insert(node.start, (node.end, Tok::Regex));
        } else if matches!(syntax.kind(id), Some(K::Template | K::TemplateType)) {
            for quasi in syntax.all(id, F::Quasi) {
                let node = syntax.tree.node(quasi);
                opaque.insert(node.start, (node.end, Tok::Template));
            }
        }
    }
    let mut lexer = Lexer::new(file.syntax.src);
    let mut tokens = Vec::new();
    loop {
        // JSX text can begin with whitespace or comment-looking text. Do not
        // let JavaScript trivia scanning cross its parser-owned span.
        if let Some(&(end, kind)) = opaque.get(&(lexer.pos as u32)) {
            if end as usize > lexer.pos {
                tokens.push(Token { tok: kind, start: lexer.pos as u32, end, nl: false });
                lexer.pos = end as usize;
                continue;
            }
        }
        let mut token = lexer.next_token();
        if token.tok == Tok::Eof {
            break;
        }
        // These lexical modes belong to the parser. Preserve its spans rather
        // than interpreting template text or a regex body as JavaScript.
        if let Some(&(end, kind)) = opaque.get(&token.start) {
            token.end = end;
            token.tok = kind;
            lexer.pos = end as usize;
        }
        tokens.push(token);
    }
    Lexed(tokens)
}

fn edge(src: &str, token: Option<&Token>, start: usize, end: usize) -> Value<'static> {
    let Some(token) = token else { return Value::Null };
    let token_start = token.start as usize;
    let token_end = token.end as usize;
    let gap = &src[start..end];
    let line_break = gap.contains(['\n', '\r', '\u{2028}', '\u{2029}']);
    Value::Record(vec![
        ("text".into(), Value::from(src[token_start..token_end].to_owned())),
        ("start".into(), Value::from(token_start)),
        ("end".into(), Value::from(token_end)),
        ("gap".into(), Value::from(gap.to_owned())),
        ("spaced".into(), Value::Bool(gap_has_space(gap))),
        ("same_line".into(), Value::Bool(!line_break)),
        ("line_break".into(), Value::Bool(line_break)),
    ])
}

fn gap_has_space(mut gap: &str) -> bool {
    while !gap.is_empty() {
        if let Some(comment) = gap.strip_prefix("/*") {
            gap = comment.find("*/").map_or("", |end| &comment[end + 2..]);
        } else if gap.starts_with("//") {
            gap = gap.find(['\n', '\r', '\u{2028}', '\u{2029}']).map_or("", |end| &gap[end..]);
        } else {
            let ch = gap.chars().next().expect("nonempty gap");
            if ch.is_whitespace() {
                return true;
            }
            gap = &gap[ch.len_utf8()..];
        }
    }
    false
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
    Value::Record(vec![
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

struct CommaSites(Vec<CommaSite>);

struct CommaSite {
    offset: usize,
    side: &'static str,
    start: usize,
    end: usize,
    spaced: bool,
    line_comment: bool,
}

fn comma_spacing_sites(file: &NativeFile<'_>, node: u32) -> Value<'static> {
    if !file.syntax.is(node, K::Program) {
        return Value::Null;
    }
    let sites = file.cached(comma_sites);
    Value::List(sites.0.iter().map(|site| Value::Record(vec![
        ("offset".into(), Value::from(site.offset)),
        ("side".into(), Value::from(site.side)),
        ("start".into(), Value::from(site.start)),
        ("end".into(), Value::from(site.end)),
        ("spaced".into(), Value::Bool(site.spaced)),
        ("line_comment".into(), Value::Bool(site.line_comment)),
    ])).collect())
}

fn comma_sites(file: &NativeFile<'_>) -> CommaSites {
    let syntax = &file.syntax;
    let lexed = file.cached(lex);
    let mut ignored_before = HashSet::new();
    let mut ignored_after = HashSet::new();
    for node in 0..syntax.tree.nodes.len() as u32 {
        if matches!(syntax.kind(node), Some(K::Array | K::ArrayPattern)) {
            let elements: Vec<_> = syntax.all(node, F::Element).collect();
            for (i, &element) in elements.iter().enumerate() {
                if syntax.is(element, K::Hole) {
                    let at = syntax.tree.node(element).start;
                    ignored_before.insert(at);
                    if elements.get(i + 1).is_none_or(|&next| syntax.is(next, K::Hole)) {
                        ignored_after.insert(at);
                    }
                }
            }
        }
        if let Some(last) = syntax.all(node, F::TypeParam).last() {
            let end = syntax.tree.node(last).end;
            if let Some(token) = lexed.0.get(lexed.0.partition_point(|token| token.start < end)) {
                if token.tok == Tok::P(",") {
                    ignored_before.insert(token.start);
                    ignored_after.insert(token.start);
                }
            }
        }
    }
    let commented = file.cached(lex_comments);
    let tokens = &commented.0;
    let mut sites = Vec::new();
    for (i, comma) in tokens.iter().enumerate().filter(|(_, token)| token.tok == Tok::P(",")) {
        if !ignored_before.contains(&comma.start) {
            if let Some(previous) = i.checked_sub(1).and_then(|i| tokens.get(i)).filter(|token| token.tok != Tok::P(",")) {
                let gap = &syntax.src[previous.end as usize..comma.start as usize];
                if !gap.contains(['\n', '\r', '\u{2028}', '\u{2029}']) {
                    sites.push(CommaSite { offset: comma.start as usize, side: "before", start: previous.end as usize, end: comma.start as usize, spaced: gap_has_space(gap), line_comment: false });
                }
            }
        }
        if !ignored_after.contains(&comma.start) {
            if let Some(next) = tokens.get(i + 1).filter(|token| !matches!(token.tok, Tok::P("," | ")" | "]" | "}"))) {
                let gap = &syntax.src[comma.end as usize..next.start as usize];
                if !gap.contains(['\n', '\r', '\u{2028}', '\u{2029}']) {
                    sites.push(CommaSite { offset: comma.start as usize, side: "after", start: comma.end as usize, end: next.start as usize, spaced: gap_has_space(gap), line_comment: next.tok == Tok::P("//") });
                }
            }
        }
    }
    CommaSites(sites)
}

fn token_neighbours(file: &NativeFile<'_>, node: u32) -> Value<'static> {
    let src = file.syntax.src;
    let syntax_node = file.syntax.tree.node(node);
    let start = syntax_node.start as usize;
    let end = syntax_node.end as usize;
    let lexed = file.cached(lex);
    let before = lexed.0.partition_point(|token| token.end as usize <= start).checked_sub(1).and_then(|i| lexed.0.get(i));
    let after = lexed.0.get(lexed.0.partition_point(|token| (token.start as usize) < end));
    Value::Record(vec![
        ("before".into(), edge(src, before, before.map_or(start, |token| token.end as usize), start)),
        ("after".into(), edge(src, after, end, after.map_or(end, |token| token.start as usize))),
        ("parent_start".into(), file.syntax.parent(node).map_or(Value::Null, |parent| Value::from(file.syntax.tree.node(parent).start as usize))),
    ])
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use crate::engine::tests::native_linter;
    use super::gap_has_space;

    #[test]
    fn comment_contents_are_not_spacing_between_tokens() {
        assert!(!gap_has_space("/* \n */"));
        assert!(!gap_has_space("// text"));
        assert!(gap_has_space("/* text */ "));
        assert!(gap_has_space("// text\n"));
        assert!(gap_has_space("\u{00a0}/* text */"));
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
