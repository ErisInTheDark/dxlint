//! Values: `unwraps`, `fits`, `could` / `always`, `through` generics
//! (docs/design/type-layers.md). Owner: Violet.

use super::memo::{Epoch, Memo};
use super::question::{Known, Leaf, NodeRef, NodeValue, Target};
use super::semantic::Semantic;
use super::Value;

pub mod compile;
mod algebra;
mod carriers;
mod conditional;
mod eval;
mod indexed;
mod keys;
mod members;
mod metadata;
mod modified;
pub mod products;
mod reduction;

pub(super) fn algebra_traits(
    traits: &[crate::config::ast::Name],
    vocabulary: &super::Vocabulary,
    names: &super::Names,
    errors: &mut Vec<crate::config::error::ConfigError>,
) -> u64 {
    let fitted = fitted_traits(names);
    let mut mask = 0;
    for name in traits {
        let bit = vocabulary.trait_bit(&name.text);
        if fitted & bit == 0 {
            errors.push(crate::config::error::ConfigError::new(name.span, format!("nothing fits trait `{}`", name.text)));
        } else {
            mask |= bit;
        }
    }
    mask
}

pub(super) fn exclusive_groups(
    config: &crate::config::ast::Config,
    vocabulary: &mut super::Vocabulary,
    names: &super::Names,
    errors: &mut Vec<crate::config::error::ConfigError>,
) -> Vec<u64> {
    use crate::config::error::ConfigError;
    let fitted = fitted_traits(names);
    let mut groups = Vec::new();
    for group in &config.exclusive {
        let Some(bits) = super::class_bits(&mut vocabulary.traits, &group.traits, errors) else { continue };
        let mut valid = true;
        for name in &group.traits {
            let bit = vocabulary.traits.iter().position(|trait_name| trait_name == &name.text).map_or(0, |index| 1 << index);
            if fitted & bit == 0 {
                errors.push(ConfigError::new(name.span, format!("nothing fits trait `{}`", name.text))
                    .suggest(&name.text, vocabulary.traits.iter().enumerate()
                        .filter(|(index, _)| fitted & (1 << index) != 0)
                        .map(|(_, name)| name.as_str())));
                valid = false;
            }
        }
        if bits.count_ones() < 2 {
            errors.push(ConfigError::new(group.span, "`exclusive` needs at least two distinct traits"));
            valid = false;
        }
        if valid { groups.push(bits) }
    }
    groups
}

fn fitted_traits(names: &super::Names) -> u64 {
    let ordinary = names.kinds.iter().flat_map(|kind| &kind.fits).fold(0, |all, &(bits, _, _)| all | bits);
    names.products.iter().flatten().fold(ordinary, |all, shape| all | shape.traits)
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Key {
    Text(Box<str>),
    Ordinal(u32),
    Symbol(NodeRef),
    Domain(crate::config::ast::values::KeyDomain),
}

impl Key {
    pub fn may_alias(&self, other: &Self) -> bool {
        self == other || matches!((self, other),
            (Self::Text(_), Self::Ordinal(_)) | (Self::Ordinal(_), Self::Text(_)))
            || (matches!(self, Self::Domain(_)) || matches!(other, Self::Domain(_)))
                && (self.domain() == other.domain()
                    || matches!((self.domain(), other.domain()),
                        (crate::config::ast::values::KeyDomain::Text, crate::config::ast::values::KeyDomain::Ordinal)
                        | (crate::config::ast::values::KeyDomain::Ordinal, crate::config::ast::values::KeyDomain::Text)))
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Identity {
    Value(crate::expr::Value<'static>),
    Declaration(NodeRef),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Member {
    pub key: Key,
    pub sources: Vec<MemberSource>,
    pub required: Known<bool>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MemberSource {
    pub site: Leaf,
    pub value: Option<Value>,
    pub origin: MemberOrigin,
    /// Declaration flags changed by the modified value it was read through.
    pub edits: Option<super::question::ModifiedId>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MemberOrigin {
    Named,
    Ordinal { repeated: bool },
    Indexer(crate::config::ast::values::KeyDomain),
}

impl MemberSource {
    fn normal(site: Leaf) -> Self {
        Self { site, value: None, origin: MemberOrigin::Named, edits: None }
    }
}

#[derive(Clone, Debug)]
pub struct CarrierRecipe {
    pub description: u32,
    pub arguments: Option<Vec<u16>>,
    pub value: Option<Value>,
    pub when: Option<u32>,
}

#[derive(Clone, Debug)]
pub struct IndexedRecipe {
    pub path: Vec<u16>,
    pub mode: IndexedMode,
    pub views: u64,
    pub value: Option<Value>,
    pub when: Option<u32>,
}

#[derive(Clone, Debug)]
pub enum IndexedMode {
    Slots { repeated: bool, strings: bool },
    Domain(crate::config::ast::values::KeyDomain),
    Keyed(Value),
    Keys { bind: Option<Vec<u16>> },
}

#[derive(Clone, Debug, PartialEq)]
pub struct Carrier {
    pub declaration: NodeRef,
    pub description: crate::expr::Value<'static>,
    pub arguments: Vec<NodeValue>,
}

#[derive(Default)]
pub(crate) struct State {
    values: Memo<Leaf, NodeValue>,
    /// Leaves whose values are being evaluated (outermost first), each with
    /// [`State::cycle_reads`] when it started.
    evaluating: std::cell::RefCell<Vec<(Leaf, u64)>>,
    /// Reads of a leaf still being evaluated (its seed). An unknown value
    /// computed after such a read may only reflect the unfinished cycle, so
    /// it is not kept.
    cycle_reads: std::cell::Cell<u64>,
    /// Entries of the `Known` memos being computed ([`Values::memoized`]),
    /// by memo and key hash, innermost last.
    pending: std::cell::RefCell<Vec<(usize, u64)>>,
    widened: Memo<Leaf, NodeValue>,
    traits: Memo<(Leaf, u16), Known<bool>>,
    optional: Memo<Leaf, Known<bool>>,
    identities: Memo<Leaf, Known<Option<Identity>>>,
    members: Memo<Leaf, Known<Vec<Member>>>,
    member_keys: Memo<(Leaf, Key), Known<Option<Member>>>,
    key_paths: Memo<(Leaf, Vec<u16>), Key>,
    member_closed: Memo<Leaf, Known<bool>>,
    bound_arguments: Memo<Leaf, Known<Option<NodeValue>>>,
    carriers: Memo<Leaf, Known<Option<Carrier>>>,
}

impl State {
    pub(super) fn finish(&self, epoch: Epoch, failed: bool, cut: u64) -> u64 {
        use super::memo::Settled;
        self.values.finish(epoch, failed, cut, Settled::settled) + self.widened.finish(epoch, failed, cut, Settled::settled)
            + self.traits.finish(epoch, failed, cut, Settled::settled) + self.optional.finish(epoch, failed, cut, Settled::settled)
            + self.identities.finish(epoch, failed, cut, Settled::settled) + self.members.finish(epoch, failed, cut, Settled::settled)
            + self.member_keys.finish(epoch, failed, cut, Settled::settled) + self.key_paths.finish(epoch, failed, cut, |_| false)
            + self.member_closed.finish(epoch, failed, cut, Settled::settled) + self.bound_arguments.finish(epoch, failed, cut, Settled::settled)
            + self.carriers.finish(epoch, failed, cut, Settled::settled)
    }
}

pub struct Values<'a> {
    pub(crate) semantic: &'a Semantic<'a>,
}

impl<'a> std::ops::Deref for Values<'a> {
    type Target = Semantic<'a>;

    fn deref(&self) -> &Self::Target {
        self.semantic
    }
}

impl Values<'_> {
    pub fn values(&self, leaf: Leaf) -> NodeValue {
        eval::values(self, leaf)
    }

    pub fn unwrap_recipe(&self, leaf: Leaf) -> Known<Option<&Value>> {
        let Known::Yes(leaf) = self.semantic.leaf(leaf) else { return Known::Unknown };
        let Target::Node(node) = leaf.target else { return Known::Yes(None) };
        let Some(tree) = self.world.tree(node.file) else { return Known::Unknown };
        let Some(data) = tree.nodes.get(node.node as usize) else { return Known::Unknown };
        let Some(kind) = self.tables.kinds.get(data.matcher as usize) else { return Known::Yes(None) };
        let classes = self.question.get(leaf.frame).filter(|frame| frame.owner == node).map_or(0, |frame| frame.classes);
        let unwraps = kind.unwraps.iter().filter(|unwrap| unwrap.classes & classes != 0)
            .chain(kind.unwraps.iter().filter(|unwrap| unwrap.classes == 0));
        for unwrap in unwraps {
            if unwrap.effect.is_some() { continue }
            match unwrap.when.map_or(Known::Yes(true), |condition| self.world.condition(leaf, condition, self.semantic)) {
                Known::Yes(false) => continue,
                Known::Unknown => return Known::Unknown,
                Known::Yes(true) => return unwrap.value.as_ref().map_or(Known::Unknown, |value| Known::Yes(Some(value))),
            }
        }
        Known::Yes(None)
    }

    /// Records that a question read something still being decided further
    /// up (a cycle outside the memos here: item conditions, attributes), so
    /// unknown answers computed meanwhile are not kept.
    pub fn note_cycle_read(&self) {
        let state = &self.semantic.values;
        state.cycle_reads.set(state.cycle_reads.get() + 1);
    }

    /// The cycle-read count, to tell later whether a computation read
    /// anything still undecided ([`Self::cycled_since`]).
    pub(super) fn cycle_mark(&self) -> u64 {
        self.semantic.values.cycle_reads.get()
    }

    pub(super) fn cycled_since(&self, mark: u64) -> bool {
        self.semantic.values.cycle_reads.get() > mark
    }

    /// A `Known` memo entry: while `compute` runs, the key reads as unknown
    /// (cutting recursion) and such a read counts as a cycle read. An
    /// unknown answer computed after a cycle read is not kept: it may only
    /// reflect the unfinished cycle, and keeping it would make later answers
    /// depend on which question happened to reach the cycle first.
    pub(super) fn memoized<K, T>(&self, memo: &Memo<K, Known<T>>, key: K, compute: impl FnOnce() -> Known<T>) -> Known<T>
    where K: Clone + Eq + std::hash::Hash, T: Clone {
        use std::hash::BuildHasher;
        let state = &self.semantic.values;
        let tag = (memo as *const Memo<K, Known<T>> as usize, crate::fx_hash::FxBuild::default().hash_one(&key));
        if let Some(answer) = memo.get(&key) {
            if matches!(answer, Known::Unknown) && state.pending.borrow().contains(&tag) {
                state.cycle_reads.set(state.cycle_reads.get() + 1);
            }
            return answer;
        }
        memo.insert(self.semantic.epoch, key.clone(), Known::Unknown);
        state.pending.borrow_mut().push(tag);
        let started = state.cycle_reads.get();
        let answer = compute();
        state.pending.borrow_mut().pop();
        if matches!(answer, Known::Unknown) && state.cycle_reads.get() > started {
            memo.remove(self.semantic.epoch, &key);
        } else {
            memo.insert(self.semantic.epoch, key, answer.clone());
        }
        answer
    }

    pub fn fits(&self, leaf: Leaf, trait_id: u16) -> Known<bool> {
        let Known::Yes(leaf) = self.semantic.leaf(leaf) else { return Known::Unknown };
        let key = (leaf, trait_id);
        if self.semantic.values.traits.get(&key).is_none() && (trait_id >= 64 || !self.question.budget.step()) {
            return Known::Unknown;
        }
        self.memoized(&self.semantic.values.traits, key, || self.direct_trait(leaf, trait_id))
    }

    fn direct_trait(&self, leaf: Leaf, trait_id: u16) -> Known<bool> {
        match leaf.target {
            Target::Traits(bits) => Known::Yes(bits & (1 << trait_id) != 0),
            Target::Product(_) => self.product_fits(leaf, trait_id),
            Target::Modified(_) => match self.modified_source(leaf) {
                Some(source) => self.fits(source, trait_id),
                None => Known::Unknown,
            },
            Target::Scope(scope) => match self.world.names(scope.file)
                .and_then(|names| names.scopes.get(scope.scope as usize)) {
                Some(info) => self.direct_fits(leaf, NodeRef { file: scope.file, node: info.node }, trait_id, true),
                None => Known::Unknown,
            },
            Target::Node(node) => self.direct_fits(leaf, node, trait_id, true),
        }
    }

    /// Whether a node site's own kind declares `trait_id`, ignoring `open`
    /// fits: for traits naming a site's shape (a spread argument) rather
    /// than its value, which an open value (`any`) leaves undecided.
    pub fn declared_fits(&self, leaf: Leaf, trait_id: u16) -> Known<bool> {
        let Known::Yes(leaf) = self.semantic.leaf(leaf) else { return Known::Unknown };
        match leaf.target {
            Target::Node(node) if trait_id < 64 => self.direct_fits(leaf, node, trait_id, false),
            _ => self.fits(leaf, trait_id),
        }
    }

    fn direct_fits(&self, leaf: Leaf, node: NodeRef, trait_id: u16, open: bool) -> Known<bool> {
        let Some(tree) = self.world.tree(node.file) else { return Known::Unknown };
        let Some(node_data) = tree.nodes.get(node.node as usize) else { return Known::Unknown };
        let Some(kind) = self.tables.kinds.get(node_data.matcher as usize) else { return Known::Yes(false) };
        let mut unknown = false;
        for &(bits, views, when) in &kind.fits {
            if !self.question.budget.step() {
                return Known::Unknown;
            }
            if bits & (1 << trait_id) == 0 {
                continue;
            }
            if views != 0 {
                let Target::Scope(scope) = leaf.target else { continue };
                let Some(info) = self.world.names(scope.file).and_then(|names| names.scopes.get(scope.scope as usize)) else {
                    return Known::Unknown;
                };
                if info.classes & views == 0 { continue }
            }
            match when.map_or(Known::Yes(true), |condition| self.world.condition(leaf, condition, self.semantic)) {
                Known::Yes(true) => return Known::Yes(true),
                Known::Unknown => unknown = true,
                Known::Yes(false) => {}
            }
        }
        for &(views, when) in kind.open_fits.iter().filter(|_| open) {
            if !self.question.budget.step() { return Known::Unknown }
            if views != 0 {
                let Target::Scope(scope) = leaf.target else { continue };
                let Some(info) = self.world.names(scope.file).and_then(|names| names.scopes.get(scope.scope as usize)) else {
                    return Known::Unknown;
                };
                if info.classes & views == 0 { continue }
            }
            if !matches!(when.map_or(Known::Yes(true), |condition| self.world.condition(leaf, condition, self.semantic)),
                Known::Yes(false)) {
                unknown = true;
            }
        }
        if unknown { Known::Unknown } else { Known::Yes(false) }
    }

    pub fn could(&self, value: &NodeValue, traits: u64) -> Known<bool> {
        self.quantify(value, traits, false)
    }

    pub fn always(&self, value: &NodeValue, traits: u64) -> Known<bool> {
        self.quantify(value, traits, true)
    }

    fn quantify(&self, value: &NodeValue, traits: u64, every: bool) -> Known<bool> {
        let NodeValue::Alternatives(alternatives) = value else { return Known::Unknown };
        let mut unknown = false;
        for alternative in alternatives {
            if !self.question.budget.step() {
                return Known::Unknown;
            }
            let mut complete = true;
            let mut uncertain = false;
            for trait_id in 0..64 {
                if traits & (1 << trait_id) == 0 {
                    continue;
                }
                let mut fitted = false;
                let mut maybe = false;
                for &leaf in alternative {
                    if !self.question.budget.step() {
                        return Known::Unknown;
                    }
                    match self.fits(leaf, trait_id) {
                        Known::Yes(true) => { fitted = true; break }
                        Known::Unknown => maybe = true,
                        Known::Yes(false) => {}
                    }
                }
                if !fitted {
                    complete = false;
                    if maybe {
                        uncertain = true;
                    } else {
                        uncertain = false;
                        break;
                    }
                }
            }
            if every && !complete && !uncertain {
                return Known::Yes(false);
            }
            if !every && complete {
                return Known::Yes(true);
            }
            unknown |= uncertain;
        }
        if unknown { Known::Unknown } else { Known::Yes(every) }
    }

    pub fn evaluate(&self, at: Leaf, expression: &Value) -> NodeValue {
        eval::evaluate(self, at, expression)
    }

    pub(crate) fn evaluate_optional(&self, at: Leaf, expression: &Value) -> Known<Option<NodeValue>> {
        eval::evaluate_optional(self, at, expression)
    }

    pub(crate) fn combine(&self, inputs: impl IntoIterator<Item = NodeValue>, intersection: bool) -> NodeValue {
        eval::combine(self, inputs, intersection)
    }

    pub fn key(&self, at: Leaf, expression: &Value) -> Known<Key> {
        let Known::Yes(at) = self.semantic.leaf(at) else { return Known::Unknown };
        let request = match expression {
            Value::Path(path) => Some((at, path.clone())),
            _ => None,
        };
        if let Some(key) = request.as_ref().and_then(|request| self.semantic.values.key_paths.get(request)) {
            return Known::Yes(key);
        }
        let answer = self.direct_key(at, expression);
        if let (Some(request), Known::Yes(key)) = (request, &answer) {
            self.semantic.values.key_paths.insert(self.semantic.epoch, request, key.clone());
        }
        answer
    }

    fn direct_key(&self, at: Leaf, expression: &Value) -> Known<Key> {
        if !self.question.budget.step() {
            return Known::Unknown;
        }
        match expression {
            Value::Text(text) => Known::Yes(Key::Text(text.clone())),
            Value::Ordinal(index) => Known::Yes(Key::Ordinal(*index)),
            Value::Path(path) => {
                let mut answer = None;
                if let Some((&super::POSITION, prefix)) = path.split_last() {
                    let Known::Yes(sites) = self.follow(at, prefix) else { return Known::Unknown };
                    for site in sites {
                        if !self.question.budget.step() { return Known::Unknown }
                        let Known::Yes(Some(index)) = self.semantic.position(site) else { return Known::Unknown };
                        let Ok(position) = u32::try_from(index) else {
                            return Known::Unknown;
                        };
                        let key = Key::Ordinal(position);
                        if answer.as_ref().is_some_and(|previous| previous != &key) { return Known::Unknown }
                        answer = Some(key);
                    }
                } else if let Some((attribute, prefix)) = path.split_last()
                    .and_then(|(&last, prefix)| super::attribute_step(last).map(|attribute| (attribute, prefix))) {
                    let Known::Yes(sites) = self.follow(at, prefix) else { return Known::Unknown };
                    for site in sites {
                        if !self.question.budget.step() { return Known::Unknown }
                        let Known::Yes(value) = self.world.attribute(site, attribute, self.semantic) else { return Known::Unknown };
                        // A node-valued key (a computed `[key]`) is that
                        // expression's value, seen from the same frame.
                        let key = match (&value, site.target) {
                            (crate::expr::Value::Node(node, _), Target::Node(owner)) => {
                                let leaf = Leaf { target: Target::Node(NodeRef { file: owner.file, node: *node }), frame: site.frame };
                                let Known::Yes(key) = self.value_key(&self.values(leaf)) else { return Known::Unknown };
                                key
                            }
                            _ => {
                                let Some(key) = Self::expression_key(value) else { return Known::Unknown };
                                key
                            }
                        };
                        if answer.as_ref().is_some_and(|previous| previous != &key) { return Known::Unknown }
                        answer = Some(key);
                    }
                } else {
                    return self.value_key(&self.evaluate(at, expression));
                }
                answer.map_or(Known::Unknown, Known::Yes)
            }
            // Any other recipe (`from instance "Symbol" ... at "iterator"`)
            // keys by the value it evaluates to.
            _ => self.value_key(&self.evaluate(at, expression)),
        }
    }

    fn expression_key(value: crate::expr::Value<'static>) -> Option<Key> {
        match value {
            crate::expr::Value::Str(text) => Some(Key::Text((&*text).into())),
            crate::expr::Value::Num(number) if number >= 0.0 && number <= u32::MAX as f64 && number.fract() == 0.0 => {
                Some(Key::Ordinal(number as u32))
            }
            _ => None,
        }
    }

    pub fn sites(&self, at: Leaf, expression: &Value) -> Known<Vec<Leaf>> {
        eval::sites(self, at, expression)
    }

    fn scopes(&self, at: Leaf, from: &Value, classes: u64) -> Known<Vec<Leaf>> {
        use super::question::ScopeRef;
        let Some(_nested) = self.question.budget.nest() else { return Known::Unknown };
        let Known::Yes(sites) = self.sites(at, from) else { return Known::Unknown };
        let mut leaves = Vec::new();
        for site in sites {
            if !self.question.budget.step() { return Known::Unknown }
            let node = match site.target {
                Target::Traits(_) | Target::Product(_) | Target::Modified(_) => return Known::Unknown,
                Target::Node(node) => node,
                Target::Scope(scope) => {
                    let Some(info) = self.world.names(scope.file).and_then(|names| names.scopes.get(scope.scope as usize)) else {
                        return Known::Unknown;
                    };
                    NodeRef { file: scope.file, node: info.node }
                }
            };
            let Some(names) = self.world.names(node.file) else { return Known::Unknown };
            let Some(tree) = self.world.tree(node.file) else { return Known::Unknown };
            let Some(text) = self.world.text(node.file) else { return Known::Unknown };
            for scope in names.index(tree, text, &|text| self.world.name_key(node.file, text)).opened(node.node) {
                if !self.question.budget.step() { return Known::Unknown }
                if names.scopes[scope as usize].classes & classes == 0 { continue }
                let leaf = Leaf { target: Target::Scope(ScopeRef { file: node.file, scope }), frame: site.frame };
                if !leaves.contains(&leaf) { leaves.push(leaf) }
            }
        }
        Known::Yes(leaves)
    }

    pub fn arguments(&self, at: Leaf, path: &[u16]) -> Known<Vec<NodeValue>> {
        self.arguments_using(at, path, None)
    }

    fn arguments_using(&self, at: Leaf, path: &[u16], value: Option<&Value>) -> Known<Vec<NodeValue>> {
        let sites = {
            let Some(_nested) = self.question.budget.nest() else { return Known::Unknown };
            if path == [super::PRODUCT_ITEMS] {
                return if value.is_none() { self.product_items(at) } else { Known::Unknown };
            }
            let Known::Yes(sites) = self.follow(at, path) else { return Known::Unknown };
            sites
        };
        self.argument_values(sites, value)
    }

    fn argument_values(&self, sites: Vec<Leaf>, value: Option<&Value>) -> Known<Vec<NodeValue>> {
        let Some(_nested) = self.question.budget.nest() else { return Known::Unknown };
        let mut arguments = Vec::with_capacity(sites.len());
        for site in sites {
            if !self.question.budget.step() { return Known::Unknown }
            arguments.push(value.map_or_else(|| self.values(site), |value| self.evaluate(site, value)));
        }
        Known::Yes(arguments)
    }

    pub fn origin(&self, at: Leaf) -> Known<Option<Leaf>> {
        let mut frame = at.frame;
        while frame != super::question::FrameId::ROOT {
            if !self.question.budget.step() { return Known::Unknown }
            let Some(binding) = self.question.get(frame) else { return Known::Unknown };
            if let Some(origin) = binding.through {
                return match self.semantic.leaf(origin) {
                    Known::Yes(origin) => Known::Yes(Some(origin)),
                    Known::Unknown => Known::Unknown,
                };
            }
            frame = binding.parent;
        }
        Known::Yes(None)
    }

    pub fn declaration_owner(&self, node: NodeRef) -> Known<NodeRef> {
        let Known::Yes(leaf) = self.semantic.leaf(Leaf { target: Target::Node(node), frame: super::question::FrameId::ROOT }) else { return Known::Unknown };
        let Target::Node(node) = leaf.target else { return Known::Unknown };
        if !self.question.budget.step() { return Known::Unknown }
        let Some(names) = self.world.names(node.file) else { return Known::Unknown };
        let classes = names.declared.get(&node.node).map(|&(_, _, classes, _)| classes)
            .or_else(|| names.asked.get(&node.node).map(|lookup| lookup.classes)).unwrap_or(0);
        let name = names.declaration_in(node.node, classes);
        let owner = names.bindings.get(&name).copied()
            .or_else(|| names.declared.get(&name).map(|&(_, owner, ..)| owner)).unwrap_or(name);
        let Some(tree) = self.world.tree(node.file) else { return Known::Unknown };
        if tree.nodes.get(owner as usize).is_none() { return Known::Unknown }
        Known::Yes(NodeRef { file: node.file, node: owner })
    }

    /// The declaration a name reference resolves to, across files (imports,
    /// globals) when nothing in its own file declares it; the first of
    /// merged declarations.
    pub fn resolved_declaration(&self, node: NodeRef) -> Known<NodeRef> {
        let Known::Yes(local) = self.declaration_owner(node) else { return Known::Unknown };
        if local != node { return Known::Yes(local) }
        let Some(names) = self.world.names(node.file) else { return Known::Unknown };
        if !names.asked.contains_key(&node.node) || names.declaration.contains_key(&node.node) { return Known::Yes(local) }
        let leaf = Leaf { target: Target::Node(node), frame: super::question::FrameId::ROOT };
        match self.definitions(leaf, leaf) {
            Known::Yes(definitions) => match definitions.first().map(|definition| definition.target) {
                Some(Target::Node(definition)) => self.declaration_owner(definition),
                _ => Known::Yes(local),
            },
            Known::Unknown => Known::Unknown,
        }
    }

    pub fn bound_argument(&self, formal: Leaf) -> Known<Option<NodeValue>> {
        let Known::Yes(formal) = self.semantic.leaf(formal) else { return Known::Unknown };
        self.memoized(&self.semantic.values.bound_arguments, formal, || self.find_bound_argument(formal))
    }

    /// The file whose globals `at` sees.
    fn importer(&self, at: Leaf) -> Option<u32> {
        Some(match at.target {
            // Trait-only values belong to no file: globals as the linted file sees them.
            Target::Traits(_) => 0,
            Target::Node(node) => node.file,
            Target::Scope(scope) => scope.file,
            Target::Product(_) => {
                let Known::Yes(Some(product)) = self.product_info(at) else { return None };
                match product.source.target {
                    Target::Node(node) => node.file,
                    Target::Scope(scope) => scope.file,
                    _ => return None,
                }
            }
            Target::Modified(_) => return self.importer(self.modified_source(at)?),
        })
    }

    /// The program declares no global `name` of `classes` (no library loaded
    /// it), as opposed to not knowing.
    pub(super) fn global_absent(&self, at: Leaf, name: &str, classes: u64) -> bool {
        self.importer(at).is_some_and(|importer| matches!(self.semantic.modules().global(importer, name, classes), Known::Yes(targets) if targets.is_empty()))
    }

    fn instance(&self, at: Leaf, name: &str, classes: u64, arguments: &[Value]) -> NodeValue {
        let Some(_nested) = self.question.budget.nest() else { return NodeValue::Unknown };
        let Some(importer) = self.importer(at) else { return NodeValue::Unknown };
        let Known::Yes(targets) = self.semantic.modules().global(importer, name, classes) else {
            return NodeValue::Unknown;
        };
        if targets.is_empty() { return NodeValue::Unknown }
        let type_class = self.tables.vocabulary.declaration_class("type");
        if targets.len() > 1 && (type_class == 0 || classes & type_class == 0) { return NodeValue::Unknown }
        let mut args = Vec::new();
        for argument in arguments {
            if !self.question.budget.step() { return NodeValue::Unknown }
            args.push(self.evaluate(at, argument));
        }
        let mut values = Vec::new();
        for target in targets {
            if !self.question.budget.step() { return NodeValue::Unknown }
            let Target::Node(node) = target else { return NodeValue::Unknown };
            let Known::Yes(owner) = self.declaration_owner(node) else { return NodeValue::Unknown };
            let through = match at.target { Target::Node(_) => Some(at), _ => None };
            let frame = self.question.bind_definition(at.frame, owner, through, Some(args.clone()), classes);
            values.push(self.values(Leaf { target: Target::Node(owner), frame }));
        }
        eval::combine(self, values, true)
    }

    fn definitions(&self, leaf: Leaf, through: Leaf) -> Known<Vec<Leaf>> {
        let Target::Node(node) = leaf.target else { return Known::Unknown };
        let Some(_nested) = self.question.budget.nest() else { return Known::Unknown };
        let Some(tree) = self.world.tree(node.file) else { return Known::Unknown };
        let Some(data) = tree.nodes.get(node.node as usize) else { return Known::Unknown };
        if let Some(kind) = self.tables.kinds.get(data.matcher as usize) {
            for (recipe, when) in &kind.defines {
                if !self.question.budget.step() { return Known::Unknown }
                match when.map_or(Known::Yes(true), |when| self.world.condition(leaf, when, self.semantic)) {
                    Known::Yes(false) => continue,
                    Known::Unknown => return Known::Unknown,
                    Known::Yes(true) => {}
                }
                let Known::Yes(sites) = self.sites(leaf, recipe) else { return Known::Unknown };
                let mut definitions = Vec::new();
                for site in sites {
                    if !self.question.budget.step() { return Known::Unknown }
                    let (target, owner) = match site.target {
                        Target::Traits(_) | Target::Product(_) | Target::Modified(_) => return Known::Unknown,
                        Target::Node(node) => {
                            let Known::Yes(owner) = self.declaration_owner(node) else { return Known::Unknown };
                            (Target::Node(owner), owner)
                        },
                        Target::Scope(scope) => {
                            let Some(info) = self.world.names(scope.file).and_then(|names| names.scopes.get(scope.scope as usize)) else {
                                return Known::Unknown;
                            };
                            (site.target, NodeRef { file: scope.file, node: info.node })
                        },
                    };
                    let binding = self.question.get(site.frame).filter(|frame| frame.owner == owner);
                    let frame = if binding.is_some_and(|frame| frame.args.is_some() || frame.through.is_some()) {
                        site.frame
                    } else {
                        let classes = binding.map_or(0, |frame| frame.classes);
                        let through = match through.target { Target::Node(_) => Some(through), _ => None };
                        self.question.bind_definition(site.frame, owner, through, None, classes)
                    };
                    let definition = Leaf { target, frame };
                    if !definitions.contains(&definition) { definitions.push(definition) }
                }
                return Known::Yes(definitions);
            }
        }
        let Some(names) = self.world.names(node.file) else { return Known::Unknown };
        if names.keys.get(&node.node) == Some(&None) { return Known::Unknown }
        let classes = names.asked.get(&node.node).map(|lookup| lookup.classes)
            .or_else(|| names.declared.get(&node.node).map(|&(_, _, classes, _)| classes));
        let declaration = names.declaration_in(node.node, classes.unwrap_or(0));
        let name = (names.declaration.contains_key(&node.node) || names.declared.contains_key(&declaration)).then_some(declaration);
        let modules = self.semantic.modules();
        let route = match classes {
            Some(classes) => modules.definition(node, classes),
            None => Known::Yes(None),
        };
        let targets = match route {
            Known::Unknown => return Known::Unknown,
            Known::Yes(Some(targets)) => targets,
            Known::Yes(None) => match name {
                // Declarations merged into the name that combine (overloads,
                // interface and namespace merging), as the global publication
                // route gives. A plain redeclaration (`var a; var a`) keeps
                // the first one's value.
                Some(name) => {
                    let Some(tree) = self.world.tree(node.file) else { return Known::Unknown };
                    let combines = |owner: u32| tree.nodes.get(owner as usize)
                        .and_then(|data| self.tables.kinds.get(data.matcher as usize))
                        .is_some_and(|kind| !kind.calls.signatures.is_empty() || !kind.calls.sources.is_empty() || kind.member_defaults != 0);
                    // Later declarations by their own owners: their names
                    // resolve to the first.
                    let mut later: Vec<u32> = names.merged.iter().filter(|&(_, &first)| first == name)
                        .map(|(&later, _)| names.declared.get(&later).map_or(later, |&(_, owner, ..)| owner))
                        .filter(|&owner| combines(owner)).collect();
                    later.sort_unstable();
                    let file = node.file;
                    std::iter::once(name).chain(later).map(|declared| Target::Node(NodeRef { file, node: declared })).collect()
                }
                None => {
                    let Some(classes) = classes else { return Known::Yes(Vec::new()) };
                    let Some(tree) = self.world.tree(node.file) else { return Known::Unknown };
                    let Some(text) = self.world.text(node.file) else { return Known::Unknown };
                    let at = tree.node(node.node);
                    let key = match names.keys.get(&node.node) {
                        Some(Some(key)) => std::borrow::Cow::Borrowed(&**key),
                        Some(None) => return Known::Unknown,
                        None => self.world.name_key(node.file, &text[at.start as usize..at.end as usize]),
                    };
                    match modules.global(node.file, &key, classes) {
                        Known::Yes(targets) if !targets.is_empty() => targets,
                        _ => return Known::Unknown,
                    }
                }
            },
        };
        let mut leaves = Vec::new();
        for target in targets {
            if !self.question.budget.step() { return Known::Unknown }
            let (target, owner) = match target {
                Target::Traits(_) | Target::Product(_) | Target::Modified(_) => return Known::Unknown,
                Target::Node(node) => {
                    let Known::Yes(owner) = self.declaration_owner(node) else { return Known::Unknown };
                    (Target::Node(owner), owner)
                }
                Target::Scope(scope) => {
                    let Some(info) = self.world.names(scope.file).and_then(|names| names.scopes.get(scope.scope as usize)) else {
                        return Known::Unknown;
                    };
                    (target, NodeRef { file: scope.file, node: info.node })
                }
            };
            let through = match through.target { Target::Node(_) => Some(through), _ => None };
            let frame = self.question.bind_definition(leaf.frame, owner, through, None, classes.unwrap_or(0));
            leaves.push(Leaf { target, frame });
        }
        Known::Yes(leaves)
    }

    fn find_bound_argument(&self, formal: Leaf) -> Known<Option<NodeValue>> {
        if !self.question.budget.step() { return Known::Unknown }
        let mut frame = formal.frame;
        'frames: while frame != super::question::FrameId::ROOT {
            if !self.question.budget.step() { return Known::Unknown }
            let Some(binding) = self.question.get(frame) else { return Known::Unknown };
            let Some(tree) = self.world.tree(binding.owner.file) else { return Known::Unknown };
            let Some(node) = tree.nodes.get(binding.owner.node as usize) else { return Known::Unknown };
            if let Some(kind) = self.tables.kinds.get(node.matcher as usize) {
                let owner = Leaf { target: Target::Node(binding.owner), frame };
                for (path, when) in &kind.generics {
                    match when.map_or(Known::Yes(true), |condition| self.world.condition(owner, condition, self.semantic)) {
                        Known::Yes(false) => continue,
                        Known::Unknown => return Known::Unknown,
                        Known::Yes(true) => {}
                    }
                    let Known::Yes(parameters) = self.follow(owner, path) else { return Known::Unknown };
                    for (position, parameter) in parameters.iter().enumerate() {
                        if !self.question.budget.step() { return Known::Unknown }
                        if parameter.target != formal.target { continue }
                        if let Some(arguments) = binding.args {
                            match self.question.argument_slot(arguments, position) {
                                Known::Yes(super::question::Argument::Bound(value)) => return Known::Yes(Some(value)),
                                Known::Yes(super::question::Argument::Unbound) => return Known::Yes(None),
                                Known::Yes(super::question::Argument::Inherited) => {
                                    frame = binding.parent;
                                    continue 'frames;
                                }
                                Known::Unknown => return Known::Unknown,
                            }
                        } else {
                            let Some(through) = binding.through else { return Known::Yes(None) };
                            return self.declared_argument(through, position);
                        }
                    }
                }
            }
            frame = binding.parent;
        }
        Known::Yes(None)
    }

    pub fn declared_arguments(&self, at: Leaf) -> Known<Option<Vec<NodeValue>>> {
        let Some(_nested) = self.question.budget.nest() else { return Known::Unknown };
        match self.declared_argument_sites(at) {
            Known::Yes(Some(sites)) => match self.argument_values(sites, None) {
                Known::Yes(arguments) => Known::Yes(Some(arguments)),
                Known::Unknown => Known::Unknown,
            },
            Known::Yes(None) => Known::Yes(None),
            Known::Unknown => Known::Unknown,
        }
    }

    fn declared_argument(&self, at: Leaf, position: usize) -> Known<Option<NodeValue>> {
        let Some(_nested) = self.question.budget.nest() else { return Known::Unknown };
        let Known::Yes(sites) = self.declared_argument_sites(at) else { return Known::Unknown };
        let Some(site) = sites.and_then(|sites| sites.get(position).copied()) else {
            return Known::Yes(None);
        };
        let Some(_value) = self.question.budget.nest() else { return Known::Unknown };
        if !self.question.budget.step() { return Known::Unknown }
        Known::Yes(Some(self.values(site)))
    }

    fn declared_argument_sites(&self, at: Leaf) -> Known<Option<Vec<Leaf>>> {
        let Known::Yes(at) = self.semantic.leaf(at) else { return Known::Unknown };
        let Some(_nested) = self.question.budget.nest() else { return Known::Unknown };
        let Target::Node(node) = at.target else { return Known::Yes(None) };
        let Some(tree) = self.world.tree(node.file) else { return Known::Unknown };
        let Some(node) = tree.nodes.get(node.node as usize) else { return Known::Unknown };
        let Some(kind) = self.tables.kinds.get(node.matcher as usize) else { return Known::Unknown };
        let mut answer = None;
        for (path, when) in &kind.arguments {
            if !self.question.budget.step() { return Known::Unknown }
            match when.map_or(Known::Yes(true), |when| self.world.condition(at, when, self.semantic)) {
                Known::Yes(false) => continue,
                Known::Unknown => return Known::Unknown,
                Known::Yes(true) => {}
            }
            if answer.is_some() { return Known::Unknown }
            let Known::Yes(sites) = self.follow(at, path) else { return Known::Unknown };
            answer = Some(sites);
        }
        Known::Yes(answer)
    }

    pub fn follow(&self, at: Leaf, path: &[u16]) -> Known<Vec<Leaf>> {
        // A modified value's syntax is its source's.
        let at = self.modified_source(at).unwrap_or(at);
        let Known::Yes(at) = self.semantic.leaf(at) else { return Known::Unknown };
        let mut leaves = vec![at];
        for &label in path {
            let mut next = Vec::new();
            for leaf in leaves {
                if !self.question.budget.step() {
                    return Known::Unknown;
                }
                if label == super::IT {
                    next.push(leaf);
                    continue;
                }
                let node = match leaf.target {
                    Target::Traits(_) | Target::Product(_) | Target::Modified(_) => return Known::Unknown,
                    Target::Node(node) => node,
                    Target::Scope(scope) => {
                        let Some(names) = self.world.names(scope.file) else { return Known::Unknown };
                        let Some(scope_info) = names.scopes.get(scope.scope as usize) else { return Known::Unknown };
                        NodeRef { file: scope.file, node: scope_info.node }
                    }
                };
                let Some(tree) = self.world.tree(node.file) else { return Known::Unknown };
                if tree.nodes.get(node.node as usize).is_none() {
                    return Known::Unknown;
                }
                match label {
                    super::THROUGH => {
                        let mut frame = leaf.frame;
                        while let Some(binding) = self.question.get(frame) {
                            if !self.question.budget.step() {
                                return Known::Unknown;
                            }
                            if let Some(through) = binding.through {
                                next.push(through);
                                break;
                            }
                            frame = binding.parent;
                        }
                    }
                    super::DECLARATION => {
                        match self.definitions(leaf, at) {
                            Known::Yes(leaves) => next.extend(leaves),
                            Known::Unknown => return Known::Unknown,
                        }
                    }
                    super::POSITION => return Known::Unknown,
                    label if super::query_step(label) => return Known::Unknown,
                    _ if super::attribute_step(label).is_some() => {
                        use crate::expr::Value as Expression;
                        let Some(attribute_id) = super::attribute_step(label) else { unreachable!() };
                        let Known::Yes(attribute) = self.world.attribute(leaf, attribute_id, self.semantic) else {
                            return Known::Unknown;
                        };
                        let items = match &attribute {
                            Expression::Null => &[][..],
                            Expression::List(items) => items.as_ref(),
                            Expression::Node(..) => std::slice::from_ref(&attribute),
                            _ => return Known::Unknown,
                        };
                        for item in items {
                            if !self.question.budget.step() { return Known::Unknown }
                            let Expression::Node(child, _) = item else { return Known::Unknown };
                            if tree.nodes.get(*child as usize).is_none() { return Known::Unknown }
                            next.push(Leaf { target: Target::Node(NodeRef { file: node.file, node: *child }), frame: leaf.frame });
                        }
                    }
                    _ => {
                        let Known::Yes(children) = self.semantic.follow(leaf, &[label]) else { return Known::Unknown };
                        next.extend(children);
                    }
                }
            }
            leaves = Vec::with_capacity(next.len());
            for leaf in next {
                let Known::Yes(leaf) = self.semantic.leaf(leaf) else { return Known::Unknown };
                leaves.push(leaf);
            }
        }
        Known::Yes(leaves)
    }
}

#[cfg(test)]
mod tests;
