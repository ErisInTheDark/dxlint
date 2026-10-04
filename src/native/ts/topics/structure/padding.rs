//! Statement categories and padding in their owning body.

use crate::expr::Value;
use crate::native::ts::{facts::NativeFile, kinds::{Field as F, Kind as K}, lexer::Lexer, syntax::Syntax};
use std::collections::HashSet;

fn multiline(text: &str) -> bool {
    text.contains(['\n', '\r', '\u{2028}', '\u{2029}'])
}

fn directive(s: &Syntax, node: u32) -> bool {
    s.is(node, K::ExprStmt) && s.get(node, F::Expr).is_some_and(|expr| s.is(expr, K::String))
        && s.parent(node).is_some_and(|parent| s.is(parent, K::Program)
            || s.is(parent, K::Block) && s.parent(parent).is_some_and(|owner| matches!(s.kind(owner), Some(K::Function | K::Arrow | K::Method))))
}

fn iife(s: &Syntax, node: u32) -> bool {
    let Some(mut expr) = s.get(node, F::Expr) else { return false };
    expr = s.unparen(expr);
    if s.is(expr, K::Unary) { expr = s.get(expr, F::Arg).map_or(expr, |arg| s.unparen(arg)); }
    let Some(mut callee) = s.get(expr, F::Callee).map(|callee| s.unparen(callee)) else { return false };
    while s.is(callee, K::Sequence) {
        let Some(last) = s.all(callee, F::Expr).last() else { break };
        callee = s.unparen(last);
    }
    matches!(s.kind(callee), Some(K::Function | K::Arrow))
}

struct BlockEnds(HashSet<u32>);

fn block_like(file: &NativeFile, node: u32) -> bool {
    let s = &file.syntax;
    if s.is(node, K::DoWhile) && s.get(node, F::Body).is_some_and(|body| s.is(body, K::Block)) || iife(s, node) {
        return true;
    }
    let end = s.tree.node(node).start as usize + s.text(node).trim_end().trim_end_matches(';').trim_end().len();
    end > s.tree.node(node).start as usize && s.src.as_bytes()[end - 1] == b'}' && file.cached(|file| BlockEnds(
        (0..file.syntax.tree.nodes.len() as u32).filter(|&inner| matches!(file.syntax.kind(inner), Some(K::Block | K::Switch)))
            .map(|inner| file.syntax.tree.node(inner).end).collect()
    )).0.contains(&(end as u32))
}

fn declaration_type(s: &Syntax, node: u32) -> &'static str {
    match s.kind(node) {
        Some(K::Function) if s.get(node, F::Body).is_none() => "TSDeclareFunction",
        Some(K::Function) => "FunctionDeclaration",
        Some(K::Class) => "ClassDeclaration",
        Some(K::Var) => "VariableDeclaration",
        Some(K::Interface) => "TSInterfaceDeclaration",
        Some(K::TypeAlias) => "TSTypeAliasDeclaration",
        Some(K::Enum) => "TSEnumDeclaration",
        _ => "",
    }
}

fn categories(file: &NativeFile, mut node: u32) -> Value<'static> {
    let s = &file.syntax;
    while s.is(node, K::Labeled) {
        let Some(body) = s.get(node, F::Body) else { break };
        node = body;
    }
    let source = s.text(node);
    let token = Lexer::new(source).next_token();
    let keyword = &source[token.start as usize..token.end as usize];
    let multi = multiline(s.text(node));
    let mut tags = vec!["*".to_owned()];
    let mut selectors = Vec::new();
    let mut add = |tag: &str, sized: bool| {
        tags.push(tag.to_owned());
        if sized { tags.push(format!("{}{tag}", if multi { "multiline-" } else { "singleline-" })); }
    };
    let mut node_type = declaration_type(s, node);
    match s.kind(node) {
        Some(K::Block) => { add("block", false); node_type = "BlockStatement"; }
        Some(K::Empty) => { add("empty", false); node_type = "EmptyStatement"; }
        Some(K::Function) => add(if s.get(node, F::Body).is_some() { "function" } else { "function-overload" }, false),
        Some(K::MethodSig) => { add("ts-method", false); node_type = "TSMethodSignature"; }
        Some(K::Var) => {
            if matches!(keyword, "var" | "let" | "const") { add(keyword, true); }
            if s.word(node, F::Kind).is_some_and(|kind| matches!(kind, "using" | "await using")) { add("using", true); }
            if let Some(init) = s.all(node, F::Decl).next().and_then(|decl| s.get(decl, F::Init)) {
                let mut call = s.unparen(init);
                while matches!(s.kind(call), Some(K::Member | K::Index)) {
                    let Some(object) = s.get(call, F::Object) else { break };
                    call = s.unparen(object);
                }
                if s.get(call, F::Callee).is_some_and(|callee| s.is(callee, K::Ident) && s.text(callee) == "require") { add("require", false); }
                if s.text(init).starts_with("require(") { add("cjs-import", false); }
            }
        }
        Some(K::ExprStmt) => {
            node_type = "ExpressionStatement";
            let prologue = directive(s, node) && s.parent(node).is_some_and(|parent| s.all(parent, F::Body).take_while(|&sibling| sibling != node).all(|sibling| directive(s, sibling)));
            if prologue { add("directive", false); } else { add("expression", true); }
            if iife(s, node) { add("iife", false); }
            if let Some(expr) = s.get(node, F::Expr).map(|expr| s.unparen(expr)) {
                if let Some(callee) = s.get(expr, F::Callee).filter(|&callee| s.is(callee, K::Ident)) {
                    selectors.push(format!("ExpressionStatement[expression.callee.name={}]", s.text(callee)));
                }
                if s.is(expr, K::Assign) {
                    if let Some(left) = s.get(expr, F::Left) {
                        let raw = s.text(left);
                        let compact: String = raw.chars().filter(|ch| !ch.is_whitespace()).collect();
                        let export_text = compact.strip_prefix("module.").unwrap_or(&compact);
                        if export_text.strip_prefix("exports").is_some_and(|tail| tail.is_empty() || tail.starts_with(['.', '['])) { add("cjs-export", false); }
                        let mut member = s.unparen(left);
                        while matches!(s.kind(member), Some(K::Member | K::Index)) {
                            let Some(object) = s.get(member, F::Object) else { break };
                            if s.is(object, K::Ident) {
                                if s.text(object) == "exports" || s.text(object) == "module" && s.word(member, F::Property) == Some("exports") { add("exports", false); }
                                break;
                            }
                            member = s.unparen(object);
                        }
                    }
                }
            }
        }
        Some(K::Export) => {
            if keyword == "export" { add("export", true); }
            node_type = if s.has_word(node, F::Kind, "default") { "ExportDefaultDeclaration" }
                else if s.has_word(node, F::Kind, "*") { "ExportAllDeclaration" } else { "ExportNamedDeclaration" };
            if let Some(decl) = s.get(node, F::Decl) { selectors.push(format!("{node_type}[declaration.type={}]", declaration_type(s, decl))); }
        }
        Some(K::TypeAlias) if keyword == "type" => add("type", true),
        Some(K::Return) => { if keyword == "return" { add("return", true); } node_type = "ReturnStatement"; }
        _ => {}
    }
    let keyword_tag = match s.kind(node) {
        Some(K::Break) => Some(("break", "BreakStatement")),
        Some(K::Case) => Some((if s.get(node, F::Test).is_some() { "case" } else { "default" }, "SwitchCase")),
        Some(K::Class) => Some(("class", "ClassDeclaration")),
        Some(K::Continue) => Some(("continue", "ContinueStatement")),
        Some(K::Debugger) => Some(("debugger", "DebuggerStatement")),
        Some(K::DoWhile) => Some(("do", "DoWhileStatement")),
        Some(K::For | K::ForIn | K::ForOf) => Some(("for", "ForStatement")),
        Some(K::If) => Some(("if", "IfStatement")),
        Some(K::Import) => Some(("import", "ImportDeclaration")),
        Some(K::Switch) => Some(("switch", "SwitchStatement")),
        Some(K::Throw) => Some(("throw", "ThrowStatement")),
        Some(K::Try) => Some(("try", "TryStatement")),
        Some(K::While) => Some(("while", "WhileStatement")),
        Some(K::With) => Some(("with", "WithStatement")),
        Some(K::Enum) => Some(("enum", "TSEnumDeclaration")),
        Some(K::Interface) => Some(("interface", "TSInterfaceDeclaration")),
        _ => None,
    };
    if let Some((tag, kind)) = keyword_tag {
        if keyword == tag { add(tag, false); }
        node_type = kind;
    }
    if block_like(file, node) { add("block-like", true); }
    if s.is(node, K::Function) {
        if let Some(name) = s.get(node, F::Name) { selectors.push(format!("{node_type}[id.name={}]", s.text(name))); }
    }
    selectors.push(node_type.to_owned());
    Value::record(vec![
        ("tags".into(), Value::list(tags.into_iter().map(Value::from).collect())),
        ("selectors".into(), Value::list(selectors.into_iter().map(Value::from).collect())),
        ("multiline".into(), Value::Bool(multi)),
    ])
}

struct PaddingSites(Vec<Value<'static>>);

pub(super) fn sites(file: &NativeFile, node: u32) -> Value<'static> {
    if !file.syntax.is(node, K::Program) { return Value::Null }
    Value::list(file.cached(scan).0.clone())
}

fn scan(file: &NativeFile) -> PaddingSites {
    let s = &file.syntax;
    let mut out = Vec::new();
    for owner in 0..s.tree.nodes.len() as u32 {
        let field = match s.kind(owner) {
            Some(K::Program | K::Block | K::StaticBlock | K::Case) => F::Body,
            Some(K::Switch) => F::Case,
            Some(K::Interface | K::TypeLit) => F::Member,
            _ => continue,
        };
        let items: Vec<_> = s.all(owner, field).filter(|&id| s.kind(id).is_some_and(|kind| !matches!(kind, K::PropSig | K::IndexSig))).collect();
        for pair in items.windows(2) {
            let previous = s.tree.node(pair[0]);
            let next = s.tree.node(pair[1]);
            let mut end = previous.end as usize;
            let start = next.start as usize;
            if end > start { continue }
            if s.src[..end].ends_with(';') {
                let before = s.src[previous.start as usize..end - 1].trim_end();
                let actual = previous.start as usize + before.len();
                if multiline(&s.src[actual..end - 1]) && !multiline(&s.src[end - 1..start]) { end = actual; }
            }
            let mut cursor = end;
            let mut padded = false;
            for comment in file.comments().iter().filter(|comment| end <= comment.start as usize && comment.end as usize <= start) {
                padded |= blank(&s.src[cursor..comment.start as usize]);
                cursor = comment.end as usize;
            }
            padded |= blank(&s.src[cursor..start]);
            out.push(Value::record(vec![
                ("offset".into(), Value::from(start)),
                ("prev".into(), categories(file, pair[0])),
                ("next".into(), categories(file, pair[1])),
                ("padded".into(), Value::Bool(padded)),
            ]));
        }
    }
    PaddingSites(out)
}

fn blank(text: &str) -> bool {
    let mut breaks = 0;
    let mut cr = false;
    for ch in text.chars() {
        if matches!(ch, '\n' | '\r' | '\u{2028}' | '\u{2029}') {
            if ch != '\n' || !cr { breaks += 1; }
            if breaks >= 2 { return true }
        } else if !ch.is_whitespace() { breaks = 0; }
        cr = ch == '\r';
    }
    false
}

#[cfg(test)]
mod tests {
    use crate::engine::tests::native_linter;
    use std::path::Path;

    #[test]
    fn block_like_categories_do_not_leak_into_adjacent_empty_statements() {
        let lint = native_linter("rule padding-line-between-statements { set options [{ blankLine: \"always\", prev: \"block-like\", next: \"block-like\" }] }");
        assert!(lint.lint(Path::new("/root/a.ts"), "function test() {};").is_empty());
        assert_eq!(lint.lint(Path::new("/root/a.ts"), "function test() {} function other() {}").len(), 1);
    }
}
