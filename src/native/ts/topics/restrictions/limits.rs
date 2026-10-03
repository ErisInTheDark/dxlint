//! Counts owned by functions, implicit class functions, and files.

use super::super::super::facts::NativeFile;
use super::super::super::kinds::{Field as F, Kind as K};
use super::super::super::syntax::Syntax;
use crate::expr::Value;

fn function(kind: Option<K>) -> bool {
    matches!(kind, Some(K::Function | K::Arrow | K::Method | K::StaticBlock))
}

fn parent_expression(s: &Syntax, mut node: u32) -> Option<u32> {
    while let Some(parent) = s.parent(node) {
        if s.is(parent, K::Paren) {
            node = parent;
        } else {
            return Some(parent);
        }
    }
    None
}

fn callback(s: &Syntax, node: u32, constructors: bool) -> bool {
    parent_expression(s, node).is_some_and(|p| {
        (s.is(p, K::Call) || constructors && s.is(p, K::New)) && s.get(p, F::Callee).is_none_or(|n| s.unparen(n) != node)
    })
}

struct FileCounts {
    classes: usize,
    declarations: usize,
    top_functions: usize,
}

fn file_counts(file: &NativeFile) -> FileCounts {
    let s = &file.syntax;
    let classes: Vec<_> = (0..s.tree.nodes.len() as u32).filter(|&n| s.is(n, K::Class)).collect();
    let declarations = classes.iter().filter(|&&n| {
        s.parent(n).is_some_and(|p| matches!(s.kind(p), Some(K::Program | K::Block | K::Export)))
    }).count();
    let top_functions = (0..s.tree.nodes.len() as u32).filter(|&n| {
        matches!(s.kind(n), Some(K::Function | K::Arrow | K::Method)) && file.enclosing_function(n).is_none()
    }).count();
    FileCounts { classes: classes.len(), declarations, top_functions }
}

pub fn metrics(file: &NativeFile, node: u32) -> Value<'static> {
    let s = &file.syntax;
    let counts = file.cached(file_counts);
    if s.is(node, K::Program) {
        let offset = s.all(node, F::Body).next().map_or(0, |n| s.tree.node(n).start as usize);
        return Value::Record(vec![("classes".into(), Value::from(counts.classes)), ("class_declarations".into(), Value::from(counts.declarations)), ("offset".into(), Value::from(offset))]);
    }
    let params: Vec<_> = s.all(node, F::Param).collect();
    let this = params.first().is_some_and(|&n| s.word(n, F::Name) == Some("this"));
    let void_this = this && params.first().and_then(|&n| s.word(n, F::Type)) == Some("void");
    let mut statements = 0;
    let mut classic = 1;
    let mut modified = 1;
    let mut stack = vec![node];
    while let Some(n) = stack.pop() {
        let kind = s.kind(n);
        if n != node && function(kind) {
            continue;
        }
        if kind == Some(K::ClassProp) {
            stack.extend(s.kids(n).filter(|(f, _)| *f != Some(F::Init)).map(|(_, n)| n));
            continue;
        }
        if kind == Some(K::Block) {
            statements += s.all(n, F::Body).count();
        }
        let branch = matches!(kind, Some(K::Conditional | K::Logical | K::For | K::ForIn | K::ForOf | K::If | K::While | K::DoWhile | K::AssignPattern))
            || kind == Some(K::Param) && s.get(n, F::Init).is_some()
            || kind == Some(K::Try) && s.get(n, F::Handler).is_some()
            || kind == Some(K::Assign) && matches!(s.word(n, F::Op), Some("&&=" | "||=" | "??="))
            || matches!(kind, Some(K::Member | K::Index | K::Call)) && s.get(n, F::Optional).is_some();
        if branch {
            classic += 1;
            modified += 1;
        }
        if kind == Some(K::Case) && s.get(n, F::Test).is_some() {
            classic += 1;
        }
        if kind == Some(K::Switch) {
            modified += 1;
        }
        stack.extend(s.kids(n).map(|(_, n)| n));
    }
    let mut callbacks = usize::from(callback(s, node, false));
    let mut constructor_callbacks = usize::from(callback(s, node, true));
    let mut function_depth = 0;
    let mut at = s.parent(node);
    while let Some(n) = at {
        if function(s.kind(n)) {
            function_depth += 1;
            callbacks += usize::from(callback(s, n, false));
            constructor_callbacks += usize::from(callback(s, n, true));
        }
        at = s.parent(n);
    }
    let iife = parent_expression(s, node).is_some_and(|p| s.is(p, K::Call) && s.get(p, F::Callee).is_some_and(|n| s.unparen(n) == node));
    Value::Record(vec![
        ("params".into(), Value::from(params.len())),
        ("this".into(), Value::Bool(this)),
        ("void_this".into(), Value::Bool(void_this)),
        ("statements".into(), Value::from(statements)),
        ("classic".into(), Value::from(classic)),
        ("modified".into(), Value::from(modified)),
        ("callbacks".into(), Value::from(callbacks)),
        ("constructor_callbacks".into(), Value::from(constructor_callbacks)),
        ("callback".into(), Value::Bool(callback(s, node, false))),
        ("constructor_callback".into(), Value::Bool(callback(s, node, true))),
        ("function_depth".into(), Value::from(function_depth)),
        ("top_functions".into(), Value::from(counts.top_functions)),
        ("iife".into(), Value::Bool(iife)),
    ])
}

pub fn depth(file: &NativeFile, node: u32) -> Value<'static> {
    let s = &file.syntax;
    let mut count = 0;
    let mut at = Some(node);
    while let Some(n) = at {
        if function(s.kind(n)) {
            break;
        }
        if matches!(s.kind(n), Some(K::If | K::Switch | K::Try | K::DoWhile | K::While | K::With | K::For | K::ForIn | K::ForOf))
            && !(s.is(n, K::If) && s.field(n) == Some(F::Alternate) && s.parent(n).is_some_and(|p| s.is(p, K::If)))
        {
            count += 1;
        }
        at = s.parent(n);
    }
    Value::from(count)
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use crate::engine::tests::native_linter;

    #[test]
    fn independent_function_and_class_paths() {
        let l = native_linter("rule complexity { set max 1 }\nrule complexity:implicit { set max 1 }");
        let code = "function outer() { function inner() { if (x) y(); } class C { field = x || y; static { if (x) y(); } } }";
        let diagnostics = l.lint(Path::new("/root/file.ts"), code);
        assert_eq!(diagnostics.len(), 3, "{diagnostics:?}");
        assert!(diagnostics.iter().all(|d| d.start >= code.find("function inner").unwrap()));
        assert_eq!(l.lint(Path::new("/root/file.ts"), "class C { field = () => x || y; }").len(), 1);
    }

    #[test]
    fn statement_and_depth_owners_reset_at_functions() {
        let l = native_linter("rule max-statements { set max 2 }\nrule max-depth { set max 1 }");
        let code = "function outer() { if (x) { function inner() { if (y) z(); } } }";
        assert!(l.lint(Path::new("/root/file.ts"), code).is_empty());
        assert_eq!(l.lint(Path::new("/root/file.ts"), "function f() { if (x) { if (y) { z(); } } }").len(), 2);
    }
}
