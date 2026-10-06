//! Commas whose meaning belongs to a syntax list.

use crate::expr::Value;
use crate::fx_hash::FxHashMap;
use crate::native::ts::facts::NativeFile;
use crate::native::ts::kinds::{Field as F, Kind as K};
use crate::native::ts::lexer::{Tok, Token};
use std::cell::OnceCell;
use std::ops::Range;

use super::lex;

fn list_groups(file: &NativeFile<'_>, node: u32) -> Vec<(F, Vec<u32>)> {
    let syntax = &file.syntax;
    let mut groups = Vec::new();
    let Some(kind) = syntax.kind(node) else { return groups };
    let main = match kind {
        K::Var => Some(F::Decl),
        K::Array | K::ArrayPattern | K::TupleType => Some(F::Element),
        K::Object | K::ObjectPattern => Some(F::Property),
        K::Function | K::Method | K::Arrow | K::FnType | K::MethodSig | K::CallSig | K::IndexSig => Some(F::Param),
        K::Call | K::New | K::ImportCall => Some(F::Arg),
        K::Import | K::Export => Some(F::Specifier),
        K::Sequence => Some(F::Expr),
        K::Enum | K::TypeLit => Some(F::Member),
        _ => None,
    };
    if let Some(field) = main {
        let mut items: Vec<_> = syntax.all(node, field).collect();
        if syntax.is(node, K::ObjectPattern) {
            items.extend(syntax.all(node, F::Rest));
            items.sort_unstable_by_key(|&id| syntax.tree.node(id).start);
        } else if syntax.is(node, K::Import) {
            items.extend(syntax.all(node, F::DefaultImport));
            items.extend(syntax.all(node, F::Namespace));
            items.sort_unstable_by_key(|&id| syntax.tree.node(id).start);
        }
        if !items.is_empty() { groups.push((field, items)); }
    }
    if syntax.is(node, K::Class) {
        let items: Vec<_> = syntax.all(node, F::Implements).collect();
        if !items.is_empty() { groups.push((F::Implements, items)); }
    } else if syntax.is(node, K::Interface) {
        for field in [F::Extends, F::Member] {
            let items: Vec<_> = syntax.all(node, field).collect();
            if !items.is_empty() { groups.push((field, items)); }
        }
    }
    if kind != K::Mapped {
        for field in [F::TypeParam, F::TypeArg] {
            if crate::native::ts::kinds::KINDS[kind as usize].1.contains(&field) {
                let items: Vec<_> = syntax.all(node, field).collect();
                if !items.is_empty() { groups.push((field, items)); }
            }
        }
    }
    groups
}

struct NativeList {
    owner: u32,
    field: F,
    items: Vec<u32>,
}

#[derive(Clone, Copy)]
struct CommaContext {
    owner: u32,
    field: F,
    previous: Option<u32>,
}

struct SyntaxLists {
    lists: Vec<NativeList>,
    owners: FxHashMap<u32, Range<usize>>,
    // Trailing-comma queries need list items, not the separator index.
    commas: OnceCell<FxHashMap<u32, CommaContext>>,
}

fn syntax_lists(file: &NativeFile<'_>) -> SyntaxLists {
    let mut lists = Vec::new();
    let mut owners = FxHashMap::default();
    for owner in 0..file.syntax.tree.nodes.len() as u32 {
        let begin = lists.len();
        lists.extend(list_groups(file, owner).into_iter().map(|(field, items)| NativeList { owner, field, items }));
        if lists.len() != begin { owners.insert(owner, begin..lists.len()); }
    }
    SyntaxLists { lists, owners, commas: OnceCell::new() }
}

/// Separators of one parser list, not commas inside its items.
fn separators(file: &NativeFile<'_>, list: &NativeList, tokens: &[Token]) -> Vec<usize> {
    let syntax = &file.syntax;
    let defined: Vec<_> = list.items.iter().copied().filter(|&id| !syntax.is(id, K::Hole)).collect();
    let mut commas = Vec::new();
    let span = syntax.tree.node(list.owner);
    if let Some(&first) = defined.first() {
        let mut at = tokens.partition_point(|token| token.start < syntax.tree.node(first).start);
        while let Some(previous) = at.checked_sub(1).filter(|&i| tokens[i].start >= span.start && matches!(tokens[i].tok, Tok::P("," | "("))) {
            at = previous;
            if tokens[at].tok == Tok::P(",") { commas.push(at); }
        }
        for pair in defined.windows(2) {
            let from = syntax.tree.node(pair[0]).end;
            let to = syntax.tree.node(pair[1]).start;
            let begin = tokens.partition_point(|token| token.start < from);
            commas.extend((begin..tokens.len()).take_while(|&i| tokens[i].start < to).filter(|&i| tokens[i].tok == Tok::P(",")));
        }
        for &item in &defined {
            if let Some(last) = tokens.partition_point(|token| token.start < syntax.tree.node(item).end).checked_sub(1).filter(|&i| tokens[i].tok == Tok::P(",")) { commas.push(last); }
        }
        let last = *defined.last().expect("nonempty items");
        let mut at = tokens.partition_point(|token| token.start < syntax.tree.node(last).end);
        while tokens.get(at).is_some_and(|token| token.end <= span.end && matches!(token.tok, Tok::P("," | ")"))) {
            if tokens[at].tok == Tok::P(",") { commas.push(at); }
            at += 1;
        }
    } else {
        let begin = tokens.partition_point(|token| token.start < span.start);
        commas.extend((begin..tokens.len()).take_while(|&i| tokens[i].end <= span.end).filter(|&i| tokens[i].tok == Tok::P(",")));
    }
    commas.sort_unstable();
    commas.dedup();
    commas
}

fn comma_context(file: &NativeFile<'_>, node: u32) -> Option<CommaContext> {
    if !file.syntax.is(node, K::Comma) { return None; }
    let lists = file.cached(syntax_lists);
    let contexts = lists.commas.get_or_init(|| {
        let tokens = file.cached(lex);
        let mut contexts = FxHashMap::default();
        for list in &lists.lists {
            let mut at = 0;
            let mut previous = None;
            for comma in separators(file, list, &tokens.0) {
                let offset = tokens.0[comma].start;
                while let Some(&item) = list.items.get(at).filter(|&&item| file.syntax.tree.node(item).start < offset) {
                    if !file.syntax.is(item, K::Hole) { previous = Some(item); }
                    at += 1;
                }
                let context = CommaContext { owner: list.owner, field: list.field, previous };
                let replaced = contexts.insert(offset, context);
                debug_assert!(replaced.is_none(), "comma belongs to two native lists");
            }
        }
        contexts
    });
    contexts.get(&file.syntax.tree.node(node).start).copied()
}

pub(super) fn list_field(file: &NativeFile<'_>, node: u32) -> Value<'static> {
    comma_context(file, node).map_or(Value::Null, |context| Value::from(crate::native::ts::kinds::FIELD_NAMES[context.field as usize]))
}

pub(super) fn list_owner(file: &NativeFile<'_>, node: u32) -> Vec<u32> {
    comma_context(file, node).map(|context| context.owner).into_iter().collect()
}

pub(super) fn list_previous(file: &NativeFile<'_>, node: u32) -> Vec<u32> {
    comma_context(file, node).and_then(|context| context.previous).into_iter().collect()
}

fn assignment_pattern(file: &NativeFile<'_>, mut node: u32) -> bool {
    let syntax = &file.syntax;
    while let Some(parent) = syntax.parent(node) {
        match syntax.kind(parent) {
            Some(K::Assign | K::ForIn | K::ForOf) => return syntax.get(parent, F::Left) == Some(node),
            Some(K::Array | K::Object | K::Paren) => node = parent,
            Some(K::Prop) if syntax.get(parent, F::Value) == Some(node) => node = parent,
            _ => return false,
        }
    }
    false
}

pub(super) fn trailing_comma_sites(file: &NativeFile<'_>, node: u32) -> Value<'static> {
    let syntax = &file.syntax;
    let lists = file.cached(syntax_lists);
    let Some(range) = lists.owners.get(&node) else { return Value::list(Vec::new()) };
    let groups = lists.lists[range.clone()].iter().filter_map(|list| {
        let kind = match (list.field, syntax.kind(node)) {
            (F::TypeParam, _) => "generics",
            (F::TypeArg, _) => "type-arguments",
            (_, Some(K::Array | K::ArrayPattern)) => "arrays",
            (_, Some(K::Object | K::ObjectPattern)) => "objects",
            (_, Some(K::Function | K::Method | K::Arrow | K::FnType | K::Call | K::New)) => "functions",
            (_, Some(K::ImportCall)) => "dynamicImports",
            (_, Some(K::Import)) => "imports",
            (_, Some(K::Export)) => "exports",
            (_, Some(K::Enum)) => "enums",
            (_, Some(K::TupleType)) => "tuples",
            _ => return None,
        };
        Some((kind, &list.items))
    });
    let lexed = file.cached(lex);
    let mut sites = Vec::new();
    for (kind, items) in groups {
        let Some(&last) = items.last() else { continue };
        if syntax.is(last, K::Hole) {
            continue;
        }
        let end = syntax.tree.node(last).end;
        let at = lexed.0.partition_point(|token| token.start < end);
        let Some(next) = lexed.0.get(at) else { continue };
        let trailing = next.tok == Tok::P(",");
        let (token, close) = if trailing {
            let Some(close) = lexed.0.get(at + 1) else { continue };
            (*next, *close)
        } else {
            let Some(previous) = at.checked_sub(1).and_then(|i| lexed.0.get(i)) else { continue };
            (*previous, *next)
        };
        if !matches!(close.tok, Tok::P("}" | "]" | ")" | ">")) {
            continue;
        }
        let allowed = !(syntax.is(last, K::Rest)
            || syntax.is(last, K::Spread) && assignment_pattern(file, node)
            || syntax.get(last, F::Name).is_some_and(|name| syntax.is(name, K::Rest)));
        let offset = if trailing { token.start } else { token.end } as usize;
        let multiline = syntax.src[token.end as usize..close.end as usize].contains(['\n', '\r', '\u{2028}', '\u{2029}']);
        let tsx_single = kind == "generics" && items.len() == 1 && file.path.extension().is_some_and(|ext| ext == "tsx");
        sites.push(Value::record(vec![
            ("kind".into(), Value::from(kind)),
            ("offset".into(), Value::from(offset)),
            ("trailing".into(), Value::Bool(trailing)),
            ("multiline".into(), Value::Bool(multiline)),
            ("allowed".into(), Value::Bool(allowed)),
            ("tsx_single".into(), Value::Bool(tsx_single)),
        ]));
    }
    Value::list(sites)
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use crate::engine::tests::native_linter;

    #[test]
    fn native_list_context_separates_fields_and_skips_leading_holes() {
        let lint = native_linter(r#"
            in ts_comma rule context {
                when list_owner is ts_call
                warn "{list_field}:{list_previous.text}"
            }
            in ts_comma rule holes {
                when list_owner is ts_array
                warn if list_previous == null then "absent" else list_previous.text
            }
        "#);
        let reports = lint.lint(Path::new("/root/file.ts"), "f<A, B>(first, second); const a = [,,a,,b,];");
        assert_eq!(reports.iter().filter(|r| r.rule == "context").map(|r| r.message.as_str()).collect::<Vec<_>>(),
            ["type_argument:A", "argument:first"]);
        assert_eq!(reports.iter().filter(|r| r.rule == "holes").map(|r| r.message.as_str()).collect::<Vec<_>>(),
            ["absent", "absent", "a", "a", "b"]);
    }

    #[test]
    fn comma_style_selection_distinguishes_lists_of_one_host() {
        let lint = native_linter(r#"
            rule comma-style { set style "last" }
            override in ts_comma, ts_import_attribute_comma let comma_style_checked comma_style_owner is comma_style_lists && comma_style_field != "type_argument"
        "#);
        let source = "f<A\n,B>(first\n,second);";
        let reports = lint.lint(Path::new("/root/file.ts"), source);
        assert_eq!(reports.len(), 1);
        assert_eq!(&source[reports[0].start..reports[0].end], ",");
        assert_eq!(reports[0].start, source.rfind(',').unwrap());
    }

    #[test]
    fn comma_style_attribute_commas_report_once_and_select_independently() {
        let source = "import {a,\nb} from 'x' with {type: 'json',\nmode: 'strict'};";
        let all = native_linter("rule comma-style { set style 'first' }");
        let reports = all.lint(Path::new("/root/file.ts"), source);
        assert_eq!(reports.len(), 2);
        assert!(reports.iter().all(|report| &source[report.start..report.end] == ","));
        let selected = native_linter("rule comma-style { set style 'first' }\ndrop group comma_style_lists ts_import_attributes");
        let reports = selected.lint(Path::new("/root/file.ts"), source);
        assert_eq!(reports.len(), 1);
        assert_eq!(reports[0].start, source.find(',').unwrap());
    }

    #[test]
    fn trailing_comma_queries_keep_argument_and_type_argument_lists_separate() {
        let lint = native_linter(r#"
            in ts_call rule lists {
                each site in trailing_comma_sites
                warn site.kind
            }
        "#);
        let reports = lint.lint(Path::new("/root/file.ts"), "f<A,B>(first,second);");
        assert_eq!(reports.iter().map(|report| report.message.as_str()).collect::<Vec<_>>(),
            ["functions", "type-arguments"]);
    }
}
