//! Compiled attribute dispatch and reads through the file's existing memo.

use super::{AttrMemo, CExpr, Ctx, Value};
use crate::grammar::{Grammar, ROOT, TEXT};
use crate::grammar::names::question::Known;
use crate::vm::{self, Begin, KindSet, VExpr, V};

/// A `let` attribute: its expression on each layer that defines it.
pub struct Attr {
    pub name: String,
    /// `(layer, body, body id)` in specificity order, resolved by the config
    /// compiler; definitions with one body id hold the same compiled body.
    pub defs: Vec<(u32, CExpr, u32)>,
}

pub struct Attributes {
    pub(crate) lowering_errors: Vec<(String, String)>,
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
    Semantic(VExpr),
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

impl Attributes {
    /// An attribute's name and its definitions' expressions.
    pub fn definition(&self, id: u16) -> Option<(&str, &[CExpr])> {
        self.attrs.get(usize::from(id)).map(|attr| (attr.name.as_str(), attr.defs.as_slice()))
    }

    pub fn count(&self) -> usize { self.attrs.len() }

    pub fn new(attrs: Vec<Attr>, forwards: &[(u32, u16, u16)], grammar: &Grammar) -> Self {
        let stats = vm::stats();
        if attrs.is_empty() {
            return Self { lowering_errors: Vec::new(), vm_stats: stats, values: Default::default(), attrs: Vec::new(), layout: Vec::new() };
        }
        let mut values = vm::Values::default();
        let mut lowering_errors = Vec::new();
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
        let semantic = semantic_attributes(&attrs, &grammar.functions);
        let attrs = attrs.into_iter().zip(by_attr).enumerate().map(|(index, (attr, forwards))| {
            // Each distinct body is lowered once; definitions route to it.
            let (mut bodies, mut lowered, mut body_ids, mut failed) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
            let mut defs: Vec<(u32, u32)> = Vec::new();
            for (matcher, expr, body) in attr.defs {
                if failed.contains(&body) { continue }
                let unique = match body_ids.iter().position(|&known| known == body) {
                    Some(unique) => unique,
                    None => {
                        let lowered_body = vm::lower(&expr, &grammar.functions, &mut values);
                        let errors = values.take_lowering_errors(&format!("attribute `{}`", attr.name));
                        if !errors.is_empty() {
                            lowering_errors.extend(errors.into_iter().map(|error| (attr.name.clone(), error)));
                            failed.push(body);
                            continue;
                        }
                        lowered.push(if semantic[index] { Lowered::Semantic(lowered_body) }
                            else if lowered_body.is_direct() { Lowered::Direct(lowered_body) } else { Lowered::Cached(lowered_body) });
                        bodies.push(expr);
                        body_ids.push(body);
                        bodies.len() - 1
                    }
                };
                defs.push((matcher, unique as u32));
            }
            let mut routes = Vec::new();
            if !defs.is_empty() || !forwards.is_empty() {
                let mut definitions = vec![u32::MAX; grammar.matchers.len()];
                for (index, (matcher, _)) in defs.iter().enumerate() {
                    definitions[*matcher as usize] = definitions[*matcher as usize].min(index as u32);
                }
                let mut forwarding = vec![None; grammar.matchers.len()];
                for (matcher, index, label) in forwards {
                    forwarding[matcher as usize] = Some((index, label));
                }
                routes = ancestors.iter().map(|chain| {
                    let definition = chain.iter().map(|&id| definitions[id]).min().unwrap_or(u32::MAX);
                    if definition != u32::MAX { return Route::Defined(defs[definition as usize].1, NO_SLOT); }
                    // Forwards keep declaration order, not nearest-ancestor order.
                    chain.iter().filter_map(|&id| forwarding[id]).min_by_key(|(index, _)| *index)
                        .map_or(Route::Missing, |(_, label)| Route::Forward(label))
                }).collect();
            }
            CompiledAttr {
                name: attr.name,
                defs: bodies,
                lowered,
                routes,
            }
        }).collect();
        let layout = vec![0; grammar.matchers.len()];
        Self { lowering_errors, vm_stats: stats, values, attrs, layout }
    }

    pub(crate) fn report_vm_stats(&self, memo: &AttrMemo<'_>, tree: &crate::grammar::Tree, file: &str) {
        if !self.vm_stats { return; }
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

/// Which attributes ask semantic questions, directly or through the
/// attributes and `fn`s they read: one walk records each body's direct
/// questions and reads, then askers spread to their readers.
fn semantic_attributes(attrs: &[Attr], functions: &[crate::grammar::UserFn]) -> Vec<bool> {
    struct Graph {
        attrs: usize,
        asks: Vec<bool>,
        readers: Vec<Vec<usize>>,
    }
    impl Graph {
        fn scan(&mut self, expr: &CExpr, reader: usize) {
            match expr {
                CExpr::Trait(..) | CExpr::QueryCall(..) => self.asks[reader] = true,
                CExpr::Attr(id) | CExpr::FieldAttr(_, id) | CExpr::ParentAttr(id) => self.readers[*id as usize].push(reader),
                CExpr::UserCall(id, _) => self.readers[self.attrs + *id as usize].push(reader),
                _ => {}
            }
            expr.for_each_child(&mut |child| self.scan(child, reader));
        }
    }
    let nodes = attrs.len() + functions.len();
    let mut graph = Graph { attrs: attrs.len(), asks: vec![false; nodes], readers: vec![Vec::new(); nodes] };
    for (index, attr) in attrs.iter().enumerate() {
        let mut scanned = Vec::new();
        for (_, body, id) in &attr.defs {
            if !scanned.contains(id) {
                scanned.push(*id);
                graph.scan(body, index);
            }
        }
    }
    for (index, function) in functions.iter().enumerate() {
        if let Some(body) = function.body.get() { graph.scan(body, attrs.len() + index); }
    }
    let Graph { mut asks, readers, .. } = graph;
    let mut pending: Vec<usize> = (0..nodes).filter(|&node| asks[node]).collect();
    while let Some(node) = pending.pop() {
        for &reader in &readers[node] {
            if !asks[reader] {
                asks[reader] = true;
                pending.push(reader);
            }
        }
    }
    asks.truncate(attrs.len());
    asks
}

/// Which kinds each attribute can be read on ([`Attributes::shape_records`]).
struct Needs<'e> {
    grammar: &'e Grammar,
    attrs: &'e Attributes,
    all: KindSet,
    need: Vec<KindSet>,
    called: Vec<KindSet>,
    pending: Vec<(KindSet, &'e CExpr)>,
    /// Expressions already visited on every kind.
    everywhere: crate::fx_hash::FxHashSet<*const CExpr>,
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
            everywhere: Default::default(),
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

    /// `e` read on any kind: once per expression, every later visit is
    /// covered by the first.
    fn everywhere(&mut self, e: &'e CExpr) {
        if self.everywhere.insert(e as *const CExpr) {
            self.pending.push((self.all.clone(), e));
        }
    }

    /// `attr` is read on `kinds`: its definitions there are evaluated too.
    fn request(&mut self, attr: u16, kinds: &KindSet) {
        let attrs = self.attrs;
        let Some(compiled) = attrs.attrs.get(attr as usize) else { return };
        // Each (attribute, kind) is followed once; forwarding kinds too,
        // since forwards can chain through themselves.
        let fresh = kinds.without(&self.need[attr as usize]);
        if fresh.is_empty() { return }
        self.need[attr as usize].extend(&fresh);
        let mut by_def: Vec<(u32, KindSet)> = Vec::new();
        for k in fresh.iter() {
            match compiled.routes.get(k as usize) {
                Some(Route::Defined(def, _)) => {
                    if matches!(compiled.lowered[*def as usize], Lowered::Cached(_) | Lowered::Semantic(_)) {
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
                    self.everywhere(part);
                }
            }
            CExpr::Path(base, _, parts) => {
                self.visit(kinds, base);
                for part in parts.iter().flatten() {
                    self.everywhere(part);
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
    fn query(&self, node: u32, subject: &crate::grammar::names::Value, query: &super::queries::Query, mask: u64) -> Option<bool> {
        Some(self.world?.query(self.file_id, node, subject, query, mask))
    }

    fn query_function(&self, node: u32, query: &super::queries::QueryFunction, args: &[Value<'static>]) -> Option<Value<'static>> {
        Some(self.world?.query_function(self.file_id, node, query, args))
    }

    fn query_handle(&self, handle: crate::grammar::names::question::Handle, query: &super::queries::HandleFunction, args: &[Value<'static>]) -> Option<Value<'static>> {
        Some(self.world?.query_handle(self.file_id, handle, query, args))
    }

    fn values(&self) -> &vm::Values {
        &self.attrs.values
    }

    fn file_vm(&self) -> Option<&vm::FileVm> {
        Some(&self.memo.vm)
    }

    fn builtin(&self, node: u32, builtin: super::Builtin) -> Option<Value<'_>> {
        match (Ctx { node, ..*self }).builtin(builtin) {
            Ok(value) => Some(value),
            Err(error) => { self.memo.vm.fail(error); None }
        }
    }

    fn option(&self, index: usize) -> Option<Value<'_>> { self.options.get(index).cloned() }

    fn debug(&self, node: u32, label: &str, value: &Value) {
        let start = self.tree.node(node).start as usize;
        let (line, col) = self.lines.line_col(self.source, start);
        let layer = match self.tree.node(node).matcher {
            ROOT | TEXT => "",
            matcher => self.grammar.matchers[matcher as usize].name.as_str(),
        };
        eprintln!("{}:{}:{} {layer} {label} = {}", self.file, line + 1, col + 1, super::show(value));
    }

    fn position(&self, offset: f64) -> Option<super::Position> {
        Some(self.position_at(offset))
    }

    fn tokens(&self) -> Option<std::rc::Rc<crate::engine::frontend::Tokens>> {
        self.native?.tokens()
    }

    fn overlapping(&self, node: u32, sel: &super::NodeSel, range: Option<(f64, f64)>) -> Option<Vec<u32>> {
        Some(Ctx { node, ..*self }.overlapping_ids(sel, range))
    }

    fn containing(&self, node: u32, sel: &super::NodeSel) -> Option<Option<u32>> {
        Some(Ctx { node, ..*self }.containing_id(sel))
    }

    fn descendants<'s>(&'s self, node: u32, sel: &super::NodeSel) -> impl Iterator<Item = u32> + 's {
        self.descendant_ids(node, sel)
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
            Route::Forward(label) => match self.tree.fields(node, label).as_slice() {
                [] => Some(V::Null),
                [one] => vm::Host::attr(self, *one, attr),
                many => {
                    let items = many.iter()
                        .map(|&id| vm::Host::attr(self, id, attr).map(|value| value.to_value(self, self.source)))
                        .collect::<Option<Vec<_>>>()?;
                    self.memo.vm.pack(&Value::list(items), &self.attrs.values, self.source)
                }
            },
            Route::Defined(index, slot) => self.vm_attr(node, attr, index, slot),
        }
    }

    fn lang(&self) -> usize {
        self.lang
    }

    fn pattern(&self, test: &CExpr, text: &str) -> Option<bool> {
        Some(self.test_value(test, &Value::from(text)))
    }

    fn fact(&self, node: u32, fact: usize, of: super::FactOf) -> Option<V> {
        let on = Ctx { node, ..*self };
        let value = on.fact_value(fact, of);
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
            (target, fact, wrapper)
        };
        let packed = self.memo.vm.fact_value(&value, &self.attrs.values, self.source, key, self.attrs.vm_stats);
        if packed.is_none() && self.attrs.vm_stats {
            eprintln!("zlint vm fact: undeclared shape {} ({})", super::facts::FACTS[fact].name, value.type_name());
        }
        packed
    }

    fn fact_call(&self, node: u32, fact: usize, args: Vec<Value<'static>>) -> Option<V> {
        let Some(native) = self.native_facts() else { return Some(V::Null) };
        let compute = || super::facts::with_args(args, || super::facts::read(native, fact, node));
        let value = match self.profile {
            Some(profile) => profile.time_fact(fact, compute),
            None => compute(),
        };
        self.memo.vm.pack(&value, &self.attrs.values, self.source)
    }

    fn facts_available(&self) -> bool { self.native_facts().is_some() }

    fn local(&self, node: u32, local: u16) -> Option<V> {
        match (Ctx { node, ..*self }).local(local) {
            Ok(value) => self.memo.vm.pack(&value, &self.attrs.values, self.source),
            Err(error) => { self.memo.vm.fail(error); None }
        }
    }
}

impl<'a> Ctx<'a> {
    /// An attribute through the VM, from the node's record when it has a
    /// field there (computed on first read); `None` on an error or a cycle.
    fn vm_attr(&self, node: u32, attr: u16, index: u32, slot: u16) -> Option<V> {
        let _timed = self.profile.and_then(|profile| profile.begin_attribute(usize::from(attr)));
        let (lowered, world) = match &self.attrs.attrs[attr as usize].lowered[index as usize] {
            Lowered::Direct(body) => return vm::eval(self, node, body)?.persist(&self.memo.vm),
            Lowered::Semantic(body) => match self.world.filter(|world| world.cache_attribute(&self.memo.vm)) {
                Some(world) => (body, Some(world)),
                None => return self.memo.vm.uncached(node, attr, || vm::eval(self, node, body)?.persist(&self.memo.vm)),
            },
            Lowered::Cached(body) => (body, self.world),
        };
        let mut evaluated = None;
        let mut complete = true;
        let mut compute = || {
            let mut run = || {
                evaluated = vm::eval(self, node, lowered).and_then(|v| v.persist(&self.memo.vm));
                evaluated
            };
            let answer = match world {
                Some(world) => world.complete_attribute(run),
                None => Known::Yes(run()),
            };
            complete = matches!(answer, Known::Yes(_));
            answer
        };
        if slot == NO_SLOT {
            let kept = self.memo.vm.memo(node, attr, compute);
            let value = kept.or(evaluated);
            if value.is_none() && complete {
                self.memo.vm.attr_failure(node, attr, &self.attrs.attrs[attr as usize].name);
                self.memo.vm.remember_attr_failure(node, attr);
            }
            return value;
        }
        match self.memo.vm.begin(self.tree, &self.attrs.layout, node, slot) {
            Begin::Ready(v) => Some(v),
            Begin::GiveUp => {
                self.memo.vm.attr_failure(node, attr, &self.attrs.attrs[attr as usize].name);
                None
            }
            Begin::Compute(at) => {
                let answer = compute();
                self.memo.vm.finish(at, answer);
                if evaluated.is_none() && complete { self.memo.vm.remember_attr_failure(node, attr); }
                evaluated
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
        use crate::engine::tests::native_linter_parts;
        use crate::native::ts::{facts::NativeFile, modules::Project, syntax::Syntax};
        let (grammar, mut attrs) = native_linter_parts("in ts_member, ts_identifier, ts_call rule probe info \"probe\"");
        let g = &grammar;
        let source = "const unused = 1; object.value; (object?.child).value; run(first, second);";
        let parsed = crate::native::ts::parse(source, false).unwrap();
        let front = &g.natives[0];
        let tree = crate::grammar::Tree::from_native(front.remap(parsed.nodes), parsed.root);
        let project = Project::default();
        let native = NativeFile::new(Syntax::new(&tree, source, front), std::path::Path::new("/root/test.ts"), &project);
        let reads: Vec<_> = [
            ("symbol_metadata", "ts_identifier"),
        ].into_iter().map(|(name, kind)| {
            let fact = super::super::facts::fact(name).unwrap();
            vm::lower(&CExpr::Fact(fact, super::super::FactOf::Node), &g.functions, &mut attrs.values);
            (fact, g.matcher(kind).unwrap())
        }).collect();
        let argument = front.labels[crate::native::ts::kinds::Field::Arg as usize];
        let field_fact = super::super::facts::fact("symbol_metadata").unwrap();
        vm::lower(&CExpr::Fact(field_fact, super::super::FactOf::Field(argument)), &g.functions, &mut attrs.values);
        let memo = AttrMemo::default();
        let lines = crate::line_index::LineIndex::new(source);
        let reindented = std::cell::RefCell::default();
        let ctx = Ctx {
            grammar: g, lang: 0, tree: &tree, node: tree.root, source, lines: &lines,
            file: "/root/test.ts", file_id: 0, options: &[], reindented: &reindented, attrs: &attrs,
            memo: &memo, locals: None, native: Some(&native), indents: &super::super::IndentPlan::EMPTY,
            indent_reset: None, profile: None, world: None, language_options: None,
        };
        for (fact, matcher) in reads {
            let mut represented = 0;
            for node in tree.order.iter().copied().filter(|&node| tree.node(node).matcher == matcher) {
                let on = Ctx { node, ..ctx };
                let expected = on.fact_value(fact, super::super::FactOf::Node);
                if matches!(expected, Value::Str(_) | Value::Record(_) | Value::List(_)) {
                    let value = vm::Host::fact(&on, node, fact, super::super::FactOf::Node)
                        .expect("native values supported by the VM must not give up at the bridge");
                    assert_eq!(value.to_value(&on, source), expected);
                    let read = VExpr::Fact { fact, of: super::super::FactOf::Node };
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
        for fact in [field_fact, super::super::facts::fact("type").unwrap()] {
            let of = super::super::FactOf::Field(argument);
            let expected = on.fact_value(fact, of);
            assert!(matches!(&expected, Value::List(items) if items.len() == 2), "field reads keep one fact result per argument");
            let value = vm::Host::fact(&on, call, fact, of).expect("a multi-field fact read adds a supported list wrapper");
            assert_eq!(value.to_value(&on, source), expected);
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
            Attr { name: "specific".into(), defs: vec![(member, number(2.0), 1), (base, number(1.0), 2)] },
            Attr { name: "fallback".into(), defs: vec![(base, number(3.0), 3)] },
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
            Attr { name: "read".into(), defs: vec![(base, number(1.0), 1)] },
            Attr { name: "unread".into(), defs: vec![(base, number(2.0), 2)] },
            Attr { name: "chained".into(), defs: vec![(base, CExpr::Attr(0), 3)] },
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
