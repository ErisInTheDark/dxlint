use super::{Known, Leaf, NodeRef, NodeValue, Target, Values};
use super::compile::Selection;
use super::super::question::FrameId;

impl Values<'_> {
    /// The formal's bound (`constrains`), in its frame.
    pub fn constraint(&self, formal: Leaf) -> Known<Option<NodeValue>> {
        self.formal_value(formal, |kind| &kind.constraints)
    }

    /// The formal's declared default (`defaults`), in its frame.
    pub fn default_argument(&self, formal: Leaf) -> Known<Option<NodeValue>> {
        self.formal_value(formal, |kind| &kind.defaults)
    }

    fn formal_value(&self, formal: Leaf, recipes: impl Fn(&super::super::KindNames) -> &Vec<(super::compile::Value, Option<u32>)>) -> Known<Option<NodeValue>> {
        let Known::Yes(formal) = self.semantic.leaf(formal) else { return Known::Unknown };
        let Target::Node(node) = formal.target else { return Known::Yes(None) };
        let Some(tree) = self.world.tree(node.file) else { return Known::Unknown };
        let Some(data) = tree.nodes.get(node.node as usize) else { return Known::Unknown };
        let Some(kind) = self.tables.kinds.get(data.matcher as usize) else { return Known::Unknown };
        let mut answer = None;
        for (recipe, when) in recipes(kind) {
            if !self.question.budget.step() { return Known::Unknown }
            match when.map_or(Known::Yes(true), |when| self.world.condition(formal, when, self.semantic)) {
                Known::Yes(false) => continue,
                Known::Unknown => return Known::Unknown,
                Known::Yes(true) => {}
            }
            if answer.is_some() { return Known::Unknown }
            let value = self.evaluate(formal, recipe);
            if value == NodeValue::Unknown { return Known::Unknown }
            answer = Some(value);
        }
        Known::Yes(answer)
    }

    pub fn generic_parameters(&self, at: Leaf) -> Known<Vec<Leaf>> {
        let Known::Yes(at) = self.semantic.leaf(at) else { return Known::Unknown };
        let Target::Node(node) = at.target else { return Known::Yes(Vec::new()) };
        let Some(tree) = self.world.tree(node.file) else { return Known::Unknown };
        let Some(data) = tree.nodes.get(node.node as usize) else { return Known::Unknown };
        let Some(kind) = self.tables.kinds.get(data.matcher as usize) else { return Known::Unknown };
        let mut answer = None;
        for (path, when) in &kind.generics {
            if !self.question.budget.step() { return Known::Unknown }
            match when.map_or(Known::Yes(true), |when| self.world.condition(at, when, self.semantic)) {
                Known::Yes(false) => continue,
                Known::Unknown => return Known::Unknown,
                Known::Yes(true) => {}
            }
            if answer.is_some() { return Known::Unknown }
            let Known::Yes(parameters) = self.follow(at, path) else { return Known::Unknown };
            answer = Some(parameters);
        }
        Known::Yes(answer.unwrap_or_default())
    }

    pub(super) fn select(&self, at: Leaf, selection: &Selection) -> NodeValue {
        let Some(_nested) = self.question.budget.nest() else { return NodeValue::Unknown };
        let Some(path) = &selection.distribute else {
            return self.select_arm(at, selection);
        };
        let Known::Yes(sites) = self.follow(at, path) else { return NodeValue::Unknown };
        let [formal] = sites.as_slice() else { return NodeValue::Unknown };
        let Known::Yes((owner, position, count)) = self.parameter_owner(at, *formal) else {
            return NodeValue::Unknown;
        };
        let NodeValue::Alternatives(arms) = self.evaluate(at, &selection.check) else {
            return NodeValue::Unknown;
        };
        let Known::Yes(classes) = self.question.owner_classes(at.frame, owner) else {
            return NodeValue::Unknown;
        };
        let mut branches = Vec::with_capacity(arms.len());
        for arm in arms {
            if !self.question.budget.step() { return NodeValue::Unknown }
            let mut arguments = vec![None; count];
            arguments[position] = Some(NodeValue::rows(vec![arm]));
            let frame = self.question.bind_overlay(at.frame, owner, arguments, classes);
            branches.push(self.select_arm(Leaf { frame, ..at }, selection));
        }
        self.combine(branches, false)
    }

    fn select_arm(&self, at: Leaf, selection: &Selection) -> NodeValue {
        let Target::Node(owner) = at.target else { return NodeValue::Unknown };
        let Known::Yes(slots) = self.generic_parameters(at) else { return NodeValue::Unknown };
        let Known::Yes(classes) = self.question.owner_classes(at.frame, owner) else {
            return NodeValue::Unknown;
        };
        let capture = if slots.is_empty() { at } else {
            Leaf { frame: self.question.bind_slots(at.frame, owner, None, vec![None; slots.len()], classes), ..at }
        };
        let actual = self.evaluate(capture, &selection.check);
        if actual == NodeValue::Unknown { return NodeValue::Unknown }
        let slots: Vec<_> = slots.into_iter().map(|slot| Leaf { frame: capture.frame, ..slot }).collect();
        let Known::Yes(matched) = self.semantic.calls().match_value(capture, &selection.pattern, &actual, &slots) else {
            return NodeValue::Unknown;
        };
        let bound = if slots.is_empty() { capture } else {
            Leaf { frame: self.question.bind_slots(capture.frame, owner, None, matched.arguments, classes), ..at }
        };
        let accepted = matched.accepted;
        if selection.dynamic_both {
            let pattern = self.evaluate(bound, &selection.pattern);
            match self.universal(&pattern) {
                Known::Unknown => return NodeValue::Unknown,
                Known::Yes(true) => return self.evaluate(bound, &selection.yes),
                Known::Yes(false) => return self.combine([
                    self.evaluate(bound, &selection.yes),
                    self.evaluate(capture, &selection.no),
                ], false),
            }
        }
        self.evaluate(if accepted { bound } else { capture }, if accepted { &selection.yes } else { &selection.no })
    }

    fn universal(&self, value: &NodeValue) -> Known<bool> {
        let NodeValue::Alternatives(arms) = value else { return Known::Unknown };
        let mut unknown = false;
        for arm in arms {
            if arm.is_empty() { continue }
            let mut complete = true;
            for &leaf in arm {
                if !self.question.budget.step() { return Known::Unknown }
                match self.top(leaf) {
                    Known::Yes(false) => { complete = false; break }
                    Known::Unknown => { unknown = true; complete = false }
                    Known::Yes(true) => {}
                }
            }
            if complete { return Known::Yes(true) }
        }
        if unknown { Known::Unknown } else { Known::Yes(false) }
    }

    /// The generic owner declaring `formal` (innermost bound frame first),
    /// the formal's position among its parameters and their count.
    pub(super) fn parameter_owner(&self, at: Leaf, formal: Leaf) -> Known<(NodeRef, usize, usize)> {
        let mut frame = at.frame;
        while frame != FrameId::ROOT {
            if !self.question.budget.step() { return Known::Unknown }
            let Some(binding) = self.question.get(frame) else { return Known::Unknown };
            let owner = Leaf { target: Target::Node(binding.owner), frame };
            let Known::Yes(parameters) = self.generic_parameters(owner) else { return Known::Unknown };
            if let Some(position) = parameters.iter().position(|parameter| parameter.target == formal.target) {
                return Known::Yes((binding.owner, position, parameters.len()));
            }
            frame = binding.parent;
        }
        let mut ancestor = formal;
        loop {
            let Known::Yes(parameters) = self.generic_parameters(ancestor) else { return Known::Unknown };
            if let Some(position) = parameters.iter().position(|parameter| parameter.target == formal.target) {
                let Target::Node(owner) = ancestor.target else { return Known::Unknown };
                return Known::Yes((owner, position, parameters.len()));
            }
            ancestor = match self.semantic.parent(ancestor) {
                Known::Yes(Some(parent)) => parent,
                _ => return Known::Unknown,
            };
        }
    }
}
