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
    pub(crate) values: vm::Values,
    attrs: Vec<CompiledAttr>,
    /// Per matcher: how many fields its VM records hold
    /// ([`Attributes::shape_records`]).
    layout: Vec<u16>,
}

struct CompiledAttr {
    name: String,
    defs: Vec<CExpr>,
    /// Each definition's typed form ([`vm::lower`] is total).
    lowered: Vec<Lowered>,
    routes: Vec<Route>,
}

enum Lowered {
    /// Small bodies: evaluated on each read.
    Direct(VExpr),
    /// Cached in the node's VM record (or evaluated when it has no field).
    Cached(VExpr),
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
        let stats = vm::stats();
        if attrs.is_empty() {
            return Self { vm_stats: stats, values: Default::default(), attrs: Vec::new(), layout: Vec::new() };
        }
        let mut values = vm::Values::default();
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
            let lowered: Vec<Lowered> = attr.defs.iter().map(|(_, expr)| match vm::lower(expr, &grammar.functions, &mut values) {
                lowered if lowered.is_direct() => Lowered::Direct(lowered),
                lowered => Lowered::Cached(lowered),
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
        Self { vm_stats: stats, values, attrs, layout }
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
        let mut owners = crate::fx_hash::FxHashMap::default();
        for (attr, def) in self.attrs.iter().enumerate() {
            for (matcher, route) in def.routes.iter().enumerate() {
                if let Route::Defined(index, slot) = route {
                    if *slot != NO_SLOT { owners.insert((matcher as u32, *slot), (attr, *index)); }
                }
            }
        }
        let mut failures = crate::fx_hash::FxHashMap::default();
        for (node, slot) in memo.vm.gave_up_slots(tree, &self.layout) {
            if let Some(&owner) = owners.get(&(tree.node(node).matcher, slot)) {
                *failures.entry(owner).or_insert(0usize) += 1;
            }
        }
        let mut failures: Vec<_> = failures.into_iter().collect();
        failures.sort_by_key(|&(owner, count)| (std::cmp::Reverse(count), owner));
        for ((attr, index), count) in failures.into_iter().take(15) {
            eprintln!("zlint vm gave up: {file} {count} {}#{index}", self.attrs[attr].name);
        }
        for (site, count) in self.values.take_interp_hits() {
            eprintln!("zlint vm interp: {file} {count} {site}");
        }
        for (route, count) in vm::take_fallbacks() {
            eprintln!("zlint vm fallback: {file} {count} {route}");
        }
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
                    if matches!(compiled.lowered[*def as usize], Lowered::Cached(_)) {
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
    fn values(&self) -> &vm::Values {
        &self.attrs.values
    }

    fn file_vm(&self) -> Option<&vm::FileVm> {
        Some(&self.memo.vm)
    }

    fn builtin(&self, node: u32, builtin: super::Builtin) -> Option<Value<'_>> {
        Ctx { node, ..*self }.builtin(builtin).ok()
    }

    fn position(&self, offset: f64) -> Option<super::Position> {
        Some(self.position_at(offset))
    }

    fn overlapping(&self, node: u32, sel: &super::NodeSel, range: Option<(f64, f64)>) -> Option<Vec<u32>> {
        Some(Ctx { node, ..*self }.overlapping_ids(sel, range))
    }

    fn containing(&self, node: u32, sel: &super::NodeSel) -> Option<Option<u32>> {
        Some(Ctx { node, ..*self }.containing_id(sel))
    }

    fn descendants(&self, node: u32, sel: &super::NodeSel) -> Vec<u32> {
        Ctx { node, ..*self }.descendant_ids(sel)
    }

    fn tree(&self) -> &crate::grammar::Tree {
        self.tree
    }

    fn grammar(&self) -> &Grammar {
        self.grammar
    }

    fn source(&self) -> &str {
        self.source
    }

    fn lines(&self) -> &crate::line_index::LineIndex {
        self.lines
    }

    fn attr(&self, node: u32, attr: u16) -> Option<V> {
        match self.attrs.route(self.tree.node(node).matcher, attr) {
            Route::Missing => Some(V::Null),
            // A forward reads the field's node: nothing, or its one value
            // (several build a list, which the interpreter answers).
            Route::Forward(label) => match self.tree.fields(node, label).as_slice() {
                [] => Some(V::Null),
                [one] => vm::Host::attr(self, *one, attr),
                _ => None,
            },
            Route::Defined(index, slot) => self.vm_attr(node, attr, index, slot),
        }
    }

    fn lang(&self) -> usize {
        self.lang
    }

    fn link(&self, node: u32, link: usize) -> Option<(Vec<u32>, bool)> {
        let targets = self.native_facts().map_or_else(Vec::new, |native| super::facts::follow(native, link, node));
        Some((targets, super::facts::LINKS[link].many))
    }

    fn pattern(&self, test: &CExpr, text: &str) -> Option<bool> {
        Some(self.test_value(test, &Value::from(text)))
    }

    fn interp(&self, node: u32, e: &CExpr, params: Vec<Value<'static>>) -> Option<Value<'_>> {
        let on = Ctx { node, ..*self };
        let mut params: Vec<Value<'_>> = params;
        on.with_params(&mut params, || on.eval(e)).ok()
    }

    fn fact(&self, node: u32, fact: usize, projection: Option<usize>, of: super::FactOf) -> Option<V> {
        // Same reads and memo as the interpreter's (recorded reads included).
        let on = Ctx { node, ..*self };
        let value = on.fact_value(fact, projection, of);
        let key = {
            let (target, wrapper) = match of {
                super::FactOf::Node => (node, None),
                super::FactOf::Parent => (on.parent_layer().unwrap_or(u32::MAX), None),
                super::FactOf::Field(label) => {
                    let targets = self.tree.fields(node, label);
                    match targets.as_slice() {
                        [] => (u32::MAX, Some(label)),
                        [target] => (*target, None),
                        _ => (node, Some(label)),
                    }
                }
            };
            (target, fact, projection, wrapper)
        };
        let packed = self.memo.vm.fact_value(&value, &self.attrs.values, self.source, key, self.attrs.vm_stats);
        if packed.is_none() && self.attrs.vm_stats {
            eprintln!("zlint vm fact: undeclared shape {} ({})", super::facts::FACTS[fact].name, value.type_name());
        }
        packed
    }
}

impl<'a> Ctx<'a> {
    /// An attribute through the VM, from the node's record when it has a
    /// field there (computed on first read); `None` on an error or a cycle
    /// (the interpreter reports it).
    fn vm_attr(&self, node: u32, attr: u16, index: u32, slot: u16) -> Option<V> {
        let lowered = match &self.attrs.attrs[attr as usize].lowered[index as usize] {
            Lowered::Direct(body) => return vm::eval(self, node, body)?.persist(&self.memo.vm),
            Lowered::Cached(body) if slot != NO_SLOT => body,
            // No record field on this kind: memoised beside the records.
            Lowered::Cached(body) => {
                return self.memo.vm.memo(node, attr, || vm::eval(self, node, body).and_then(|v| v.persist(&self.memo.vm)));
            }
        };
        match self.memo.vm.begin(self.tree, &self.attrs.layout, node, slot) {
            Begin::Ready(v) => Some(v),
            Begin::GiveUp => None,
            Begin::Compute(at) => {
                let v = vm::eval(self, node, lowered).and_then(|v| v.persist(&self.memo.vm));
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
                if vm::mode() != vm::Mode::Off {
                    match self.vm_attr(node, attr, index, slot) {
                        Some(v) if vm::mode() == vm::Mode::On => return Ok(v.to_value(self, self.source)),
                        Some(v) => checked = Some(v),
                        // An error or a cycle: the interpreter reports it.
                        None => vm::note_fallback("attribute error"),
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
                if let Some(v) = checked.filter(|v| !v.agrees(&value, self)) {
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
    fn declared_fact_shapes_match_native_values() {
        use crate::expr::facts::{Shape, FACTS};
        use crate::engine::tests::native_linter;
        use crate::native::ts::{facts::NativeFile, modules::Project, syntax::Syntax};
        fn conforms(value: &Value, shape: Shape) -> bool {
            match (value, shape) {
                (Value::Null, _) => true,
                (Value::Record(entries), Shape::Record(fields)) => entries.len() == fields.len()
                    && entries.iter().zip(fields).all(|((key, value), (expected, shape))| &**key == *expected && conforms(value, *shape)),
                (Value::List(items), Shape::List(shape)) => items.iter().all(|value| conforms(value, *shape)),
                (Value::Str(_) | Value::Node(_, _) | Value::Bool(_) | Value::Num(_), Shape::Scalar) => true,
                _ => false,
            }
        }
        let lint = native_linter("rule *");
        let g = &lint.grammar;
        let source = r#"
            const unused = 1; let x = 2; let x = 3;
            object.value; (object?.child).value; run(first, second);
            const obj = { value: value, method: function () {}, get item() { return 1; }, set item(v) {} };
            async function wait() { return 1; }
            function mixed(v) { if (v) return 1; return; }
            values.map(v => { if (v) return v; });
            for (; x;) { run(); }
            x = x; if (Boolean(x)) run(); this.value;
            Object.defineProperty(obj, "item", { get: function () {} });
            /[a  b]/; /a/; eval("value"); value_name;
            try { run(); } catch (error) { throw new Error("failed", { cause: error }); }
        "#;
        let parsed = crate::native::ts::parse(source, false).unwrap();
        let front = &g.natives[0];
        let tree = crate::grammar::Tree::from_native(front.remap(parsed.nodes), parsed.root);
        let project = Project::default();
        let native = NativeFile::new(Syntax::new(&tree, source, front), std::path::Path::new("/root/probe.ts"), &project);
        crate::engine::tests::give_names(&native, &tree, source);
        for (index, fact) in FACTS.iter().enumerate().filter(|(_, fact)| !matches!(fact.shape, Shape::Scalar)) {
            for &node in &tree.order {
                let value = crate::expr::facts::read(&native, index, node);
                assert!(conforms(&value, fact.shape), "fact {} at node {node}: {value:?}", fact.name);
            }
        }
    }

    #[test]
    fn native_fact_text_records_and_lists_cross_the_vm_bridge() {
        use crate::engine::tests::native_linter;
        use crate::native::ts::{facts::NativeFile, modules::Project, syntax::Syntax};
        let lint = native_linter("in ts_member, ts_identifier, ts_call rule probe info \"probe\"");
        let g = &lint.grammar;
        let source = "const unused = 1; object.value; (object?.child).value; run(first, second);";
        let parsed = crate::native::ts::parse(source, false).unwrap();
        let front = &g.natives[0];
        let tree = crate::grammar::Tree::from_native(front.remap(parsed.nodes), parsed.root);
        let project = Project::default();
        let native = NativeFile::new(Syntax::new(&tree, source, front), std::path::Path::new("/root/test.ts"), &project);
        let mut attrs = Attributes::new(vec![], &[], g);
        let reads: Vec<_> = [
            ("static_name", "ts_member"),
            ("token_neighbours", "ts_member"),
            ("optional_chain_risks", "ts_member"),
            ("unused_var", "ts_identifier"),
            ("list_layout", "ts_call"),
        ].into_iter().map(|(name, kind)| {
            let fact = super::super::facts::fact(name).unwrap();
            vm::lower(&CExpr::Fact(fact, super::super::FactOf::Node), &g.functions, &mut attrs.values);
            (fact, g.matcher(kind).unwrap())
        }).collect();
        let argument = front.labels[crate::native::ts::kinds::Field::Arg as usize];
        let yield_fact = super::super::facts::fact("contains_yield").unwrap();
        vm::lower(&CExpr::Fact(yield_fact, super::super::FactOf::Field(argument)), &g.functions, &mut attrs.values);
        let memo = AttrMemo::default();
        let lines = crate::line_index::LineIndex::new(source);
        let reindented = std::cell::RefCell::default();
        let ctx = Ctx {
            grammar: g, lang: 0, tree: &tree, node: tree.root, source, lines: &lines,
            file: "/root/test.ts", options: &[], reindented: &reindented, attrs: &attrs,
            memo: &memo, locals: None, native: Some(&native), indents: &super::super::IndentPlan::EMPTY,
            indent_reset: None, profile: None,
        };
        for (fact, matcher) in reads {
            let mut represented = 0;
            for node in tree.order.iter().copied().filter(|&node| tree.node(node).matcher == matcher) {
                let on = Ctx { node, ..ctx };
                let expected = on.fact_value(fact, None, super::super::FactOf::Node);
                if matches!(expected, Value::Str(_) | Value::Record(_) | Value::List(_)) {
                    let value = vm::Host::fact(&on, node, fact, None, super::super::FactOf::Node)
                        .expect("native values supported by the VM must not give up at the bridge");
                    assert!(value.agrees(&expected, &on));
                    let read = VExpr::Fact { fact, projection: None, of: super::super::FactOf::Node };
                    assert!(vm::eval(&on, node, &read).unwrap().value(&on, source) == expected);
                    assert_eq!(vm::eval(&on, node, &VExpr::Not(Box::new(read.clone()))).unwrap().truthy(&on), !expected.truthy());
                    if matches!(expected, Value::Str(_)) {
                        let equal = VExpr::Binary(crate::config::ast::BinOp::Eq, Box::new(read.clone()), Box::new(VExpr::Const(V::Str("value"))));
                        assert_eq!(vm::eval(&on, node, &equal).unwrap().truthy(&on), expected.loose_eq(&Value::from("value")));
                        let starts = VExpr::StartsWith(Box::new(read), Box::new(VExpr::Const(V::Str("v"))));
                        assert_eq!(vm::eval(&on, node, &starts).unwrap().truthy(&on), expected.text().starts_with('v'));
                    }
                    represented += 1;
                }
            }
            assert!(represented > 0, "fact {} must exercise a non-scalar result", super::super::facts::FACTS[fact].name);
        }
        let call = tree.order.iter().copied().find(|&node| tree.node(node).matcher == g.matcher("ts_call").unwrap()).unwrap();
        let on = Ctx { node: call, ..ctx };
        for fact in [yield_fact, super::super::facts::fact("optional_chain_risks").unwrap()] {
            let of = super::super::FactOf::Field(argument);
            let expected = on.fact_value(fact, None, of);
            assert!(matches!(&expected, Value::List(items) if items.len() == 2), "field reads keep one fact result per argument");
            let value = vm::Host::fact(&on, call, fact, None, of).expect("a multi-field fact read adds a supported list wrapper");
            assert!(value.agrees(&expected, &on));
        }
    }

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
    fn records_hold_only_nontrivial_attributes_read_on_each_kind() {
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
        assert_eq!(plan.shape_records(&g, vec![(on_leaf, &root)]), 1);
        assert_eq!(plan.layout[leaf as usize], 1);
        assert!(matches!(plan.route(leaf, 0), Route::Defined(_, NO_SLOT)));
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
}
