//! Declaration groups in their function or lexical scope.

use std::collections::HashMap;
use crate::expr::Value;
use crate::native::ts::{facts::NativeFile, kinds::{Field as F, Kind as K}, syntax::Syntax};

#[derive(Default)]
struct Counts {
    initialized: usize,
    uninitialized: usize,
    requires: usize,
}

impl Counts {
    fn read(s: &Syntax, node: u32) -> Self {
        let mut counts = Self { initialized: 0, uninitialized: 0, requires: 0 };
        for declarator in s.all(node, F::Decl) {
            if let Some(init) = s.get(declarator, F::Init) {
                counts.initialized += 1;
                if s.is(init, K::Call) && s.get(init, F::Callee).is_some_and(|callee| s.is(callee, K::Ident) && s.text(callee) == "require") {
                    counts.requires += 1;
                }
            } else {
                counts.uninitialized += 1;
            }
        }
        counts
    }

    fn value(&self) -> Value<'static> {
        Value::record(vec![
            ("initialized".into(), Value::from(self.initialized)),
            ("uninitialized".into(), Value::from(self.uninitialized)),
            ("requires".into(), Value::from(self.requires)),
        ])
    }
}

fn scope(s: &Syntax, mut node: u32, function: bool) -> u32 {
    while let Some(parent) = s.parent(node) {
        if matches!(s.kind(parent), Some(K::Program | K::Function | K::Arrow | K::Method | K::StaticBlock))
            || !function && matches!(s.kind(parent), Some(K::Block | K::For | K::ForIn | K::ForOf | K::Switch)) {
            return parent;
        }
        node = parent;
    }
    s.root()
}

struct Declarations(HashMap<u32, Value<'static>>);

pub(super) fn groups(file: &NativeFile, node: u32) -> Value<'static> {
    file.cached(scan).0.get(&node).cloned().unwrap_or(Value::Null)
}

fn scan(file: &NativeFile) -> Declarations {
    let s = &file.syntax;
    let mut declarations: Vec<_> = (0..s.tree.nodes.len() as u32).filter(|&id| s.is(id, K::Var)).collect();
    declarations.sort_unstable_by_key(|&id| s.tree.node(id).start);
    let mut scopes: HashMap<(u32, String), Counts> = HashMap::new();
    let mut entries = HashMap::new();
    for node in declarations {
        let kind = s.word(node, F::Kind).unwrap_or("var");
        let prior = scopes.entry((scope(s, node, kind == "var"), kind.to_owned())).or_default();
        let counts = Counts::read(s, node);
        let parent = s.parent(node);
        let previous = parent.and_then(|parent| {
            let mut previous = None;
            for child in s.all(parent, F::Body) {
                if child == node {
                    return previous.filter(|&previous| s.is(previous, K::Var) && s.word(previous, F::Kind) == Some(kind));
                }
                previous = Some(child);
            }
            None
        });
        entries.insert(node, Value::record(vec![
            ("kind".into(), Value::from(kind.to_owned())),
            ("counts".into(), counts.value()),
            ("prior".into(), prior.value()),
            ("previous".into(), previous.map_or(Value::Null, |previous| Counts::read(s, previous).value())),
            ("for_init".into(), Value::Bool(parent.is_some_and(|parent| s.is(parent, K::For)) && s.field(node) == Some(F::Init))),
            ("for_binding".into(), Value::Bool(parent.is_some_and(|parent| matches!(s.kind(parent), Some(K::ForIn | K::ForOf))))),
        ]));
        prior.initialized += counts.initialized;
        prior.uninitialized += counts.uninitialized;
        prior.requires += counts.requires;
    }
    Declarations(entries)
}
