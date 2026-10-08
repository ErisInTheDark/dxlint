//! Compiled attribute dispatch and reads through the file's existing memo.

use super::{AttrMemo, CExpr, Ctx, Value};
use crate::grammar::{Grammar, ROOT, TEXT};
use crate::vm::{self, Begin, KindSet, VExpr, V};
use std::collections::hash_map::Entry as MapEntry;

/// A `let` attribute: its expression on each layer that defines it.
pub struct Attr {
    pub name: String,
    /// Specificity order, resolved by the config compiler.
    pub defs: Vec<(u32, CExpr)>,
}

pub struct Attributes {
    pub(crate) vm_stats: bool,
    attrs: Vec<CompiledAttr>,
    /// Per matcher: how many fields its VM records hold
    /// ([`Attributes::shape_records`]).
    layout: Vec<u16>,
    /// `ZLINT_VM_STATS`: what kept each unlowered definition interpreted.
    vm_blockers: Vec<String>,
}

struct CompiledAttr {
    name: String,
    defs: Vec<CExpr>,
    /// Each definition's typed form, when it lowers to the VM.
    lowered: Vec<Option<VExpr>>,
    routes: Vec<Route>,
}

/// A `Route::Defined` without a field in the matcher's VM record.
const NO_SLOT: u16 = u16::MAX;

#[derive(Clone, Copy)]
enum Route {
    Missing,
    /// The definition's index, and its field in the matcher's VM record.
    Defined(u32, u16),
    Forward(u16),
}

pub(super) enum Entry<'a> {
    Busy,
    Ready(Value<'a>),
}

impl Attributes {
    pub fn new(attrs: Vec<Attr>, forwards: &[(u32, u16, u16)], grammar: &Grammar) -> Self {
        let stats = std::env::var_os("ZLINT_VM_STATS").is_some();
        if attrs.is_empty() { return Self { vm_stats: stats, attrs: Vec::new(), layout: Vec::new(), vm_blockers: Vec::new() }; }
        let mut vm_blockers = Vec::new();
        let ancestors: Vec<Vec<usize>> = (0..grammar.matchers.len()).map(|kind| {
            let mut chain = Vec::new();
            let mut at = Some(kind as u32);
            while let Some(id) = at {
                chain.push(id as usize);
                at = grammar.matchers[id as usize].like;
            }
            chain
        }).collect();
        let mut by_attr = vec![Vec::new(); attrs.len()];
        for (index, &(matcher, attr, label)) in forwards.iter().enumerate() {
            by_attr[attr as usize].push((matcher, index, label));
        }
        let attrs = attrs.into_iter().zip(by_attr).map(|(attr, forwards)| {
            let lowered: Vec<Option<VExpr>> = attr.defs.iter().map(|(_, expr)| match vm::lower(expr, &grammar.functions) {
                Ok(lowered) => Some(lowered),
                Err(blocker) => {
                    if stats {
                        vm_blockers.push(vm::blocker_name(blocker));
                    }
                    None
                }
            }).collect();
            let mut routes = Vec::new();
            if !attr.defs.is_empty() || !forwards.is_empty() {
                let mut definitions = vec![u32::MAX; grammar.matchers.len()];
                for (index, (matcher, _)) in attr.defs.iter().enumerate() {
                    definitions[*matcher as usize] = definitions[*matcher as usize].min(index as u32);
                }
                let mut forwarding = vec![None; grammar.matchers.len()];
                for (matcher, index, label) in forwards {
                    forwarding[matcher as usize] = Some((index, label));
                }
                routes = ancestors.iter().map(|chain| {
                    let definition = chain.iter().map(|&id| definitions[id]).min().unwrap_or(u32::MAX);
                    if definition != u32::MAX { return Route::Defined(definition, NO_SLOT); }
                    // Forwards keep declaration order, not nearest-ancestor order.
                    chain.iter().filter_map(|&id| forwarding[id]).min_by_key(|(index, _)| *index)
                        .map_or(Route::Missing, |(_, label)| Route::Forward(label))
                }).collect();
            }
            CompiledAttr {
                name: attr.name,
                defs: attr.defs.into_iter().map(|(_, expr)| expr).collect(),
                lowered,
                routes,
            }
        }).collect();
        let layout = vec![0; grammar.matchers.len()];
        Self { vm_stats: stats, attrs, layout, vm_blockers }
    }

    pub(crate) fn report_vm_stats(&self, memo: &AttrMemo<'_>, tree: &crate::grammar::Tree, file: &str) {
        if !self.vm_stats { return; }
        let attrs = memo.attributes.borrow();
        let lowered = attrs.keys().filter(|&&(node, attr)| {
            matches!(self.route(tree.node(node).matcher, attr), Route::Defined(_, slot) if slot != NO_SLOT)
        }).count();
        // Inline payload estimate only: hash-table controls, spare buckets
        // and shared list/record/string allocations are not included.
        let entry_bytes = std::mem::size_of::<((u32, u16), Entry<'_>)>();
        memo.vm.report_stats(tree, &self.layout, file);
        eprintln!("zlint vm memo: {file} entries={} capacity={} inline_capacity_approx_bytes={} lowered_entries={lowered} lowered_inline_bytes={}",
            attrs.len(), attrs.capacity(), attrs.capacity() * entry_bytes, lowered * entry_bytes);
    }

    /// Lays out each node kind's VM record: exactly the lowered attributes
    /// that `roots` (expressions evaluated on nodes of the given kinds) can
    /// read on that kind, following reads through attribute definitions,
    /// `fn` bodies, fields, parents and children with the grammar's shapes.
    /// Returns the number of record fields over all kinds.
    pub fn shape_records(&mut self, grammar: &Grammar, roots: Vec<(KindSet, &CExpr)>) -> usize {
        let need = Needs::find(self, grammar, roots);
        self.layout.fill(0);
        for attr in &mut self.attrs {
            for route in &mut attr.routes {
                if let Route::Defined(_, slot) = route {
                    *slot = NO_SLOT;
                }
            }
        }
        for (matcher, attrs) in need.into_iter().enumerate() {
            for (slot, attr) in attrs.into_iter().enumerate() {
                if let Route::Defined(_, field) = &mut self.attrs[attr as usize].routes[matcher] {
                    *field = slot as u16;
                    self.layout[matcher] += 1;
                }
            }
        }
        self.layout.iter().map(|&n| n as usize).sum()
    }

    /// `ZLINT_VM_STATS`: what kept each unlowered definition interpreted.
    pub fn vm_blockers(&self) -> &[String] {
        &self.vm_blockers
    }

    /// Definitions lowered to the VM, and all definitions.
    pub fn lowered_counts(&self) -> (usize, usize) {
        let defs = self.attrs.iter().flat_map(|attr| &attr.lowered);
        (defs.clone().filter(|d| d.is_some()).count(), defs.count())
    }

    fn route(&self, matcher: u32, attr: u16) -> Route {
        if matcher == ROOT || matcher == TEXT { return Route::Missing; }
        self.attrs[attr as usize].routes.get(matcher as usize).copied().unwrap_or(Route::Missing)
    }
}

/// Which kinds each attribute can be read on ([`Attributes::shape_records`]).
struct Needs<'e> {
    grammar: &'e Grammar,
    attrs: &'e Attributes,
    all: KindSet,
    need: Vec<KindSet>,
    called: Vec<KindSet>,
    pending: Vec<(KindSet, &'e CExpr)>,
    /// First-request order per kind, grouped by the root that first needs
    /// each attribute. Shared dependencies occupy their first root's group.
    order: Vec<Vec<u16>>,
}

impl<'e> Needs<'e> {
    fn find(attrs: &'e Attributes, grammar: &'e Grammar, roots: Vec<(KindSet, &'e CExpr)>) -> Vec<Vec<u16>> {
        let n = grammar.matchers.len();
        let mut needs = Needs {
            grammar,
            attrs,
            all: KindSet::all(n),
            need: vec![KindSet::empty(n); attrs.attrs.len()],
            called: vec![KindSet::empty(n); grammar.functions.len()],
            pending: Vec::new(),
            order: vec![Vec::new(); n],
        };
        for root in roots {
            needs.pending.push(root);
            while let Some((kinds, e)) = needs.pending.pop() {
                needs.visit(&kinds, e);
            }
        }
        needs.order
    }

    fn map(&self, kinds: &KindSet, step: impl Fn(u32, &mut KindSet)) -> KindSet {
        let mut out = KindSet::empty(self.grammar.matchers.len());
        for k in kinds.iter() {
            step(k, &mut out);
        }
        out
    }

    /// Parent layer nodes (looking through text captures, as reads do).
    fn parents(&self, kinds: &KindSet) -> KindSet {
        self.map(kinds, |k, out| {
            let mut stack = vec![k];
            let mut seen_text = false;
            while let Some(k) = stack.pop() {
                for &p in self.grammar.parent_kinds(k) {
                    if p == TEXT {
                        if !seen_text {
                            seen_text = true;
                            stack.push(TEXT);
                        }
                    } else {
                        out.insert(p);
                    }
                }
            }
        })
    }

    fn children(&self, kinds: &KindSet) -> KindSet {
        self.map(kinds, |k, out| {
            for &c in self.grammar.children_kinds(k) {
                out.insert(c);
            }
        })
    }

    fn field(&self, kinds: &KindSet, label: u16) -> KindSet {
        if kinds.iter().any(|k| self.grammar.field_targets(k, label).any) {
            return self.all.clone();
        }
        self.map(kinds, |k, out| {
            for &c in &self.grammar.field_targets(k, label).kinds {
                out.insert(c);
            }
        })
    }

    fn specialisations(&self, base: u32) -> KindSet {
        let mut out = KindSet::empty(self.grammar.matchers.len());
        for k in 0..self.grammar.matchers.len() as u32 {
            if self.grammar.is_a(k, base) {
                out.insert(k);
            }
        }
        out
    }

    /// `attr` is read on `kinds`: its definitions there are evaluated too.
    fn request(&mut self, attr: u16, kinds: &KindSet) {
        let attrs = self.attrs;
        let Some(compiled) = attrs.attrs.get(attr as usize) else { return };
        let mut by_def: Vec<(u32, KindSet)> = Vec::new();
        for k in kinds.iter() {
            // Each (attribute, kind) is followed once; forwarding kinds too,
            // since forwards can chain through themselves.
            if !self.need[attr as usize].insert(k) {
                continue;
            }
            match compiled.routes.get(k as usize) {
                Some(Route::Defined(def, _)) => {
                    if compiled.lowered[*def as usize].is_some() {
                        self.order[k as usize].push(attr);
                    }
                    match by_def.iter_mut().find(|(d, _)| d == def) {
                        Some((_, set)) => { set.insert(k); }
                        None => {
                            let mut set = KindSet::empty(self.grammar.matchers.len());
                            set.insert(k);
                            by_def.push((*def, set));
                        }
                    }
                }
                Some(Route::Forward(label)) => {
                    let mut one = KindSet::empty(self.grammar.matchers.len());
                    one.insert(k);
                    let targets = self.field(&one, *label);
                    self.request(attr, &targets);
                }
                _ => {}
            }
        }
        for (def, set) in by_def {
            self.pending.push((set, &compiled.defs[def as usize]));
        }
    }

    fn visit(&mut self, kinds: &KindSet, e: &'e CExpr) {
        use super::Relative;
        match e {
            CExpr::Attr(a) => self.request(*a, kinds),
            CExpr::ParentAttr(a) => {
                let parents = self.parents(kinds);
                self.request(*a, &parents);
            }
            CExpr::FieldAttr(label, a) => {
                let fields = self.field(kinds, *label);
                self.request(*a, &fields);
            }
            CExpr::OnParent(inner) => self.pending.push((self.parents(kinds), inner)),
            CExpr::OnField(label, inner) => self.pending.push((self.field(kinds, *label), inner)),
            CExpr::Relative(Relative::Children, inner) | CExpr::OwnChild(_, _, inner) => self.pending.push((self.children(kinds), inner)),
            CExpr::Relative(_, inner) => {
                let siblings = self.children(&self.parents(kinds));
                self.pending.push((siblings, inner));
            }
            CExpr::OnAncestor(base, inner) => self.pending.push((self.specialisations(*base), inner)),
            CExpr::Member(base, _, part) => {
                self.visit(kinds, base);
                if let Some(part) = part {
                    self.pending.push((self.all.clone(), part));
                }
            }
            CExpr::Path(base, _, parts) => {
                self.visit(kinds, base);
                for part in parts.iter().flatten() {
                    self.pending.push((self.all.clone(), part));
                }
            }
            CExpr::UserCall(i, args) => {
                for arg in args {
                    self.visit(kinds, arg);
                }
                let grammar = self.grammar;
                if let (Some(called), Some(body)) = (self.called.get_mut(*i as usize), grammar.functions.get(*i as usize).and_then(|f| f.body.get())) {
                    let new = {
                        let mut new = KindSet::empty(grammar.matchers.len());
                        for k in kinds.iter() {
                            if called.insert(k) {
                                new.insert(k);
                            }
                        }
                        new
                    };
                    if !new.is_empty() {
                        self.pending.push((new, body));
                    }
                }
            }
            other => other.for_each_child(&mut |child| self.visit(kinds, child)),
        }
    }
}

impl vm::Host for Ctx<'_> {
    fn tree(&self) -> &crate::grammar::Tree {
        self.tree
    }

    fn grammar(&self) -> &Grammar {
        self.grammar
    }

    fn source(&self) -> &str {
        self.source
    }

    fn attr(&self, node: u32, attr: u16) -> Option<V> {
        match self.attrs.route(self.tree.node(node).matcher, attr) {
            Route::Missing => Some(V::Null),
            Route::Forward(_) | Route::Defined(_, NO_SLOT) => None,
            Route::Defined(index, slot) => self.vm_attr(node, attr, index, slot),
        }
    }

    fn fact(&self, node: u32, fact: usize, projection: Option<usize>, of: super::FactOf) -> Option<V> {
        // Same reads and memo as the interpreter's (recorded reads included).
        let on = Ctx { node, ..*self };
        Some(match on.fact_value(fact, projection, of) {
            Value::Null => V::Null,
            Value::Bool(b) => V::Bool(b),
            Value::Num(n) => V::Num(n),
            Value::Node(id, _) => V::Node(id),
            _ => return None,
        })
    }
}

impl<'a> Ctx<'a> {
    /// A VM-lowered attribute from the node's record, computing it there
    /// first; `None` when the VM gives up (the interpreter answers).
    fn vm_attr(&self, node: u32, attr: u16, index: u32, slot: u16) -> Option<V> {
        match self.memo.vm.begin(self.tree, &self.attrs.layout, node, slot) {
            Begin::Ready(v) => Some(v),
            Begin::GiveUp => None,
            Begin::Compute(at) => {
                let lowered = self.attrs.attrs[attr as usize].lowered[index as usize].as_ref().expect("record fields are lowered");
                let v = vm::eval(self, node, lowered);
                self.memo.vm.finish(at, v);
                v
            }
        }
    }

    pub(super) fn attr(&self, node: u32, attr: u16) -> Result<Value<'a>, String> {
        if self.memo.params.borrow().len() == self.memo.param_base.get() {
            self.attr_unbound(node, attr)
        } else {
            self.unbound(|| self.attr_unbound(node, attr))
        }
    }

    fn attr_unbound(&self, node: u32, attr: u16) -> Result<Value<'a>, String> {
        // `ZLINT_VM=verify`: the VM's value, compared with the interpreter's.
        let mut checked = None;
        let index = match self.attrs.route(self.tree.node(node).matcher, attr) {
            Route::Missing => return Ok(Value::Null),
            Route::Forward(label) => {
                let mut values = Vec::new();
                for id in self.tree.fields(node, label) {
                    values.push(self.attr(id, attr)?);
                }
                return Ok(match values.len() {
                    0 => Value::Null,
                    1 => values.pop().expect("one"),
                    _ => Value::list(values),
                });
            }
            Route::Defined(index, slot) => {
                if slot != NO_SLOT && vm::mode() != vm::Mode::Off {
                    if let Some(v) = self.vm_attr(node, attr, index, slot) {
                        if vm::mode() == vm::Mode::On {
                            return Ok(v.value(self.tree, self.source));
                        }
                        checked = Some(v);
                    }
                }
                index as usize
            }
        };
        let def = &self.attrs.attrs[attr as usize];
        let key = (node, attr);
        match self.memo.attributes.borrow_mut().entry(key) {
            MapEntry::Occupied(entry) => match entry.get() {
                Entry::Ready(value) => return Ok(value.clone()),
                Entry::Busy => return Err(format!("attribute `{}` depends on itself", def.name)),
            },
            MapEntry::Vacant(entry) => { entry.insert(Entry::Busy); }
        }
        let ctx = Ctx { node, options: &[], ..*self };
        match ctx.eval(&def.defs[index]) {
            Ok(value) => {
                if let Some(v) = checked.filter(|v| !v.same(&value, self.tree, self.source)) {
                    eprintln!("zlint vm mismatch: {} attribute `{}` on node {node}: vm {v:?}, interpreter {value:?}", self.file, def.name);
                }
                self.memo.attributes.borrow_mut().insert(key, Entry::Ready(value.clone()));
                Ok(value)
            }
            Err(error) => {
                self.memo.attributes.borrow_mut().remove(&key);
                Err(error)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{self, source::Disk};

    fn grammar() -> Grammar {
        let loaded = config::load_text("attrs.zlint", r#"
            lang t "*.t"
            match base "a"
            match member like base "b"
            match leaf like member "c"
            match other "d"
            in .t {
                match leaf
                match other
            }
        "#, &Disk);
        assert!(loaded.errors.is_empty(), "{:?}", loaded.errors);
        let mut errors = Vec::new();
        let grammar = Grammar::build(&loaded.config, &mut errors);
        assert!(errors.is_empty(), "{errors:?}");
        grammar
    }

    fn number(value: f64) -> CExpr { CExpr::Const(Value::Num(value)) }

    #[test]
    fn definitions_keep_compiler_priority_over_inherited_forwards() {
        let g = grammar();
        let base = g.matcher("base").unwrap();
        let member = g.matcher("member").unwrap();
        let leaf = g.matcher("leaf").unwrap();
        let other = g.matcher("other").unwrap();
        let plan = Attributes::new(vec![
            Attr { name: "specific".into(), defs: vec![(member, number(2.0)), (base, number(1.0))] },
            Attr { name: "fallback".into(), defs: vec![(base, number(3.0))] },
        ], &[(leaf, 0, 0), (leaf, 1, 0)], &g);
        let read = |attr| match plan.route(leaf, attr) {
            Route::Defined(index, _) => match &plan.attrs[attr as usize].defs[index as usize] {
                CExpr::Const(Value::Num(n)) => *n,
                _ => panic!("number"),
            },
            _ => panic!("definition"),
        };
        assert_eq!(read(0), 2.0);
        assert_eq!(read(1), 3.0);
        for matcher in [other, ROOT, TEXT] {
            assert!(matches!(plan.route(matcher, 0), Route::Missing));
        }
    }

    #[test]
    fn records_hold_exactly_the_attributes_read_on_each_kind() {
        let g = grammar();
        let base = g.matcher("base").unwrap();
        let member = g.matcher("member").unwrap();
        let leaf = g.matcher("leaf").unwrap();
        let other = g.matcher("other").unwrap();
        let mut plan = Attributes::new(vec![
            Attr { name: "read".into(), defs: vec![(base, number(1.0))] },
            Attr { name: "unread".into(), defs: vec![(base, number(2.0))] },
            Attr { name: "chained".into(), defs: vec![(base, CExpr::Attr(0))] },
        ], &[], &g);
        let mut on_leaf = KindSet::empty(g.matchers.len());
        on_leaf.insert(leaf);
        let root = CExpr::Attr(2);
        assert_eq!(plan.shape_records(&g, vec![(on_leaf, &root)]), 2);
        assert_eq!(plan.layout[leaf as usize], 2);
        for m in [base, member, other] {
            assert_eq!(plan.layout[m as usize], 0);
        }
    }

    #[test]
    fn forwards_keep_first_matching_declaration_not_nearest_ancestor() {
        let g = grammar();
        let base = g.matcher("base").unwrap();
        let member = g.matcher("member").unwrap();
        let leaf = g.matcher("leaf").unwrap();
        let plan = Attributes::new(vec![
            Attr { name: "broad_first".into(), defs: Vec::new() },
            Attr { name: "narrow_first".into(), defs: Vec::new() },
        ], &[(base, 0, 7), (member, 0, 9), (member, 1, 9), (base, 1, 7)], &g);
        assert!(matches!(plan.route(leaf, 0), Route::Forward(7)));
        assert!(matches!(plan.route(leaf, 1), Route::Forward(9)));
        for matcher in [ROOT, TEXT] {
            assert!(matches!(plan.route(matcher, 0), Route::Missing));
        }
    }

    #[test]
    fn forwarded_values_keep_empty_scalar_and_list_cardinality_across_files() {
        use std::path::Path;
        use crate::engine::tests::native_linter;

        let lint = native_linter(r#"
            in ts_identifier let size len(text)
            in ts_call forward size from argument
            in ts_parenthesized forward size from expression
            in ts_call rule sizes info "{count([] size)}:{sum([] size)}"
        "#);
        for (source, expected) in [
            ("f(); f(a); f(a, bc);", vec!["0:0", "1:1", "2:3"]),
            ("f((long));", vec!["1:4"]),
        ] {
            let diagnostics = lint.lint(Path::new("/root/a.ts"), source);
            assert_eq!(diagnostics.iter().map(|d| d.message.as_str()).collect::<Vec<_>>(), expected);
        }
        let defined = native_linter(r#"
            in ts_identifier let size len(text)
            in ts_call let size 7
            in ts_call forward size from argument
            in ts_call rule sizes info "{size}"
        "#);
        assert_eq!(defined.lint(Path::new("/root/a.ts"), "f(a, bc);")[0].message, "7");
    }

    #[test]
    fn inherited_definitions_refine_defaults_and_cross_node_reads() {
        use std::path::Path;
        use crate::engine::tests::native_linter;

        let lint = native_linter(r#"
            in ts_node let size 1
            in ts_identifier let size len(text)
            in ts_binary rule operands info "{left.size}:{right.size}"
            in ts_identifier rule direct info "{size}"
        "#);
        let messages = |source| {
            let mut values: Vec<_> = lint.lint(Path::new("/root/a.ts"), source).into_iter().map(|d| d.message).collect();
            values.sort();
            values
        };
        assert_eq!(messages("a + long;"), ["1", "1:4", "4"]);
        assert_eq!(messages("other + b;"), ["1", "5", "5:1"]);
    }

    #[test]
    fn failed_and_cyclic_attributes_leave_other_reads_and_later_files_usable() {
        use std::path::Path;
        use crate::config::ast::Severity;
        use crate::engine::tests::native_linter;

        let lint = native_linter(r#"
            in ts_identifier let bad byte_slice(text, [])
            in ts_identifier let cyclic cyclic
            in ts_identifier let size len(text)
            in ts_identifier rule first info "{bad}"
            in ts_identifier rule second info "{bad}"
            in ts_identifier rule cycle info "{cyclic}"
            in ts_identifier rule good info "{size}"
        "#);
        for (source, size) in [("a;", "1"), ("long;", "4")] {
            let diagnostics = lint.lint(Path::new("/root/a.ts"), source);
            assert_eq!(diagnostics.iter().filter(|d| d.severity == Severity::Error).count(), 3);
            for rule in ["first", "second"] {
                let failure = diagnostics.iter().find(|d| d.rule == rule).unwrap();
                assert!(!failure.message.contains("depends on itself"), "{failure:?}");
            }
            assert!(diagnostics.iter().find(|d| d.rule == "cycle").unwrap().message.contains("depends on itself"));
            assert_eq!(diagnostics.iter().find(|d| d.rule == "good").unwrap().message, size);
        }
    }

    #[test]
    fn cross_node_cycles_unwind_every_pending_attribute_before_retry() {
        use std::path::Path;
        use crate::config::ast::Severity;
        use crate::engine::tests::native_linter;

        let lint = native_linter(r#"
            in ts_binary let cycle left.cycle
            in ts_identifier let cycle parent.cycle
            in ts_binary let broken left.broken
            in ts_identifier let broken byte_slice(text, [])
            in ts_binary rule first info "{broken}"
            in ts_binary rule retry info "{broken}"
            in ts_binary rule cyclic info "{cycle}"
            in ts_identifier rule nested_cycle info "{cycle}"
            in ts_identifier rule good info "{len(text)}"
        "#);
        for source in ["a + long;", "other + b;"] {
            let diagnostics = lint.lint(Path::new("file.ts"), source);
            let failures: Vec<_> = diagnostics.iter().filter(|d| d.severity == Severity::Error).collect();
            assert_eq!(failures.len(), 5, "{diagnostics:?}");
            for rule in ["first", "retry"] {
                assert!(!failures.iter().find(|d| d.rule == rule).unwrap().message.contains("depends on itself"));
            }
            assert_eq!(diagnostics.iter().filter(|d| d.rule == "good").count(), 2);
        }
    }
}
