//! Values: `unwraps`, `fits`, `could` / `always`, `through` generics
//! (docs/design/type-layers.md). Owner: Violet.

use super::memo::{Epoch, Memo};
use super::question::{FrameId, Known, Leaf, NodeRef, NodeValue, ScopeRef, Target};
use super::semantic::Semantic;
use super::Value;

pub mod compile;
mod algebra;
mod carriers;
mod conditional;
mod enumerated;
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
    /// Leaves of the linted file read ([`Values::root_reads`]).
    root_reads: std::cell::Cell<u64>,
    /// Reads of a leaf still being evaluated (its seed). An unknown value
    /// computed after such a read may only reflect the unfinished cycle, so
    /// it is not kept.
    cycle_reads: std::cell::Cell<u64>,
    /// Value questions asked ([`Values::value_reads`]): an attribute that
    /// asked none answers alike in every frame.
    value_reads: std::cell::Cell<u64>,
    /// Entries of the `Known` memos being computed ([`Values::memoized`]),
    /// by memo and key hash, innermost last.
    pending: std::cell::RefCell<Vec<(usize, u64)>>,
    /// Pending entries something read while they were still being computed:
    /// the heads of cycles.
    cycle_heads: std::cell::RefCell<crate::fx_hash::FxHashSet<(usize, u64)>>,
    /// Whether a cycle head is being computed again: heads inside it don't
    /// retry too, so the extra work stays one recomputation.
    retrying: std::cell::Cell<bool>,
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
    /// Globals named without arguments whose frames don't depend on where
    /// they are named (`Symbol` in every `Symbol.dispose` key), by importer.
    instances: Memo<(u32, Box<str>, u64), NodeValue>,
}

impl State {
    /// The innermost leaf still being evaluated: where a question cut short
    /// by its budget ran out ([`crate::engine::Profile::record_cut_root`]).
    pub(super) fn innermost(&self) -> Option<Leaf> {
        self.evaluating.borrow().last().map(|&(leaf, _)| leaf)
    }

    pub(super) fn finish(&self, epoch: Epoch, failed: bool, cut: u64) -> u64 {
        use super::memo::Settled;
        // A root cut short leaves the evaluations it was inside open: none
        // is in progress once the root ends.
        self.evaluating.borrow_mut().clear();
        self.pending.borrow_mut().clear();
        self.cycle_heads.borrow_mut().clear();
        self.retrying.set(false);
        self.values.finish(epoch, failed, cut, Settled::settled) + self.widened.finish(epoch, failed, cut, Settled::settled)
            + self.traits.finish(epoch, failed, cut, Settled::settled) + self.optional.finish(epoch, failed, cut, Settled::settled)
            + self.identities.finish(epoch, failed, cut, Settled::settled) + self.members.finish(epoch, failed, cut, Settled::settled)
            + self.member_keys.finish(epoch, failed, cut, Settled::settled) + self.key_paths.finish(epoch, failed, cut, |_| true)
            + self.member_closed.finish(epoch, failed, cut, Settled::settled) + self.bound_arguments.finish(epoch, failed, cut, Settled::settled)
            + self.carriers.finish(epoch, failed, cut, Settled::settled)
            + self.instances.finish(epoch, failed, cut, Settled::settled)
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

/// `ZLINT_LOCAL_VALUES=1`: every world evaluates other files' values itself
/// (A/B ruler).
fn share_values() -> bool {
    static LOCAL: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    !*LOCAL.get_or_init(|| std::env::var_os("ZLINT_LOCAL_VALUES").is_some())
}

impl Values<'_> {
    pub fn values(&self, leaf: Leaf) -> NodeValue {
        self.note_value_read();
        // File 0 is the linted file; others' values are work every file
        // that reads them repeats.
        let _foreign = match (self.world.profile(), leaf.target) {
            (Some(profile), Target::Node(node)) if node.file != 0 => self.world.path(node.file).zip(self.world.path(0))
                .map(|(path, linted)| profile.foreign_scope(path, node.node, linted)),
            _ => None,
        };
        let foreign = matches!(leaf.target, Target::Node(NodeRef { file, .. }) | Target::Scope(ScopeRef { file, .. }) if file != 0);
        if !foreign {
            if matches!(leaf.target, Target::Node(_) | Target::Scope(_)) {
                self.semantic.values.root_reads.set(self.semantic.values.root_reads.get() + 1);
            }
            return eval::values(self, leaf);
        }
        if !share_values() { return eval::values(self, leaf) }
        let Known::Yes(leaf) = self.semantic.leaf(leaf) else { return NodeValue::Unknown };
        if let Some(value) = eval::kept_value(self, leaf) { return value }
        let value = self.world.shared_value(leaf, self.semantic, &mut || eval::values(self, leaf));
        if value != NodeValue::Unknown {
            self.semantic.values.values.insert(self.semantic.epoch, leaf, value.clone());
        }
        value
    }

    /// Steps into the linted file's own nodes so far: a value computed
    /// without any is the same for every linted file.
    pub(crate) fn root_reads(&self) -> u64 {
        self.semantic.values.root_reads.get()
    }

    /// Value questions asked so far (values, recipes, syntax followed
    /// through frames): the answers that may differ by frame.
    pub(crate) fn value_reads(&self) -> u64 {
        self.semantic.values.value_reads.get()
    }

    pub(crate) fn note_value_read(&self) {
        self.semantic.values.value_reads.set(self.semantic.values.value_reads.get() + 1);
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
    /// depend on which question happened to reach the cycle first. The head
    /// of a cycle that comes out unknown is computed once more, now that the
    /// answers inside its cycle are settled: its answer can then be kept
    /// instead of every later ask recomputing the whole cycle.
    pub(super) fn memoized<K, T>(&self, memo: &Memo<K, Known<T>>, key: K, compute: impl Fn() -> Known<T>) -> Known<T>
    where K: Clone + Eq + std::hash::Hash, T: Clone {
        self.memoized_with(memo, key, || Some(()), compute)
    }

    /// [`Self::memoized`], where computing a missing answer first needs
    /// `enter` (a budget step or nesting guard, held while computing): an
    /// answer already kept costs one lookup and no budget.
    pub(super) fn memoized_with<K, T, G>(&self, memo: &Memo<K, Known<T>>, key: K, enter: impl FnOnce() -> Option<G>,
        compute: impl Fn() -> Known<T>) -> Known<T>
    where K: Clone + Eq + std::hash::Hash, T: Clone {
        use std::hash::BuildHasher;
        let state = &self.semantic.values;
        let kept = memo.get(&key);
        if let Some(answer @ Known::Yes(_)) = kept { return answer }
        let tag = (memo as *const Memo<K, Known<T>> as usize, crate::fx_hash::FxBuild::default().hash_one(&key));
        if kept.is_some() {
            if state.pending.borrow().contains(&tag) {
                state.cycle_reads.set(state.cycle_reads.get() + 1);
                state.cycle_heads.borrow_mut().insert(tag);
            }
            return Known::Unknown;
        }
        let Some(_entered) = enter() else { return Known::Unknown };
        let _site = self.world.profile().map(|_| crate::engine::StepSite::enter(std::any::type_name::<(K, T)>()));
        memo.insert(self.semantic.epoch, key.clone(), Known::Unknown);
        state.pending.borrow_mut().push(tag);
        let started = state.cycle_reads.get();
        let mut answer = compute();
        if matches!(answer, Known::Unknown) && state.cycle_heads.borrow_mut().remove(&tag) && !state.retrying.get() {
            state.retrying.set(true);
            answer = compute();
            state.retrying.set(false);
        }
        state.cycle_heads.borrow_mut().remove(&tag);
        state.pending.borrow_mut().pop();
        if matches!(answer, Known::Unknown) && state.cycle_reads.get() > started {
            memo.remove(self.semantic.epoch, &key);
        } else {
            memo.insert(self.semantic.epoch, key, answer.clone());
        }
        answer
    }

    /// Whether `memo`'s answer for `key` is being computed right now (an
    /// ask would be a cycle).
    pub(super) fn computing<K, T>(&self, memo: &Memo<K, Known<T>>, key: &K) -> bool
    where K: std::hash::Hash {
        use std::hash::BuildHasher;
        let tag = (memo as *const Memo<K, Known<T>> as usize, crate::fx_hash::FxBuild::default().hash_one(key));
        self.semantic.values.pending.borrow().contains(&tag)
    }

    /// Whether `leaf` fits `trait_id`, declared or through a trait it fits
    /// that implies it (`trait shaped implies object`).
    pub fn fits(&self, leaf: Leaf, trait_id: u16) -> Known<bool> {
        let direct = self.declared_trait(leaf, trait_id);
        let implying = self.tables.vocabulary.implying.get(usize::from(trait_id)).copied().unwrap_or(0);
        if direct != Known::Yes(false) || implying == 0 { return direct }
        let mut answer = Known::Yes(false);
        for source in 0..64u16 {
            if implying & (1 << source) == 0 { continue }
            match self.declared_trait(leaf, source) {
                Known::Yes(true) => return Known::Yes(true),
                Known::Unknown => answer = Known::Unknown,
                Known::Yes(false) => {}
            }
        }
        answer
    }

    /// [`Self::fits`] by the leaf's own declared traits only: implied traits
    /// match, but never make an intersection impossible (`number & {brand}`
    /// stays a number though shapes imply `object`).
    pub(crate) fn declared_trait(&self, leaf: Leaf, trait_id: u16) -> Known<bool> {
        let Known::Yes(leaf) = self.semantic.leaf(leaf) else { return Known::Unknown };
        if let Some(answer) = self.fixed_trait(leaf, trait_id) { return answer }
        // A scope's fits are what its declaration declares: one answer for
        // every instantiation (frame) of it.
        if matches!(leaf.target, Target::Scope(_)) && leaf.frame != FrameId::ROOT {
            return self.declared_trait(Leaf { frame: FrameId::ROOT, ..leaf }, trait_id);
        }
        self.memoized_with(&self.semantic.values.traits, (leaf, trait_id),
            || (trait_id < 64 && self.question.budget.step()).then_some(()), || self.direct_trait(leaf, trait_id))
    }

    /// A trait answer the leaf's shape alone settles, without the memo: a
    /// trait set's own bits; a node whose kind fits the trait without
    /// condition (as [`Self::direct_fits`] would find); or one whose kind
    /// declares no fit (and no open fit) for it at all. Only conditional fits
    /// are worth a memo entry.
    fn fixed_trait(&self, leaf: Leaf, trait_id: u16) -> Option<Known<bool>> {
        if trait_id >= 64 { return None }
        match leaf.target {
            Target::Traits(bits) => Some(Known::Yes(bits & (1 << trait_id) != 0)),
            Target::Node(node) => self.fixed_fits(node, None, trait_id),
            // A scope's fits are its owner's, filtered by the scope's classes.
            Target::Scope(scope) => {
                let info = self.world.names(scope.file)?.scopes.get(scope.scope as usize)?;
                self.fixed_fits(NodeRef { file: scope.file, node: info.node }, Some(info.classes), trait_id)
            }
            _ => None,
        }
    }

    /// [`Self::direct_fits`] where no condition decides: a fit applying
    /// without one, or no fit (nor open fit) applying at all. View-filtered
    /// fits apply only to a scope whose `classes` meet their views.
    fn fixed_fits(&self, node: NodeRef, classes: Option<u64>, trait_id: u16) -> Option<Known<bool>> {
        let data = self.world.tree(node.file)?.nodes.get(node.node as usize)?;
        let Some(kind) = self.tables.kinds.get(data.matcher as usize) else { return Some(Known::Yes(false)) };
        let applies = |views: u64| views == 0 || classes.is_some_and(|classes| classes & views != 0);
        let mut declared = false;
        for &(bits, views, when) in &kind.fits {
            if bits & (1 << trait_id) == 0 || !applies(views) { continue }
            if when.is_none() { return Some(Known::Yes(true)) }
            declared = true;
        }
        let open = kind.open_fits.iter().any(|&(views, _)| applies(views));
        (!declared && !open).then_some(Known::Yes(false))
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
        self.quantify(value, traits, false, false)
    }

    pub fn always(&self, value: &NodeValue, traits: u64) -> Known<bool> {
        self.quantify(value, traits, true, false)
    }

    /// [`Self::always`] by declared traits only (what a target demands of a
    /// source: a shape implies `object` but doesn't require it).
    pub fn always_declared(&self, value: &NodeValue, traits: u64) -> Known<bool> {
        self.quantify(value, traits, true, true)
    }

    fn quantify(&self, value: &NodeValue, traits: u64, every: bool, declared: bool) -> Known<bool> {
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
                    let fits = if declared { self.declared_trait(leaf, trait_id) } else { self.fits(leaf, trait_id) };
                    match fits {
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
        self.note_value_read();
        eval::evaluate(self, at, expression)
    }

    pub(crate) fn evaluate_optional(&self, at: Leaf, expression: &Value) -> Known<Option<NodeValue>> {
        self.note_value_read();
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
        let kept = arguments.is_empty().then(|| (importer, Box::<str>::from(name), classes));
        if let Some(value) = kept.as_ref().and_then(|key| self.semantic.values.instances.get(key)) { return value }
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
        let mut site_free = true;
        for target in targets {
            if !self.question.budget.step() { return NodeValue::Unknown }
            let Target::Node(node) = target else { return NodeValue::Unknown };
            let Known::Yes(owner) = self.declaration_owner(node) else { return NodeValue::Unknown };
            let shared = if args.is_empty() { self.shared_frame(owner, classes) } else { None };
            let frame = shared.unwrap_or_else(|| {
                // A declaration generic only in its own formals, given every
                // argument, reads nothing from where it was named: every site
                // instantiating `Awaited<Promise<X>>` shares one frame (and
                // its answers).
                if self.semantic.generic_scope(owner) == Some(owner.node) {
                    return self.question.bind_definition(super::question::FrameId::ROOT, owner, None, Some(args.clone()), classes);
                }
                site_free = false;
                let through = match at.target { Target::Node(_) => Some(at), _ => None };
                self.question.bind_definition(at.frame, owner, through, Some(args.clone()), classes)
            });
            values.push(self.values(Leaf { target: Target::Node(owner), frame }));
        }
        let value = eval::combine(self, values, true);
        if let (Some(key), true) = (kept, site_free && value != NodeValue::Unknown) {
            self.semantic.values.instances.insert(self.semantic.epoch, key, value.clone());
        }
        value
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
                        self.definition_frame(site.frame, owner, through, binding.map_or(0, |frame| frame.classes))
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
            let frame = self.definition_frame(leaf.frame, owner, through, classes.unwrap_or(0));
            leaves.push(Leaf { target, frame });
        }
        Known::Yes(leaves)
    }

    /// The frame a reference (`through`, under `parent`) reads `owner` in,
    /// with view `classes`. A declaration with no type parameters around it
    /// instantiates nothing, so every reference shares one frame (only the
    /// view matters): a 2,000-member enum is evaluated once, not per
    /// reference.
    fn definition_frame(&self, parent: super::question::FrameId, owner: NodeRef, through: Leaf, classes: u64) -> super::question::FrameId {
        if let Some(shared) = self.shared_frame(owner, classes) { return shared }
        // A bare reference (`Map`) to a declaration generic only in its own
        // formals binds every formal to its default, whoever refers: all
        // such references share one frame, read through the declaration.
        if self.semantic.generic_scope(owner) == Some(owner.node)
            && matches!(self.declared_argument_sites(through), Known::Yes(Some(ref sites)) if sites.is_empty()) {
            let declaration = Leaf { target: Target::Node(owner), frame: super::question::FrameId::ROOT };
            return self.question.bind_definition(super::question::FrameId::ROOT, owner, Some(declaration), None, classes);
        }
        let through = match through.target { Target::Node(_) => Some(through), _ => None };
        self.question.bind_definition(parent, owner, through, None, classes)
    }

    /// The one frame every reach of `owner` (with view `classes`) shares
    /// when no type parameters are around it: nothing it holds depends on
    /// where it was reached from. `None` for owners that may instantiate.
    pub(super) fn shared_frame(&self, owner: NodeRef, classes: u64) -> Option<super::question::FrameId> {
        (self.world.tree(owner.file).is_some() && self.semantic.generic_scope(owner).is_none())
            .then(|| self.question.bind_definition(super::question::FrameId::ROOT, owner, None, None, classes))
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
                            // The frame bare references share ([`Values::definition_frame`]).
                            if through.target == Target::Node(binding.owner) {
                                return self.default_argument(Leaf { target: parameter.target, frame });
                            }
                            return self.declared_argument(through, position, Leaf { target: parameter.target, frame });
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

    /// The argument the reference `at` gives `formal` (at `position`); a
    /// reference taking type arguments that leaves it out (`Map<K>`) gives
    /// its default.
    fn declared_argument(&self, at: Leaf, position: usize, formal: Leaf) -> Known<Option<NodeValue>> {
        let Some(_nested) = self.question.budget.nest() else { return Known::Unknown };
        let Known::Yes(sites) = self.declared_argument_sites(at) else { return Known::Unknown };
        let Some(sites) = sites else { return Known::Yes(None) };
        let Some(&site) = sites.get(position) else { return self.default_argument(formal) };
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
        // Declarations and references reached are framed by where they are.
        if path.iter().any(|&label| label == super::THROUGH || label == super::DECLARATION) { self.note_value_read() }
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
