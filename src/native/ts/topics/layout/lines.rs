//! Blank-line decisions at structural boundaries.

use crate::expr::Value;
use crate::line_index::LineIndex;
use crate::native::ts::facts::NativeFile;
use crate::native::ts::kinds::{Field as F, Kind as K};

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
