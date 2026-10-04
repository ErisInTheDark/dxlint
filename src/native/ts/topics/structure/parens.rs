//! Expression precedence and grammar-required parentheses.

use crate::expr::Value;
use crate::native::ts::{facts::NativeFile, kinds::{Field as F, Kind as K}, syntax::Syntax};
use super::operators::{new_args, precedence};

fn assertion(s: &Syntax, node: u32) -> bool {
    matches!(s.kind(node), Some(K::As | K::Satisfies | K::TypeAssertion))
}

fn selector_type(s: &Syntax, node: u32) -> &'static str {
    match s.kind(node) {
        Some(K::Member | K::Index) => "MemberExpression",
        Some(K::New) => "NewExpression",
        Some(K::Call) => "CallExpression",
        Some(K::Spread) => "SpreadElement",
        Some(K::Declarator) => "VariableDeclarator",
        Some(K::Conditional) => "ConditionalExpression",
        Some(K::Binary) => "BinaryExpression",
        Some(K::Logical) => "LogicalExpression",
        Some(K::Assign) => "AssignmentExpression",
        Some(K::Await) => "AwaitExpression",
        Some(K::Arrow) => "ArrowFunctionExpression",
        Some(K::Function) => "FunctionExpression",
        Some(K::TypeAlias) => "TSTypeAliasDeclaration",
        Some(K::Union) => "TSUnionType",
        Some(K::Intersection) => "TSIntersectionType",
        Some(K::Ident) => "Identifier",
        Some(K::Object) => "ObjectExpression",
        Some(K::Array) => "ArrayExpression",
        _ => "",
    }
}

fn selectors(s: &Syntax, parent: u32) -> Value<'static> {
    let kind = selector_type(s, parent);
    let mut out = vec![Value::from(kind)];
    for (field, child) in s.kids(parent) {
        let name = match field {
            Some(F::Object) => "object",
            Some(F::Arg) => "argument",
            Some(F::Init) => "init",
            Some(F::Body) => "body",
            Some(F::Type) if s.is(parent, K::TypeAlias) => "typeAnnotation",
            _ => continue,
        };
        out.push(Value::from(format!("{kind}[{name}]")));
        let child = selector_type(s, unwrap(s, child));
        if !child.is_empty() {
            out.push(Value::from(format!("{kind}[{name}.type={child}]")));
        }
    }
    Value::list(out)
}

fn unwrap(s: &Syntax, mut node: u32) -> u32 {
    while let Some(inner) = match s.kind(node) {
        Some(K::Paren) => s.get(node, F::Expr),
        Some(K::ParenType) => s.get(node, F::Type),
        _ => None,
    } {
        node = inner;
    }
    node
}

fn optional_chain(s: &Syntax, node: u32) -> bool {
    s.get(node, F::Optional).is_some() || matches!(s.kind(node), Some(K::Member | K::Index | K::Call))
        && s.get(node, if s.is(node, K::Call) { F::Callee } else { F::Object }).is_some_and(|n| optional_chain(s, n))
}

fn has_call(s: &Syntax, node: u32) -> bool {
    s.is(node, K::Call) || s.kids(node).any(|(_, child)| has_call(s, child))
}

fn has_in(s: &Syntax, node: u32) -> bool {
    if matches!(s.kind(node), Some(K::Call | K::Array | K::Object | K::Paren | K::Function | K::Block)) {
        return false;
    }
    s.word(node, F::Op) == Some("in") || s.kids(node).any(|(_, child)| has_in(s, child))
}

fn leading_context(s: &Syntax, mut node: u32) -> Option<K> {
    while let Some(parent) = s.parent(node) {
        let field = s.field(node)?;
        let kind = s.kind(parent)?;
        if matches!(kind, K::ExprStmt | K::ExportAssign | K::Return)
            || kind == K::Arrow && field == F::Body
            || kind == K::Export && field == F::Expr
            || kind == K::For && field == F::Init
            || matches!(kind, K::ForIn | K::ForOf) && field == F::Left {
            return Some(kind);
        }
        let leading = match kind {
            K::Paren | K::Sequence | K::As | K::Satisfies | K::NonNull => field == F::Expr,
            K::Member | K::Index => field == F::Object,
            K::Call => field == F::Callee,
            K::Binary | K::Logical | K::Assign => field == F::Left,
            K::Conditional => field == F::Test,
            K::Update => field == F::Arg && s.tree.node(node).start == s.tree.node(parent).start,
            _ => false,
        };
        if !leading || kind == K::Sequence && s.all(parent, F::Expr).next() != Some(node) {
            return None;
        }
        node = parent;
    }
    None
}

fn in_for_initializer(s: &Syntax, mut node: u32) -> bool {
    while let Some(parent) = s.parent(node) {
        match s.kind(parent) {
            Some(K::For) => return s.field(node) == Some(F::Init),
            Some(K::Array | K::Object | K::Call | K::Function | K::Block | K::Template) => return false,
            _ => node = parent,
        }
    }
    false
}

fn iife(s: &Syntax, node: u32) -> bool {
    super::calls::iife_function(s, node).is_some_and(|(_, prototype)| !prototype)
}

fn function_prototype(s: &Syntax, node: u32) -> bool {
    if super::calls::iife_function(s, node).is_some_and(|(_, prototype)| prototype) {
        return true;
    }
    if s.is(node, K::Call) {
        if let Some(callee) = s.get(node, F::Callee) {
            let callee = unwrap(s, callee);
            return function_prototype(s, callee);
        }
    }
    if matches!(s.kind(node), Some(K::Member | K::Index)) {
        return s.get(node, F::Object).is_some_and(|object| function_prototype(s, unwrap(s, object)));
    }
    false
}

fn statement_hazard(s: &Syntax, mut node: u32) -> bool {
    loop {
        if matches!(s.kind(node), Some(K::Object | K::Function | K::Class)) {
            return true;
        }
        if s.is(node, K::Index) && s.get(node, F::Object).is_some_and(|object| s.text(object) == "let") {
            return true;
        }
        let field = match s.kind(node) {
            Some(K::Call | K::New) => F::Callee,
            Some(K::Member | K::Index) => F::Object,
            Some(K::Binary | K::Logical | K::Assign) => F::Left,
            Some(K::Conditional) => F::Test,
            Some(K::Sequence | K::As | K::Satisfies | K::NonNull) => F::Expr,
            _ => return false,
        };
        let Some(next) = s.get(node, field) else { return false };
        // An inner parenthesis already protects the statement's start.
        if s.is(next, K::Paren) {
            return false;
        }
        node = next;
    }
}

fn required(s: &Syntax, parent: u32, inner: u32, field: F) -> Option<usize> {
    let kind = s.kind(inner);
    let prec = precedence(s, inner);
    let assignment = usize::from(prec < 1);
    Some(match s.kind(parent)? {
        K::Binary | K::Logical => {
            let op = s.word(parent, F::Op).unwrap_or("");
            let child_op = s.word(inner, F::Op).unwrap_or("");
            let mixed_nullish = (op == "??" && matches!(child_op, "&&" | "||"))
                || (matches!(op, "&&" | "||") && child_op == "??");
            usize::from(mixed_nullish || assertion(s, inner)
                || field == F::Left && op == "**" && matches!(kind, Some(K::Unary | K::Await))
                || prec < precedence(s, parent)
                || prec == precedence(s, parent) && (field == F::Right) != (op == "**"))
        }
        K::Conditional => usize::from(assertion(s, inner) || prec < if field == F::Test { 4 } else { 1 }),
        K::Call | K::New if field == F::Callee => {
            if kind == Some(K::Function) { return Some(1); }
            usize::from(prec < 19 || optional_chain(s, inner) || s.is(parent, K::New) && (has_call(s, inner) || s.is(inner, K::ImportCall)))
        }
        K::Call | K::New | K::ImportCall => assignment,
        K::Member | K::Index if field == F::Object => {
            if kind == Some(K::Regex) { return None; }
            let integer = kind == Some(K::Number) && s.is(parent, K::Member)
                && s.text(inner).chars().all(|c| c.is_ascii_digit() || c == '_');
            usize::from(integer || assertion(s, inner) || optional_chain(s, inner) && s.get(parent, F::Optional).is_none()
                || prec < 19 || kind == Some(K::New) && !new_args(s, inner))
        }
        K::Member | K::Index => 0,
        K::Arrow => {
            if assertion(s, inner) { return None; }
            usize::from(kind == Some(K::Object) || prec < 1)
        }
        K::Declarator if field == F::Init => {
            if kind == Some(K::Regex) { return None; }
            assignment
        }
        K::Return | K::Throw | K::Yield => {
            if kind == Some(K::Regex) && s.is(parent, K::Return) { return None; }
            let start = s.tree.node(parent).start as usize;
            let end = s.tree.node(inner).start as usize;
            usize::from(s.src[start..end].contains(['\n', '\r', '\u{2028}', '\u{2029}'])
                || s.is(parent, K::Yield) && prec < 1)
        }
        K::ExprStmt | K::ExportAssign => usize::from(statement_hazard(s, inner) || iife(s, inner)),
        K::If | K::While | K::DoWhile | K::Switch | K::With => 0,
        K::For => {
            if assertion(s, inner) && field != F::Init { return None; }
            usize::from(field == F::Init && has_in(s, inner))
        }
        K::ForIn => 0,
        K::ForOf if field == F::Right => assignment,
        K::ForOf => 0,
        K::Assign if field == F::Left => {
            if !matches!(kind, Some(K::Ident | K::Member | K::Index)) { return None; }
            usize::from(kind == Some(K::Ident) && s.get(parent, F::Right).is_some_and(|rhs| {
                let rhs = unwrap(s, rhs);
                matches!(s.kind(rhs), Some(K::Function | K::Class | K::Arrow)) && s.get(rhs, F::Name).is_none()
            }))
        }
        K::Spread if assertion(s, inner) => return None,
        K::Assign | K::AssignPattern | K::Array | K::Spread | K::Prop | K::ClassProp | K::Param => assignment,
        K::Unary => usize::from(prec < 15),
        K::Await => usize::from(prec < 16),
        K::Update => usize::from(prec < 17),
        K::Computed => assignment,
        K::Sequence | K::TypeAlias | K::ParenType | K::Template => 0,
        K::TaggedTemplate if field == F::Tag => usize::from(prec < 19 || optional_chain(s, inner)),
        K::Class if field == F::Extends => usize::from(assertion(s, inner) || prec < 19),
        K::EnumMember if field == F::Init => assignment,
        K::ArrayType => usize::from(prec < 20),
        K::Union | K::Intersection => usize::from(prec < precedence(s, parent)),
        K::TypeAssertion | K::As | K::Satisfies if field == F::Expr => usize::from(prec < 15),
        _ if field == F::Type || field == F::ReturnType => 0,
        _ => return None,
    })
}

pub(super) fn excess(file: &NativeFile, node: u32) -> Value<'static> {
    let s = &file.syntax;
    let child_field = match s.kind(node) {
        Some(K::Paren) => F::Expr,
        Some(K::ParenType) => F::Type,
        _ => return Value::Null,
    };
    let Some(inner) = s.get(node, child_field) else { return Value::Null };
    if matches!(s.kind(inner), Some(K::Paren | K::ParenType)) {
        return Value::Null;
    }
    let mut outer = node;
    let mut count = 1;
    while let Some(parent) = s.parent(outer).filter(|&p| matches!(s.kind(p), Some(K::Paren | K::ParenType))) {
        outer = parent;
        count += 1;
    }
    let (Some(parent), Some(field)) = (s.parent(outer), s.field(outer)) else { return Value::Null };
    if field == F::TypeArg {
        return Value::Null;
    }
    let Some(mut minimum) = required(s, parent, inner, field) else { return Value::Null };
    let context = leading_context(s, outer);
    if iife(s, inner) || assertion(s, inner) && matches!(s.kind(parent), Some(K::For | K::ForIn | K::ForOf | K::Throw | K::Yield | K::Unary | K::Await))
        || context.is_some_and(|kind| kind != K::Return) && (statement_hazard(s, inner)
            || s.text(inner) == "let" || s.is(inner, K::Ident) && s.text(inner) == "async" && context == Some(K::ForOf))
        || in_for_initializer(s, outer) && has_in(s, inner)
        || s.is(parent, K::Update) && s.src[s.tree.node(inner).end as usize..s.tree.node(parent).end as usize].contains(['\n', '\r'])
        || s.is(parent, K::Await) && s.is(inner, K::NonNull) {
        minimum = minimum.max(1);
    }
    if count <= minimum {
        return Value::Null;
    }
    let prefix = &s.src[..s.tree.node(outer).start as usize];
    let comment = file.comments().iter().rev().find(|c| {
        c.end as usize <= prefix.len() && prefix[c.end as usize..].trim().is_empty()
    });
    let typed_inner = file.comments().iter().any(|c| c.block && c.start > s.tree.node(outer).start
        && c.end <= s.tree.node(inner).start && c.body(s.src).contains("@type"));
    if comment.is_some_and(|c| c.block && c.body(s.src).contains("@type")) && (count == 1 || typed_inner) {
        return Value::Null;
    }
    let binary = matches!(s.kind(inner), Some(K::Binary | K::Logical));
    let mut ancestor = parent;
    while let Some(next) = s.parent(ancestor).filter(|_| !matches!(s.kind(ancestor), Some(K::Function | K::Arrow | K::Program))) {
        if matches!(s.kind(ancestor), Some(K::Return | K::If | K::While | K::DoWhile | K::Conditional | K::For)) {
            break;
        }
        ancestor = next;
    }
    let mut return_owner = parent;
    while let Some(next) = s.parent(return_owner).filter(|_| !matches!(s.kind(return_owner), Some(K::Return | K::Arrow | K::Function | K::Program))) {
        return_owner = next;
    }
    Value::record(vec![
        ("offset".into(), Value::from(s.tree.node(node).start as usize)),
        ("functions".into(), Value::Bool(matches!(s.kind(inner), Some(K::Function | K::Arrow)) || assertion(s, inner))),
        ("sequence".into(), Value::Bool(s.is(inner, K::Sequence))),
        ("nested_binary".into(), Value::Bool(binary && matches!(s.kind(parent), Some(K::Binary | K::Logical))
            || count == 1 && matches!(s.kind(inner), Some(K::Union | K::Intersection)) && matches!(s.kind(parent), Some(K::Union | K::Intersection)))),
        ("ternary_binary".into(), Value::Bool(binary && s.is(parent, K::Conditional))),
        ("nested_conditional".into(), Value::Bool(s.is(inner, K::Conditional) && s.is(parent, K::Conditional))),
        ("conditional_assign".into(), Value::Bool(s.is(inner, K::Assign) && matches!(s.kind(ancestor), Some(K::If | K::While | K::DoWhile | K::Conditional | K::For)))),
        ("return_assign".into(), Value::Bool(s.is(inner, K::Assign) && matches!(s.kind(return_owner), Some(K::Return | K::Arrow)))),
        ("arrow_conditional".into(), Value::Bool(s.is(inner, K::Conditional) && s.is(parent, K::Arrow))),
        ("new_member".into(), Value::Bool(s.is(inner, K::New) && matches!(s.kind(parent), Some(K::Member | K::Index)))),
        ("function_member".into(), Value::Bool(function_prototype(s, inner)
            || s.is(inner, K::Function) && s.parent(parent).is_some_and(|call| function_prototype(s, call)))),
        ("jsx".into(), Value::Bool(matches!(s.kind(inner), Some(K::JsxElement | K::JsxFragment)))),
        ("multiline".into(), Value::Bool(s.text(inner).contains(['\n', '\r', '\u{2028}', '\u{2029}']))),
        ("spread_conditional".into(), Value::Bool(s.is(parent, K::Spread) && s.is(inner, K::Conditional))),
        ("spread_logical".into(), Value::Bool(s.is(parent, K::Spread) && s.is(inner, K::Logical))),
        ("spread_assignment".into(), Value::Bool(s.is(parent, K::Spread) && s.is(inner, K::Assign))),
        ("spread_await".into(), Value::Bool(s.is(parent, K::Spread) && s.is(inner, K::Await))),
        ("selectors".into(), selectors(s, parent)),
        ("comment".into(), comment.map_or(Value::Null, |c| Value::from(c.body(s.src).to_owned()))),
    ])
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use crate::engine::tests::native_linter;

    #[test]
    fn precedence_and_statement_context_preserve_required_parentheses() {
        let lint = native_linter("rule no-extra-parens");
        for (source, count) in [
            ("const x = (a + b) * c;", 0),
            ("const x = a + (b * c);", 1),
            ("const x = a - (b - c);", 0),
            ("({}).method();", 0),
            ("const x = ((a));", 1),
            ("for (let a = (b in c);;);", 0),
            ("for (let a = (b + c);;);", 1),
        ] {
            assert_eq!(lint.lint(Path::new("/root/a.ts"), source).len(), count, "{source}");
        }
    }
}
