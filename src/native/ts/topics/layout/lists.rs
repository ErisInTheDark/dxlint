//! Commas whose meaning belongs to a syntax list.

use crate::expr::Value;
use crate::native::ts::facts::NativeFile;
use crate::native::ts::kinds::{Field as F, Kind as K};
use crate::native::ts::lexer::Tok;

use super::lex;

fn list_groups(file: &NativeFile<'_>, node: u32) -> Vec<(&'static str, Vec<u32>)> {
    let syntax = &file.syntax;
    let mut groups = Vec::new();
    let main = match syntax.kind(node) {
        Some(K::Var) => Some(("VariableDeclaration", F::Decl)),
        Some(K::Array) => Some(("ArrayExpression", F::Element)),
        Some(K::ArrayPattern) => Some(("ArrayPattern", F::Element)),
        Some(K::Object) => Some(("ObjectExpression", F::Property)),
        Some(K::ObjectPattern) => Some(("ObjectPattern", F::Property)),
        Some(K::Function) => {
            let declaration = syntax.parent(node).is_some_and(|p| matches!(syntax.kind(p), Some(K::Program | K::Block | K::Export | K::Namespace)));
            Some((if declaration { if syntax.get(node, F::Body).is_none() { "TSDeclareFunction" } else { "FunctionDeclaration" } } else { "FunctionExpression" }, F::Param))
        }
        Some(K::Method) => Some((if syntax.get(node, F::Body).is_none() { "TSEmptyBodyFunctionExpression" } else { "FunctionExpression" }, F::Param)),
        Some(K::Arrow) => Some(("ArrowFunctionExpression", F::Param)),
        Some(K::FnType) => Some((if syntax.word(node, F::Kind) == Some("new") { "TSConstructorType" } else { "TSFunctionType" }, F::Param)),
        Some(K::Call) => Some(("CallExpression", F::Arg)),
        Some(K::New) => Some(("NewExpression", F::Arg)),
        Some(K::ImportCall) => Some(("ImportExpression", F::Arg)),
        Some(K::Import) => Some(("ImportDeclaration", F::Specifier)),
        Some(K::Export) => Some(("ExportNamedDeclaration", F::Specifier)),
        Some(K::Sequence) => Some(("SequenceExpression", F::Expr)),
        Some(K::Enum) => Some(("TSEnumBody", F::Member)),
        Some(K::TupleType) => Some(("TSTupleType", F::Element)),
        Some(K::TypeLit) => Some(("TSTypeLiteral", F::Member)),
        Some(K::MethodSig) => Some(("TSMethodSignature", F::Param)),
        Some(K::CallSig) => Some((if syntax.word(node, F::Kind) == Some("new") { "TSConstructSignatureDeclaration" } else { "TSCallSignatureDeclaration" }, F::Param)),
        Some(K::IndexSig) => Some(("TSIndexSignature", F::Param)),
        _ => None,
    };
    if let Some((owner, field)) = main {
        let mut items: Vec<_> = syntax.all(node, field).collect();
        if syntax.is(node, K::ObjectPattern) {
            items.extend(syntax.all(node, F::Rest));
            items.sort_unstable_by_key(|&id| syntax.tree.node(id).start);
        } else if syntax.is(node, K::Import) {
            items.extend(syntax.all(node, F::DefaultImport));
            items.extend(syntax.all(node, F::Namespace));
            items.sort_unstable_by_key(|&id| syntax.tree.node(id).start);
        }
        groups.push((owner, items));
    }
    if syntax.is(node, K::Class) {
        groups.push(("ClassDeclaration", syntax.all(node, F::Implements).collect()));
    } else if syntax.is(node, K::Interface) {
        groups.push(("TSInterfaceDeclaration", syntax.all(node, F::Extends).collect()));
        groups.push(("TSInterfaceBody", syntax.all(node, F::Member).collect()));
    }
    for (owner, field) in [("TSTypeParameterDeclaration", F::TypeParam), ("TSTypeParameterInstantiation", F::TypeArg)] {
        if !syntax.is(node, K::Mapped) {
            groups.push((owner, syntax.all(node, field).collect()));
        }
    }
    groups
}

pub(super) fn comma_line_sites(file: &NativeFile<'_>, node: u32) -> Value<'static> {
    let syntax = &file.syntax;
    let lexed = file.cached(lex);
    let tokens = &lexed.0;
    let mut sites = Vec::new();
    for (owner, items) in list_groups(file, node) {
        let defined: Vec<_> = items.iter().copied().filter(|&id| !syntax.is(id, K::Hole)).collect();
        let mut commas = std::collections::BTreeSet::new();
        let span = syntax.tree.node(node);
        if matches!(owner, "ImportDeclaration" | "ExportNamedDeclaration") {
            // The parser currently omits import attributes from the tree.
            // Their values are literals, so this bounded lexical list is flat.
            if let Some(from) = syntax.get(node, F::From) {
                let at = tokens.partition_point(|token| token.start < syntax.tree.node(from).end);
                if tokens.get(at).is_some_and(|token| matches!(token.tok, Tok::Ident) && matches!(&syntax.src[token.start as usize..token.end as usize], "with" | "assert")) && tokens.get(at + 1).is_some_and(|token| token.tok == Tok::P("{")) {
                    commas.extend((at + 2..tokens.len()).take_while(|&i| tokens[i].end <= span.end && tokens[i].tok != Tok::P("}")).filter(|&i| tokens[i].tok == Tok::P(",")));
                }
            }
        }
        if let Some(&first) = defined.first() {
            let mut at = tokens.partition_point(|token| token.start < syntax.tree.node(first).start);
            while let Some(previous) = at.checked_sub(1).filter(|&i| tokens[i].start >= span.start && matches!(tokens[i].tok, Tok::P("," | "("))) {
                at = previous;
                if tokens[at].tok == Tok::P(",") { commas.insert(at); }
            }
            for pair in defined.windows(2) {
                let from = syntax.tree.node(pair[0]).end;
                let to = syntax.tree.node(pair[1]).start;
                let begin = tokens.partition_point(|token| token.start < from);
                commas.extend((begin..tokens.len()).take_while(|&i| tokens[i].start < to).filter(|&i| tokens[i].tok == Tok::P(",")));
            }
            for &item in &defined {
                if let Some(last) = tokens.partition_point(|token| token.start < syntax.tree.node(item).end).checked_sub(1).filter(|&i| tokens[i].tok == Tok::P(",")) { commas.insert(last); }
            }
            let last = *defined.last().expect("nonempty items");
            let mut at = tokens.partition_point(|token| token.start < syntax.tree.node(last).end);
            while tokens.get(at).is_some_and(|token| token.end <= span.end && matches!(token.tok, Tok::P("," | ")"))) {
                if tokens[at].tok == Tok::P(",") { commas.insert(at); }
                at += 1;
            }
        } else if !items.is_empty() {
            let begin = tokens.partition_point(|token| token.start < span.start);
            commas.extend((begin..tokens.len()).take_while(|&i| tokens[i].end <= span.end).filter(|&i| tokens[i].tok == Tok::P(",")));
        }
        for at in commas {
            let Some(before) = at.checked_sub(1).and_then(|i| tokens.get(i)) else { continue };
            let Some(after) = tokens.get(at + 1) else { continue };
            let comma = tokens[at];
            let after_break = syntax.src[comma.end as usize..after.start as usize].contains(['\n', '\r', '\u{2028}', '\u{2029}']);
            if before.tok == Tok::P("[") || before.tok == Tok::P(",") && tokens[..at].iter().rev().find(|token| token.tok != Tok::P(",")).is_some_and(|token| token.tok == Tok::P("[")) || after.tok == Tok::P(",") && after_break {
                continue;
            }
            sites.push(Value::record(vec![
                ("owner".into(), Value::from(owner)),
                ("offset".into(), Value::from(comma.start as usize)),
                ("before_break".into(), Value::Bool(syntax.src[before.end as usize..comma.start as usize].contains(['\n', '\r', '\u{2028}', '\u{2029}']))),
                ("after_break".into(), Value::Bool(after_break)),
            ]));
        }
    }
    Value::list(sites)
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
    let groups = list_groups(file, node).into_iter().filter_map(|(owner, items)| {
        let kind = match owner {
            "ArrayExpression" | "ArrayPattern" => "arrays",
            "ObjectExpression" | "ObjectPattern" => "objects",
            "FunctionDeclaration" | "FunctionExpression" | "TSDeclareFunction" | "TSEmptyBodyFunctionExpression" | "ArrowFunctionExpression" | "TSFunctionType" | "TSConstructorType" | "CallExpression" | "NewExpression" => "functions",
            "ImportExpression" => "dynamicImports",
            "ImportDeclaration" => "imports",
            "ExportNamedDeclaration" => "exports",
            "TSEnumBody" => "enums",
            "TSTupleType" => "tuples",
            "TSTypeParameterDeclaration" => "generics",
            "TSTypeParameterInstantiation" => "type-arguments",
            _ => return None,
        };
        Some((kind, items))
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
