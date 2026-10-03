//! Structural punctuation sites, sharing the parser-aware lexical stream.

use std::collections::BTreeMap;

use crate::expr::Value;
use crate::native::ts::facts::NativeFile;
use crate::native::ts::kinds::{Field as F, Kind as K};
use crate::native::ts::lexer::Tok;

use super::{gap_has_space, lex};

pub(super) fn generic_spacing_sites(file: &NativeFile<'_>, node: u32) -> Value<'static> {
    let syntax = &file.syntax;
    let parameters: Vec<_> = syntax.all(node, F::TypeParam).collect();
    let arguments: Vec<_> = syntax.all(node, F::TypeArg).collect();
    if parameters.is_empty() && arguments.is_empty() {
        return Value::List(Vec::new());
    }
    let lexed = file.cached(lex);
    let mut sites = Vec::new();
    for group in [&parameters, &arguments].into_iter().filter(|group| !group.is_empty()) {
        let start = syntax.tree.node(group[0]).start;
        let end = syntax.tree.node(*group.last().expect("nonempty generics")).end;
        let Some(open) = lexed.0.partition_point(|token| token.end <= start).checked_sub(1).filter(|&i| lexed.0[i].tok == Tok::P("<")) else { continue };
        let from = lexed.0.partition_point(|token| token.start < end);
        let Some(close) = (from..lexed.0.len()).find(|&i| matches!(lexed.0[i].tok, Tok::P("," | ">"))).filter(|&i| lexed.0[i].tok == Tok::P(">")) else { continue };
        let kind = syntax.kind(node);
        let skip_before = matches!(kind, Some(K::CallSig | K::Arrow | K::FnType))
            || matches!(kind, Some(K::Function | K::Class)) && syntax.get(node, F::Name).is_none();
        let check_after = matches!(kind, Some(K::Call | K::New | K::TaggedTemplate | K::CallSig | K::MethodSig | K::FnType));
        let before = open.checked_sub(1).map(|i| (lexed.0[i].end, lexed.0[open].start));
        let after = lexed.0.get(close + 1).map(|next| (lexed.0[close].end, next.start));
        for (side, bounds) in [
            ("before", if skip_before { None } else { before }),
            ("after", if check_after { after } else { None }),
            ("inner", Some((lexed.0[open].end, lexed.0[open + 1].start))),
            ("inner", Some((lexed.0[close - 1].end, lexed.0[close].start))),
        ] {
            let Some((mut start, mut end)) = bounds else { continue };
            if side == "before" {
                if let Some(comment) = file.comments().iter().rev().find(|comment| start <= comment.start && comment.end <= end) {
                    start = comment.end;
                }
            } else if side == "after" {
                if let Some(comment) = file.comments().iter().find(|comment| start <= comment.start && comment.end <= end) {
                    end = comment.start;
                }
            }
            let gap = &syntax.src[start as usize..end as usize];
            if side == "inner" && gap.starts_with(['\n', '\r']) {
                continue;
            }
            sites.push(Value::Record(vec![
                ("side".into(), Value::from(side)),
                ("start".into(), Value::from(start as usize)),
                ("end".into(), Value::from(end as usize)),
                ("spaced".into(), Value::Bool(if side == "inner" { gap.chars().any(char::is_whitespace) } else { gap_has_space(gap) })),
            ]));
        }
    }
    Value::List(sites)
}

pub(super) fn semicolon_style_sites(file: &NativeFile<'_>, node: u32) -> Value<'static> {
    let syntax = &file.syntax;
    if !syntax.is(node, K::Program) {
        return Value::Null;
    }
    let lexed = file.cached(lex);
    let sites = semicolons(file, false);
    Value::List(sites.into_iter().map(|(at, (last_child, for_header))| {
        let token = lexed.0[at];
        let previous_line = at.checked_sub(1).is_some_and(|i| syntax.src[lexed.0[i].end as usize..token.start as usize].contains(['\n', '\r', '\u{2028}', '\u{2029}']));
        let next_line = lexed.0.get(at + 1).is_some_and(|next| syntax.src[token.end as usize..next.start as usize].contains(['\n', '\r', '\u{2028}', '\u{2029}']));
        Value::Record(vec![
            ("offset".into(), Value::from(token.start as usize)),
            ("previous_line".into(), Value::Bool(previous_line)),
            ("next_line".into(), Value::Bool(next_line)),
            ("last_child".into(), Value::Bool(last_child)),
            ("for_header".into(), Value::Bool(for_header)),
        ])
    }).collect())
}

pub(super) fn semicolon_spacing_sites(file: &NativeFile<'_>, node: u32) -> Value<'static> {
    let syntax = &file.syntax;
    if !syntax.is(node, K::Program) {
        return Value::Null;
    }
    let lexed = file.cached(lex);
    let mut sites = Vec::new();
    for (at, _) in semicolons(file, true) {
        let token = lexed.0[at];
        let previous = at.checked_sub(1).and_then(|i| lexed.0.get(i));
        let next = lexed.0.get(at + 1);
        let before_start = previous.map_or(token.start, |previous| previous.end) as usize;
        let after_end = next.map_or(token.end, |next| next.start) as usize;
        let before_gap = &syntax.src[before_start..token.start as usize];
        let after_gap = &syntax.src[token.end as usize..after_end];
        let first_in_line = previous.is_none() || before_gap.contains(['\n', '\r', '\u{2028}', '\u{2029}']);
        let last_in_line = next.is_none() || after_gap.contains(['\n', '\r', '\u{2028}', '\u{2029}']);
        for (side, start, end, spaced) in [
            ("before", before_start, token.start as usize, !first_in_line && gap_has_space(before_gap)),
            ("after", token.end as usize, after_end, !last_in_line && gap_has_space(after_gap)),
        ] {
            if side == "after" && (first_in_line || last_in_line || next.is_some_and(|next| matches!(next.tok, Tok::P(")" | "}")))) {
                continue;
            }
            sites.push(Value::Record(vec![
                ("side".into(), Value::from(side)),
                ("offset".into(), Value::from(if spaced { start } else { token.start as usize })),
                ("start".into(), Value::from(start)),
                ("end".into(), Value::from(end)),
                ("spaced".into(), Value::Bool(spaced)),
            ]));
        }
    }
    Value::List(sites)
}

fn semicolons(file: &NativeFile<'_>, include_signatures: bool) -> BTreeMap<usize, (bool, bool)> {
    let syntax = &file.syntax;
    let lexed = file.cached(lex);
    let mut sites = BTreeMap::new();
    for id in 0..syntax.tree.nodes.len() as u32 {
        if syntax.is(id, K::For) {
            for field in [F::Init, F::Test] {
                if let Some(child) = syntax.get(id, field) {
                    let end = syntax.tree.node(child).end;
                    let at = lexed.0.partition_point(|token| token.end < end);
                    if let Some(at) = (at..lexed.0.len()).find(|&i| lexed.0[i].tok == Tok::P(";")) {
                        sites.insert(at, (false, true));
                    }
                }
            }
            continue;
        }
        if !matches!(syntax.kind(id), Some(K::Break | K::Continue | K::Debugger | K::DoWhile | K::Export | K::ExportAssign | K::ExprStmt | K::Import | K::Return | K::Throw | K::Var | K::ClassProp))
            && (!include_signatures || !matches!(syntax.kind(id), Some(K::Function | K::Method | K::TypeAlias | K::PropSig | K::MethodSig | K::CallSig))) {
            continue;
        }
        let end = syntax.tree.node(id).end;
        let Some(mut at) = lexed.0.partition_point(|token| token.end <= end).checked_sub(1) else { continue };
        if include_signatures && matches!(syntax.kind(id), Some(K::PropSig | K::MethodSig | K::CallSig))
            && lexed.0.get(at + 1).is_some_and(|token| token.tok == Tok::P(";")) {
            at += 1;
        }
        if lexed.0[at].tok != Tok::P(";") {
            continue;
        }
        let last = syntax.parent(id).is_none_or(|parent| match syntax.kind(parent) {
            Some(K::DoWhile) => true,
            Some(K::If) if syntax.get(parent, F::Consequent) == Some(id) && syntax.get(parent, F::Alternate).is_some() => true,
            Some(K::Program | K::Block | K::StaticBlock | K::Case) => syntax.all(parent, F::Body).last() == Some(id),
            Some(K::Class) => syntax.all(parent, F::Member).last() == Some(id),
            _ => false,
        });
        sites.entry(at).or_insert((last, false));
    }
    sites
}

pub(super) fn switch_colon_sites(file: &NativeFile<'_>, node: u32) -> Value<'static> {
    let syntax = &file.syntax;
    if !syntax.is(node, K::Case) {
        return Value::Null;
    }
    let lexed = file.cached(lex);
    let case = syntax.tree.node(node);
    let from = syntax.get(node, F::Test).map_or(case.start + 7, |test| syntax.tree.node(test).end);
    let at = lexed.0.partition_point(|token| token.start < from);
    let Some(colon) = lexed.0.get(at).filter(|token| token.tok == Tok::P(":")) else {
        return Value::List(Vec::new());
    };
    let mut sites = Vec::new();
    for (side, adjacent) in [
        ("before", at.checked_sub(1).and_then(|i| lexed.0.get(i))),
        ("after", lexed.0.get(at + 1).filter(|token| token.tok != Tok::P("}"))),
    ] {
        let Some(adjacent) = adjacent else { continue };
        let (start, end) = if side == "before" {
            (adjacent.end as usize, colon.start as usize)
        } else {
            (colon.end as usize, adjacent.start as usize)
        };
        let gap = &syntax.src[start..end];
        if gap.contains(['\n', '\r', '\u{2028}', '\u{2029}']) {
            continue;
        }
        sites.push(Value::Record(vec![
            ("offset".into(), Value::from(colon.start as usize)),
            ("side".into(), Value::from(side)),
            ("start".into(), Value::from(start)),
            ("end".into(), Value::from(end)),
            ("spaced".into(), Value::Bool(gap_has_space(gap))),
        ]));
    }
    Value::List(sites)
}

pub(super) fn extra_semicolons(file: &NativeFile<'_>, node: u32) -> Value<'static> {
    let syntax = &file.syntax;
    if !syntax.is(node, K::Program) {
        return Value::Null;
    }
    let lexed = file.cached(lex);
    let mut offsets = Vec::new();
    for id in 0..syntax.tree.nodes.len() as u32 {
        match syntax.kind(id) {
            Some(K::Empty) => {
                if syntax.parent(id).is_some_and(|parent| matches!(syntax.kind(parent),
                    Some(K::For | K::ForIn | K::ForOf | K::While | K::DoWhile | K::If | K::Labeled | K::With))) {
                    continue;
                }
                offsets.push(syntax.tree.node(id).start);
            }
            Some(K::Class) => {
                let class = syntax.tree.node(id);
                let members: Vec<_> = syntax.all(id, F::Member).map(|member| syntax.tree.node(member)).collect();
                let from = lexed.0.partition_point(|token| token.start < class.start);
                for token in lexed.0[from..].iter().take_while(|token| token.end <= class.end) {
                    if token.tok == Tok::P(";") && !members.iter().any(|member| member.start <= token.start && token.end <= member.end) {
                        offsets.push(token.start);
                    }
                }
            }
            _ => {}
        }
    }
    offsets.sort_unstable();
    offsets.dedup();
    Value::List(offsets.into_iter().map(|offset| Value::from(offset as usize)).collect())
}
