//! Binary-operand indentation constraints.

use crate::expr::Value;
use crate::native::ts::{facts::NativeFile, kinds::{Field as F, Kind as K}};

struct OperandIndents(Vec<Value<'static>>);

pub(super) fn operands(file: &NativeFile, node: u32) -> Value<'static> {
    if !file.syntax.is(node, K::Program) { return Value::Null }
    Value::list(file.cached(scan).0.clone())
}

fn scan(file: &NativeFile) -> OperandIndents {
    let s = &file.syntax;
    let tokens: Vec<_> = file.tokens(s.root()).into_iter().map(|text| {
        let start = text.as_ptr() as usize - s.src.as_ptr() as usize;
        (start, start + text.len())
    }).collect();
    let mut starts = vec![0];
    for (at, ch) in s.src.char_indices() {
        if ch == '\n' && at > 0 && s.src.as_bytes()[at - 1] == b'\r' {
            *starts.last_mut().expect("initial line exists") = at + 1;
        } else if matches!(ch, '\n' | '\r' | '\u{2028}' | '\u{2029}') {
            starts.push(at + ch.len_utf8());
        }
    }
    let line = |at| starts.partition_point(|&start| start <= at) - 1;
    let text = |index: usize| &s.src[tokens[index].0..tokens[index].1];
    let indent = |row: usize| {
        let end = starts.get(row + 1).copied().unwrap_or(s.src.len());
        s.src[starts[row]..end].chars().take_while(|ch| ch.is_whitespace() && !matches!(ch, '\n' | '\r' | '\u{2028}' | '\u{2029}')).collect::<String>()
    };
    let mut out = Vec::new();
    let mut stack = vec![s.root()];
    while let Some(node) = stack.pop() {
        let kids: Vec<_> = s.kids(node).map(|(_, child)| child).collect();
        stack.extend(kids.into_iter().rev());
        let rights: Vec<_> = match s.kind(node) {
            Some(K::Binary | K::Logical) => s.all(node, F::Right).collect(),
            Some(K::Union | K::Intersection) => s.all(node, F::Type).collect(),
            _ => continue,
        };
        let bounds = s.tree.node(node);
        if line(bounds.start as usize) == line(bounds.end as usize) { continue }
        let type_members = s.is(node, K::Union) || s.is(node, K::Intersection);
        for (member, right) in rights.into_iter().enumerate() {
            let start = s.tree.node(s.unparen(right)).start as usize;
            let mut right_index = tokens.partition_point(|&(at, _)| at < start);
            if right_index == 0 || right_index >= tokens.len() { continue }
            let mut climbed = false;
            while right_index > 1 && text(right_index - 1) == "(" {
                right_index -= 1;
                climbed = true;
            }
            let operator = s.get(node, F::Op).map_or(right_index - 1, |op| tokens.partition_point(|&(at, _)| at < s.tree.node(op).start as usize));
            // A first type member can follow a delimiter outside the union/intersection span.
            if operator == 0 || (tokens[operator].0 <= bounds.start as usize && !(type_members && member == 0 && !climbed)) { continue }
            let left_row = line(tokens[operator - 1].0);
            let right_row = line(tokens[right_index].0);
            if left_row == right_row { continue }
            let first = tokens.partition_point(|&(at, _)| at < starts[left_row]);
            let last = tokens.partition_point(|&(at, _)| line(at) <= left_row).saturating_sub(1);
            let first_text = text(first);
            let last_text = text(last);
            let parens = tokens[first..=last].iter().map(|&(at, end)| match &s.src[at..end] { "(" => 1_i32, ")" => -1, _ => 0 }).sum::<i32>();
            let type_keyword = first_text == "type" && {
                let mut current = node;
                let mut found = false;
                while let Some(parent) = s.parent(current) {
                    if s.is(parent, K::TypeAlias) && s.tree.node(parent).start as usize == tokens[first].0 { found = true; break }
                    current = parent;
                }
                found
            };
            let before = tokens.partition_point(|&(at, _)| at < bounds.start as usize).checked_sub(1);
            let before_text = before.filter(|&index| line(tokens[index].0) == left_row).map_or("", text);
            let root_row = line(bounds.start as usize);
            out.push(Value::record(vec![
                ("offset".into(), Value::from(starts[right_row])),
                ("left_line".into(), Value::from(left_row)),
                ("right_line".into(), Value::from(right_row)),
                ("root_line".into(), Value::from(root_row)),
                ("left".into(), Value::from(indent(left_row))),
                ("right".into(), Value::from(indent(right_row))),
                ("root".into(), Value::from(indent(root_row))),
                ("first".into(), Value::text_copy(first_text)),
                ("last".into(), Value::text_copy(last_text)),
                ("before".into(), Value::text_copy(before_text)),
                ("parens".into(), Value::Num(parens as f64)),
                ("type_keyword".into(), Value::Bool(type_keyword)),
            ]));
        }
    }
    OperandIndents(out)
}
