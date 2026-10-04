//! Blank-line decisions at structural boundaries.

use crate::expr::Value;
use crate::line_index::LineIndex;
use crate::native::ts::facts::NativeFile;
use crate::native::ts::kinds::{Field as F, Kind as K};
use crate::native::ts::lexer::Tok;

use super::lex_comments;

struct Lines(LineIndex);

fn lines(file: &NativeFile<'_>) -> Lines {
    Lines(LineIndex::new(file.syntax.src))
}

pub(super) fn empty_line_runs(file: &NativeFile<'_>, node: u32) -> Value<'static> {
    let syntax = &file.syntax;
    if !syntax.is(node, K::Program) {
        return Value::list(Vec::new());
    }
    let lines = file.cached(lines);
    let mut protected = vec![false; lines.0.line_count()];
    for id in 0..syntax.tree.nodes.len() as u32 {
        if matches!(syntax.kind(id), Some(K::Template | K::TemplateType)) {
            for quasi in syntax.all(id, F::Quasi) {
                let span = syntax.tree.node(quasi);
                protected[lines.0.line_of(span.start as usize)..lines.0.line_of(span.end as usize)].fill(true);
            }
        }
    }
    let mut count = lines.0.line_count();
    if lines.0.line_text(syntax.src, count - 1).is_empty() {
        count -= 1;
    }
    let mut runs = Vec::new();
    let mut first = 0;
    for (line, protected) in protected.into_iter().take(count).chain(std::iter::once(true)).enumerate() {
        let nonempty = protected || !lines.0.line_text(syntax.src, line).trim().is_empty();
        if nonempty {
            if line > first {
                runs.push(Value::record(vec![
                    ("kind".into(), Value::from(if first == 0 { "bof" } else if line == count { "eof" } else { "middle" })),
                    ("count".into(), Value::from(line - first)),
                    ("offsets".into(), Value::list((first..line).map(|i| Value::from(lines.0.line_start(i))).collect())),
                ]));
            }
            first = line + 1;
        }
    }
    Value::list(runs)
}

pub(super) fn class_member_gaps(file: &NativeFile<'_>, node: u32) -> Value<'static> {
    let syntax = &file.syntax;
    if !syntax.is(node, K::Class) {
        return Value::list(Vec::new());
    }
    let lexed = file.cached(lex_comments);
    let lines = file.cached(lines);
    let line = |at: u32| lines.0.line_of(at as usize);
    let kind = |id| if syntax.is(id, K::Method) { "method" } else if syntax.is(id, K::ClassProp) { "field" } else { "*" };
    let mut gaps = Vec::new();
    for overloads_skipped in [false, true] {
        let members: Vec<_> = syntax.all(node, F::Member).filter(|&id| !overloads_skipped || !syntax.is(id, K::Method) || syntax.get(id, F::Body).is_some()).collect();
        for pair in members.windows(2) {
            let current = syntax.tree.node(pair[0]);
            let next = syntax.tree.node(pair[1]);
            let Some(mut cur_last) = lexed.0.partition_point(|token| token.end <= current.end).checked_sub(1) else { continue };
            let mut next_first = lexed.0.partition_point(|token| token.start < next.start);
            if next_first >= lexed.0.len() {
                continue;
            }
            if lexed.0[cur_last].tok == Tok::P(";") && cur_last > 0
                && line(lexed.0[cur_last - 1].end) != line(lexed.0[cur_last].start)
                && line(lexed.0[cur_last].end) == line(lexed.0[next_first].start) {
                next_first = cur_last;
                cur_last -= 1;
            }
            let mut before = cur_last;
            while before + 1 < next_first && line(lexed.0[before + 1].start).saturating_sub(line(lexed.0[before].end)) <= 1 {
                before += 1;
            }
            let mut after = next_first;
            while after > cur_last + 1 && line(lexed.0[after].start).saturating_sub(line(lexed.0[after - 1].end)) <= 1 {
                after -= 1;
            }
            gaps.push(Value::record(vec![
                ("prev".into(), Value::from(kind(pair[0]))),
                ("next".into(), Value::from(kind(pair[1]))),
                ("offset".into(), Value::from(next.start as usize)),
                ("padded".into(), Value::Bool(line(lexed.0[after].start).saturating_sub(line(lexed.0[before].end)) > 1)),
                ("single_line".into(), Value::Bool(line(current.start) == line(lexed.0[cur_last].end))),
                ("overloads_skipped".into(), Value::Bool(overloads_skipped)),
            ]));
        }
    }
    Value::list(gaps)
}
