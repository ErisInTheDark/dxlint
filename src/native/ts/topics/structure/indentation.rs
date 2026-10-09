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
    let assignments = ["=", "+=", "-=", "*=", "/=", "%=", "**=", "<<=", ">>=", ">>>=", "|=", "^=", "&=", "&&=", "||=", "??="];
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
        for right in rights {
            let start = s.tree.node(s.unparen(right)).start as usize;
            let mut right_index = tokens.partition_point(|&(at, _)| at < start);
            if right_index == 0 || right_index >= tokens.len() { continue }
            while right_index > 1 && text(right_index - 1) == "(" { right_index -= 1; }
            let operator = s.get(node, F::Op).map_or(right_index - 1, |op| tokens.partition_point(|&(at, _)| at < s.tree.node(op).start as usize));
            if operator == 0 || tokens[operator].0 <= bounds.start as usize { continue }
            let left_row = line(tokens[operator - 1].0);
            let right_row = line(tokens[right_index].0);
            if left_row == right_row { continue }
            let first = tokens.partition_point(|&(at, _)| at < starts[left_row]);
            let last = tokens.partition_point(|&(at, _)| line(at) <= left_row).saturating_sub(1);
            let first_text = text(first);
            let last_text = text(last);
            let closes_more = last_text == ")" && tokens[first..=last].iter().map(|&(at, end)| match &s.src[at..end] { "(" => 1_i32, ")" => -1, _ => 0 }).sum::<i32>() < 0;
            let chain = closes_more && matches!(first_text, "." | "?.");
            let keyword = matches!(first_text, "break" | "case" | "catch" | "class" | "const" | "continue" | "debugger" | "default" | "delete" | "do" | "else" | "export" | "extends" | "finally" | "for" | "function" | "if" | "import" | "in" | "let" | "new" | "return" | "super" | "switch" | "throw" | "try" | "var" | "void" | "while" | "with");
            let type_alias = first_text == "type" && {
                let mut current = node;
                let mut found = false;
                while let Some(parent) = s.parent(current) {
                    if s.is(parent, K::TypeAlias) { found = true; break }
                    current = parent;
                }
                found
            };
            let before = tokens.partition_point(|&(at, _)| at < bounds.start as usize).checked_sub(1);
            let add = keyword || type_alias || matches!(last_text, ":" | "[" | "(" | "<") || assignments.contains(&last_text)
                || before.is_some_and(|index| line(tokens[index].0) == left_row
                    && (matches!(text(index), "[" | "(" | "{" | "=>" | ":") || assignments.contains(&text(index))));
            let subtract = closes_more && !matches!(first_text, "]" | ")" | "}") && !chain;
            out.push(Value::record(vec![
                ("offset".into(), Value::from(starts[right_row])),
                ("left_line".into(), Value::from(if chain { line(bounds.start as usize) } else { left_row })),
                ("right_line".into(), Value::from(right_row)),
                ("left".into(), Value::from(indent(if chain { line(bounds.start as usize) } else { left_row }))),
                ("right".into(), Value::from(indent(right_row))),
                ("adjust".into(), Value::Num(if add && !subtract { 1.0 } else if subtract && !add { -1.0 } else { 0.0 })),
            ]));
        }
    }
    OperandIndents(out)
}
