//! Values: `unwraps`, `fits`, `could` / `always`, `through` generics
//! (docs/design/type-layers.md). Owner: Violet.

use std::cell::RefCell;

use crate::fx_hash::FxHashMap;

use super::question::{Known, Leaf, NodeRef, NodeValue, Target};
use super::semantic::Semantic;
use super::Value;

pub mod compile;
mod eval;

pub(super) fn exclusive_groups(
    config: &crate::config::ast::Config,
    vocabulary: &mut super::Vocabulary,
    kinds: &[super::KindNames],
    errors: &mut Vec<crate::config::error::ConfigError>,
) -> Vec<u64> {
    use crate::config::error::ConfigError;
    let fitted = kinds.iter().flat_map(|kind| &kind.fits).fold(0, |all, &(bits, _, _)| all | bits);
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

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Key {
    Text(Box<str>),
    Ordinal(u32),
    Symbol(NodeRef),
}

#[derive(Clone, Debug, PartialEq)]
pub enum Identity {
    Value(crate::expr::Value<'static>),
    Declaration(NodeRef),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Member {
    pub key: Key,
    pub sites: Vec<Leaf>,
}

/// One root question. Nested value, call and narrowing queries use this
/// controller and its context rather than opening another work budget.
#[derive(Default)]
pub(crate) struct State {
    values: RefCell<FxHashMap<Leaf, NodeValue>>,
    traits: RefCell<FxHashMap<(Leaf, u16), Known<bool>>>,
    bound_arguments: RefCell<FxHashMap<Leaf, Known<Option<NodeValue>>>>,
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

    pub fn fits(&self, leaf: Leaf, trait_id: u16) -> Known<bool> {
        if let Some(&answer) = self.semantic.values.traits.borrow().get(&(leaf, trait_id)) {
            return answer;
        }
        if trait_id >= 64 || !self.question.budget.step() {
            return Known::Unknown;
        }
        // A pending question cuts recursion without a separate cycle set.
        self.semantic.values.traits.borrow_mut().insert((leaf, trait_id), Known::Unknown);
        let answer = match leaf.target {
            Target::Scope(scope) => match self.world.names(scope.file)
                .and_then(|names| names.scopes.get(scope.scope as usize)) {
                Some(info) => self.direct_fits(leaf, NodeRef { file: scope.file, node: info.node }, trait_id),
                None => Known::Unknown,
            },
            Target::Node(node) => self.direct_fits(leaf, node, trait_id),
        };
        self.semantic.values.traits.borrow_mut().insert((leaf, trait_id), answer);
        answer
    }

    fn direct_fits(&self, leaf: Leaf, node: NodeRef, trait_id: u16) -> Known<bool> {
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
        if unknown { Known::Unknown } else { Known::Yes(false) }
    }

    pub fn could(&self, value: &NodeValue, traits: u64) -> Known<bool> {
        self.quantify(value, traits, false)
    }

    pub fn always(&self, value: &NodeValue, traits: u64) -> Known<bool> {
        self.quantify(value, traits, true)
    }

    fn normalize(&self, value: NodeValue) -> NodeValue {
        let NodeValue::Alternatives(alternatives) = value else { return NodeValue::Unknown };
        if self.tables.exclusive.is_empty() { return NodeValue::Alternatives(alternatives) }
        let mask = self.tables.exclusive.iter().fold(0, |all, group| all | group);
        let mut possible = Vec::new();
        'alternative: for alternative in alternatives {
            let mut proven = 0;
            for &leaf in &alternative {
                for trait_id in 0..64 {
                    if mask & (1 << trait_id) == 0 { continue }
                    if !self.question.budget.step() { return NodeValue::Unknown }
                    if self.fits(leaf, trait_id) == Known::Yes(true) {
                        proven |= 1 << trait_id;
                        for group in &self.tables.exclusive {
                            if !self.question.budget.step() { return NodeValue::Unknown }
                            if (proven & group).count_ones() > 1 { continue 'alternative }
                        }
                    }
                    if self.question.budget.exhausted() { return NodeValue::Unknown }
                }
            }
            possible.push(alternative);
        }
        NodeValue::Alternatives(possible)
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

    pub fn member(&self, _value: &NodeValue, _key: &Key) -> NodeValue {
        NodeValue::Unknown
    }

    pub fn member_sites(&self, _value: &NodeValue, _key: &Key) -> Known<Vec<Leaf>> {
        Known::Unknown
    }

    pub fn members(&self, _value: &NodeValue) -> Known<Vec<Vec<Member>>> {
        Known::Unknown
    }

    pub fn optional(&self, _at: Leaf) -> Known<bool> {
        Known::Unknown
    }

    pub fn identity(&self, _at: Leaf) -> Known<Option<Identity>> {
        Known::Unknown
    }

    pub fn evaluate(&self, at: Leaf, expression: &Value) -> NodeValue {
        eval::evaluate(self, at, expression)
    }

    pub fn key(&self, _at: Leaf, _expression: &Value) -> Known<Key> {
        Known::Unknown
    }

    pub fn sites(&self, at: Leaf, expression: &Value) -> Known<Vec<Leaf>> {
        eval::sites(self, at, expression)
    }

    pub fn arguments(&self, _at: Leaf, _path: &[u16]) -> Known<Vec<NodeValue>> {
        Known::Unknown
    }

    pub fn bound_argument(&self, formal: Leaf) -> Known<Option<NodeValue>> {
        if let Some(answer) = self.semantic.values.bound_arguments.borrow().get(&formal) {
            return answer.clone();
        }
        self.semantic.values.bound_arguments.borrow_mut().insert(formal, Known::Unknown);
        let answer = self.find_bound_argument(formal);
        self.semantic.values.bound_arguments.borrow_mut().insert(formal, answer.clone());
        answer
    }

    fn find_bound_argument(&self, formal: Leaf) -> Known<Option<NodeValue>> {
        if !self.question.budget.step() { return Known::Unknown }
        let mut frame = formal.frame;
        while frame != super::question::FrameId::ROOT {
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
                        let Some(arguments) = binding.args else { return Known::Yes(None) };
                        return self.question.argument(arguments, position);
                    }
                }
            }
            frame = binding.parent;
        }
        Known::Yes(None)
    }

    pub fn follow(&self, at: Leaf, path: &[u16]) -> Known<Vec<Leaf>> {
        let mut leaves = vec![at];
        for &label in path {
            let mut next = Vec::new();
            for leaf in leaves {
                if !self.question.budget.step() {
                    return Known::Unknown;
                }
                let node = match leaf.target {
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
                    super::IT => next.push(leaf),
                    super::THROUGH => {
                        let mut frame = leaf.frame;
                        while let Some(binding) = self.question.get(frame) {
                            if !self.question.budget.step() {
                                return Known::Unknown;
                            }
                            if let Some(through) = binding.through {
                                next.push(Leaf { target: Target::Node(through), frame: binding.parent });
                                break;
                            }
                            frame = binding.parent;
                        }
                    }
                    super::DECLARATION => {
                        let Some(names) = self.world.names(node.file) else { return Known::Unknown };
                        if let Some(&name) = names.declaration.get(&node.node) {
                            let declaring = names.declared.get(&name).map_or(name, |&(_, item, ..)| item);
                            let owner = NodeRef { file: node.file, node: declaring };
                            let through = match at.target { Target::Node(node) => Some(node), Target::Scope(_) => None };
                            let frame = self.question.bind(leaf.frame, owner, through, None);
                            next.push(Leaf { target: Target::Node(owner), frame });
                        }
                    }
                    super::POSITION => return Known::Unknown,
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
                        for child in super::follow(tree, node.node, &[label]) {
                            if !self.question.budget.step() {
                                return Known::Unknown;
                            }
                            next.push(Leaf { target: Target::Node(NodeRef { file: node.file, node: child }), frame: leaf.frame });
                        }
                    }
                }
            }
            leaves = next;
        }
        Known::Yes(leaves)
    }
}

#[cfg(test)]
mod tests;
