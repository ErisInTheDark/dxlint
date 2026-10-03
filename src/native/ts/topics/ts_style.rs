//! typescript-eslint's rules without type information (`std/ts/style`).

use crate::expr::facts::Fact;
use crate::expr::Value;
use super::super::facts::NativeFile;
use super::super::kinds::{Field as F, Kind as K};
use super::super::syntax::Syntax;

mod declarations;
mod class_styles;
mod indexed_objects;
mod loops;
mod expressions;
mod parameter_assignments;
mod signatures;
mod return_types;
mod boundary_types;
mod void_types;

pub const FACTS: &[Fact] = &[
    Fact { name: "function_boundary_style", doc: "native layers: function or parameter reachable through module exports, with type exemptions and parameter diagnostics", get: |f, n| f.with_file(boundary_types::function_boundary_style, n) },
    Fact { name: "function_type_candidate", doc: "native layers: sole call or construct signature replaceable by a function type, or invalid interface this type", get: |f, n| f.with_file(signatures::function_type_candidate, n) },
    Fact { name: "nonadjacent_signatures", doc: "native layers: function or member overloads separated by another declaration", get: |f, n| f.with_file(signatures::nonadjacent_signatures, n) },
    Fact { name: "generic_constructor_style", doc: "native layers: generic arguments belong on constructor or type annotation of an initialized declaration", get: |f, n| f.with_file(declarations::generic_constructor_style, n) },
    Fact { name: "function_return_style", doc: "native layers: explicit return-type context, exemptions, name and function head offset", get: |f, n| f.with_file(return_types::function_return_style, n) },
    Fact { name: "unifiable_signatures", doc: "native layers: overload pairs with identical or unifiable parameter lists, names and comments", get: |f, n| f.with_file(signatures::unifiable_signatures, n) },
    Fact { name: "unnecessary_parameter_assignment", doc: "native layers: redundant constructor parameter-property assignment without earlier writes", get: |f, n| f.with_file(parameter_assignments::unnecessary_parameter_assignment, n) },
    Fact { name: "expression_statement", doc: "native layers: expression statement directive status and effectful option modes", get: |f, n| f.with_file(expressions::expression_statement, n) },
    Fact { name: "member_style", doc: "native layers: member display name, undecorated head offset and public modifier offset", get: |f, n| f.with_file(class_styles::member_style, n) },
    Fact { name: "extraneous_class", doc: "native layers: empty, constructor-only or static-only class", get: |f, n| f.with_file(class_styles::extraneous_class, n) },
    Fact { name: "literal_member", doc: "native layers: literal getter or readonly field eligible for the alternative declaration style", get: |f, n| f.with_file(class_styles::literal_member, n) },
    Fact { name: "simple_array_iteration", doc: "native layers: counted loop index is used only for reading one array", get: |f, n| f.with_file(loops::simple_array_iteration, n) },
    Fact { name: "record_convertible", doc: "native layers: indexed object can become a Record without recursion or key dependencies", get: |f, n| f.with_file(indexed_objects::record_convertible, n) },
    Fact { name: "any_rest_parameter", doc: "native layers: any nested in a rest function parameter's array type", get: |f, n| f.with_file(any_rest_parameter, n) },
    Fact { name: "dynamic_delete", doc: "native layers: an index expression deleted with a nonliteral key", get: |f, n| f.with_file(dynamic_delete, n) },
    Fact { name: "declared_namespace", doc: "native layers: namespace nested within a declare namespace", get: |f, n| f.with_file(declared_namespace, n) },
    Fact { name: "useless_empty_export", doc: "native layers: empty export alongside another import or export", get: |f, n| f.with_file(useless_empty_export, n) },
    Fact { name: "literal_enum_initializer", doc: "native layers: enum initializer is a literal, optionally signed", get: |f, n| f.with_file(literal_enum_initializer, n) },
    Fact { name: "bitwise_enum_initializer", doc: "native layers: enum initializer uses literals or enum members in bitwise operations", get: |f, n| f.with_file(bitwise_enum_initializer, n) },
    Fact { name: "void_type_context", doc: "native layers: whether void is a return, generic, default, this parameter, union or other type", get: |f, n| f.with_file(void_types::context, n) },
    Fact { name: "parameter_property_modifiers", doc: "native layers: accessibility and readonly modifiers of a parameter or class property", get: |f, n| f.with_file(declarations::parameter_property_modifiers, n) },
    Fact { name: "parameter_property_candidate", doc: "native layers: class property matching a constructor parameter and leading this assignment", get: |f, n| f.with_file(declarations::parameter_property_candidate, n) },
    Fact { name: "assertion_as_parameter", doc: "native layers: assertion used as a call argument, throw value, parameter default, JSX expression or tagged template substitution", get: |f, n| f.with_file(assertion_as_parameter, n) },
    Fact { name: "inferrable_annotation", doc: "native layers: annotated primitive or RegExp type stated by a declaration's initializer", get: |f, n| f.with_file(declarations::inferrable_annotation, n) },
    Fact { name: "redundant_non_null_coalescing", doc: "native layers: non-null assertion before ?? with an already assigned operand", get: |f, n| f.with_file(redundant_non_null_coalescing, n) },
];

fn any_rest_parameter(file: &NativeFile, node: u32) -> Value<'static> {
    let s = &file.syntax;
    let Some(parent) = s.parent(node) else { return Value::Bool(false) };
    let Some(mut parameter) = s.parent(parent) else { return Value::Bool(false) };
    if s.is(parameter, K::TypeOperator) && s.word(parameter, F::Op) == Some("readonly")
        || s.is(parameter, K::TypeRef) && matches!(s.word(parameter, F::Name), Some("Array" | "ReadonlyArray"))
    {
        let Some(next) = s.parent(parameter) else { return Value::Bool(false) };
        parameter = next;
    }
    let rest = s.is(parameter, K::Param) && s.get(parameter, F::Name).is_some_and(|n| s.is(n, K::Rest));
    Value::Bool(rest && s.parent(parameter).is_some_and(|n| matches!(s.kind(n), Some(K::Function | K::Arrow | K::Method | K::MethodSig | K::CallSig | K::FnType))))
}

fn dynamic_delete(file: &NativeFile, node: u32) -> Value<'static> {
    let s = &file.syntax;
    let mut at = node;
    while let Some(parent) = s.parent(at).filter(|&n| s.is(n, K::Paren)) {
        at = parent;
    }
    let Some(parent) = s.parent(at) else { return Value::Bool(false) };
    let deleted = s.is(parent, K::Unary) && s.word(parent, F::Op) == Some("delete");
    let static_key = s.get(node, F::Index).is_some_and(|n| {
        let n = s.unparen(n);
        matches!(s.kind(n), Some(K::Number | K::String))
            || s.is(n, K::Unary) && s.word(n, F::Op) == Some("-")
                && s.get(n, F::Arg).is_some_and(|n| s.is(s.unparen(n), K::Number))
    });
    Value::Bool(deleted && !static_key)
}

fn declared_namespace(file: &NativeFile, node: u32) -> Value<'static> {
    let s = &file.syntax;
    let mut at = Some(node);
    while let Some(n) = at {
        if s.is(n, K::Namespace) && s.has_word(n, F::Modifier, "declare") {
            return Value::Bool(true);
        }
        at = s.parent(n);
    }
    Value::Bool(false)
}

fn empty_export(s: &Syntax, node: u32) -> bool {
    s.is(node, K::Export) && s.get(node, F::Decl).is_none() && s.get(node, F::Expr).is_none()
        && s.get(node, F::From).is_none() && s.get(node, F::Namespace).is_none()
        && s.all(node, F::Specifier).next().is_none() && s.text(node).contains('{')
}

fn useless_empty_export(file: &NativeFile, node: u32) -> Value<'static> {
    let s = &file.syntax;
    let other = s.parent(node).is_some_and(|parent| s.all(parent, F::Body).any(|n| {
        matches!(s.kind(n), Some(K::Import | K::ImportEquals | K::ExportAssign | K::Export)) && !empty_export(s, n)
    }));
    Value::Bool(empty_export(s, node) && other)
}

fn enum_initializer(file: &NativeFile, node: u32, bitwise: bool) -> bool {
    let s = &file.syntax;
    let Some(init) = s.get(node, F::Init) else { return true };
    let Some(declaration) = s.parent(node) else { return false };
    allowed_enum_expression(s, declaration, init, bitwise, false)
}

fn allowed_enum_expression(s: &Syntax, declaration: u32, node: u32, bitwise: bool, in_bitwise: bool) -> bool {
    let node = s.unparen(node);
    if in_bitwise {
        let member_name = if s.is(node, K::Ident) {
            Some(s.text(node).to_string())
        } else if matches!(s.kind(node), Some(K::Member | K::Index)) && s.word(node, F::Object) == s.word(declaration, F::Name) {
            super::super::statics::property_name(s, node)
        } else {
            None
        };
        if member_name.is_some_and(|name| s.all(declaration, F::Member).any(|n| super::super::statics::property_name(s, n).as_deref() == Some(name.as_str()))) {
            return true;
        }
    }
    match s.kind(node) {
        Some(K::Number | K::String | K::BigInt | K::Bool | K::Null | K::Regex) => true,
        Some(K::Template) => s.all(node, F::Expr).next().is_none(),
        Some(K::Unary) => {
            let op = s.word(node, F::Op);
            let allowed = matches!(op, Some("+" | "-")) || bitwise && op == Some("~");
            allowed && s.get(node, F::Arg).is_some_and(|n| allowed_enum_expression(s, declaration, n, bitwise, in_bitwise || op == Some("~")))
        }
        Some(K::Binary) if bitwise && matches!(s.word(node, F::Op), Some("&" | "^" | "<<" | ">>" | ">>>" | "|")) => {
            s.get(node, F::Left).is_some_and(|n| allowed_enum_expression(s, declaration, n, bitwise, true))
                && s.get(node, F::Right).is_some_and(|n| allowed_enum_expression(s, declaration, n, bitwise, true))
        }
        _ => false,
    }
}

fn literal_enum_initializer(file: &NativeFile, node: u32) -> Value<'static> {
    Value::Bool(enum_initializer(file, node, false))
}

fn bitwise_enum_initializer(file: &NativeFile, node: u32) -> Value<'static> {
    Value::Bool(enum_initializer(file, node, true))
}

fn assertion_as_parameter(file: &NativeFile, node: u32) -> Value<'static> {
    let s = &file.syntax;
    let mut at = node;
    while let Some(parent) = s.parent(at).filter(|&n| s.is(n, K::Paren)) {
        at = parent;
    }
    let Some(parent) = s.parent(at) else { return Value::Bool(false) };
    Value::Bool(matches!(s.kind(parent), Some(K::New | K::Call | K::Throw | K::AssignPattern | K::JsxExpression))
        || s.is(parent, K::Param) && s.field(at) == Some(F::Init)
        || s.is(parent, K::Template) && s.parent(parent).is_some_and(|n| s.is(n, K::TaggedTemplate)))
}

fn redundant_non_null_coalescing(file: &NativeFile, node: u32) -> Value<'static> {
    let s = &file.syntax;
    let mut at = node;
    while let Some(parent) = s.parent(at).filter(|&n| s.is(n, K::Paren)) {
        at = parent;
    }
    if !s.parent(at).is_some_and(|p| s.is(p, K::Logical) && s.word(p, F::Op) == Some("??") && s.field(at) == Some(F::Left)) {
        return Value::Bool(false);
    }
    let Some(expression) = s.get(node, F::Expr).map(|n| s.unparen(n)) else { return Value::Bool(false) };
    if !s.is(expression, K::Ident) {
        return Value::Bool(true);
    }
    let Some(declaration) = file.decl_of(expression) else { return Value::Bool(true) };
    let end = s.tree.node(node).end;
    let initialised = s.parent(declaration).is_some_and(|p| {
        s.is(p, K::Declarator) && s.tree.node(p).end < end
            && (s.get(p, F::Init).is_some() || s.text(p)[s.text(declaration).len()..].trim_start().starts_with('!'))
    });
    let written = file.bindings().access.iter().any(|(&reference, &access)| {
        matches!(access, "write" | "read_write") && s.tree.node(reference).end < end && file.decl_of(reference) == Some(declaration)
    });
    Value::Bool(initialised || written)
}
