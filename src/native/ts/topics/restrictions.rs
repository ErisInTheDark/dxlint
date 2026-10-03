//! Configured limits and bans (`std/ts/restrictions`): sizes, restricted
//! names and imports, identifier shapes, sort orders.

use super::super::facts::{Facts, NativeFile};
use super::super::kinds::{Field as F, Kind as K};
use crate::expr::facts::Fact;
use crate::expr::Value;

mod comments;
mod limits;
mod names;
mod sorting;
mod imports;
mod numbers;

pub const FACTS: &[Fact] = &[
    Fact { name: "restriction_import", doc: "native import/export declarations or members: source, imported name, type status, report offset and source parent paths", get: |f, n| f.with_file(imports::restriction, n) },
    Fact { name: "restriction_number", doc: "native numeric literals: signed raw/value and offset, constant, object, array, radix, JSX, defaults and TypeScript contexts", get: |f, n| f.with_file(numbers::context, n) },
    Fact { name: "restriction_duplicate_imports", doc: "native imports and exports: duplicate mergeable sources, separated by type-import option and import/export origin", get: |f, n| f.with_file(imports::duplicates, n) },
    Fact { name: "restriction_import_order", doc: "native imports: declaration syntax and alphabetic order against the previous import, and first unsorted named member", get: |f, n| f.with_file(imports::order, n) },
    Fact { name: "restriction_sorted_variables", doc: "native variable declarations: out-of-order declarators by case mode and report offset", get: |f, n| f.with_file(sorting::variables, n) },
    Fact { name: "restriction_sorted_keys", doc: "native object literals: out-of-order key pairs by comparison and grouping options", get: |f, n| f.with_file(sorting::keys, n) },
    Fact { name: "restriction_statement", doc: "native layers: an expression used as a statement, through parentheses", get: |f, n| f.with_file(statement, n) },
    Fact { name: "restriction_afterthought", doc: "native layers: an update in a for loop's afterthought, through sequences and parentheses", get: |f, n| f.with_file(afterthought, n) },
    Fact { name: "restriction_alert", doc: "native layers: on a call: the unshadowed alert, confirm or prompt it calls; null otherwise", get: |f, n| f.with_file(alert, n) },
    Fact { name: "restriction_label_kind", doc: "native layers: on a labelled statement or jump: loop, switch or other for the label's target", get: |f, n| f.with_file(label_kind, n) },
    Fact { name: "restriction_limits", doc: "native layers: function and file counts for configured limits: parameters, statements, complexity, callback nesting and class counts", get: |f, n| f.with_file(limits::metrics, n) },
    Fact { name: "restriction_depth", doc: "native layers: control-flow nesting depth, resetting at functions and static blocks and excluding else-if branches", get: |f, n| f.with_file(limits::depth, n) },
    Fact { name: "restriction_comments", doc: "native layers: file comments with bodies, offsets, inline and consecutive status and initial letter", get: |f, n| f.with_file(comments::comments, n) },
    Fact { name: "restriction_lines", doc: "native layers: lines covered by a file or function: offset, blank and comment-only for each line", get: |f, n| f.with_file(comments::lines, n) },
    Fact { name: "restriction_name", doc: "native layers: identifier eligibility for id-denylist, id-length and id-match, with name and property/declaration/destructuring roles", get: |f, n| f.with_file(names::name, n) },
    Fact { name: "restriction_properties", doc: "native layers: on a member or object pattern: restricted-property candidates with object name, property name and report offset", get: |f, n| f.with_file(names::properties, n) },
    Fact { name: "restriction_exports", doc: "native layers: on an export: exported names, offsets and default-export form", get: |f, n| f.with_file(names::exports, n) },
    Fact { name: "restriction_global_member", doc: "native layers: on a global-object member: root global, static property and property report offset; self-name chains are transparent", get: |f, n| f.with_file(names::global_member, n) },
];

fn statement(file: &NativeFile, node: u32) -> Value<'static> {
    let s = &file.syntax;
    let mut at = node;
    while let Some(parent) = s.parent(at) {
        if s.is(parent, K::Paren) {
            at = parent;
        } else {
            return Value::Bool(s.is(parent, K::ExprStmt));
        }
    }
    Value::Bool(false)
}

fn afterthought(file: &NativeFile, node: u32) -> Value<'static> {
    let s = &file.syntax;
    let mut at = node;
    while let Some(parent) = s.parent(at) {
        if matches!(s.kind(parent), Some(K::Paren | K::Sequence)) {
            at = parent;
        } else {
            return Value::Bool(s.is(parent, K::For) && s.field(at) == Some(F::Update));
        }
    }
    Value::Bool(false)
}

fn alert(file: &NativeFile, node: u32) -> Value<'static> {
    let s = &file.syntax;
    let Some(callee) = s.get(node, F::Callee).map(|n| s.unparen(n)) else { return Value::Null };
    let name = if s.is(callee, K::Ident) && file.resolved(callee) == Some("global") {
        Some(s.text(callee).to_string())
    } else if matches!(s.kind(callee), Some(K::Member | K::Index)) {
        s.get(callee, F::Object).filter(|&obj| {
            let obj = s.unparen(obj);
            s.is(obj, K::Ident) && file.resolved(obj) == Some("global") && (s.text(obj) == "window" || s.text(obj) == "globalThis" && file.global_setting(obj).is_some())
                || s.is(obj, K::This) && file.enclosing_function(obj).is_none()
        }).and_then(|_| super::super::statics::property_name(s, callee))
    } else {
        None
    };
    match name.filter(|n| matches!(n.as_str(), "alert" | "confirm" | "prompt")) {
        Some(name) => Value::from(name),
        None => Value::Null,
    }
}

fn label_kind(file: &NativeFile, node: u32) -> Value<'static> {
    let s = &file.syntax;
    let target = if s.is(node, K::Labeled) {
        Some(node)
    } else {
        let label = s.word(node, F::Label);
        let mut at = s.parent(node);
        let mut found = None;
        while let Some(n) = at {
            if s.is(n, K::Labeled) && s.word(n, F::Label) == label {
                found = Some(n);
                break;
            }
            at = s.parent(n);
        }
        found
    };
    let kind = target.and_then(|n| s.get(n, F::Body)).and_then(|n| s.kind(n));
    Value::from(match kind {
        Some(K::For | K::ForIn | K::ForOf | K::While | K::DoWhile) => "loop",
        Some(K::Switch) => "switch",
        _ => "other",
    })
}
