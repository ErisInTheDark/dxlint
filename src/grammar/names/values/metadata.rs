use super::{Identity, Known, Leaf, NodeRef, Target, Values};

impl Values<'_> {
    pub fn optional(&self, at: Leaf) -> Known<bool> {
        let Known::Yes(at) = self.semantic.leaf(at) else { return Known::Unknown };
        if let Some(&answer) = self.semantic.values.optional.borrow().get(&at) {
            return answer;
        }
        self.semantic.values.optional.borrow_mut().insert(at, Known::Unknown);
        let answer = self.direct_optional(at);
        self.semantic.values.optional.borrow_mut().insert(at, answer);
        answer
    }

    fn direct_optional(&self, at: Leaf) -> Known<bool> {
        let Some(_nested) = self.question.budget.nest() else { return Known::Unknown };
        let Target::Node(node) = at.target else { return Known::Yes(false) };
        let Some(tree) = self.world.tree(node.file) else { return Known::Unknown };
        let Some(node) = tree.nodes.get(node.node as usize) else { return Known::Unknown };
        let Some(kind) = self.tables.kinds.get(node.matcher as usize) else { return Known::Unknown };
        let mut unknown = false;
        for &when in &kind.calls.optional {
            if !self.question.budget.step() { return Known::Unknown }
            match when.map_or(Known::Yes(true), |condition| self.world.condition(at, condition, self.semantic)) {
                Known::Yes(true) => return Known::Yes(true),
                Known::Unknown => unknown = true,
                Known::Yes(false) => {}
            }
        }
        if unknown { Known::Unknown } else { Known::Yes(false) }
    }

    pub fn identity(&self, at: Leaf) -> Known<Option<Identity>> {
        let Known::Yes(at) = self.semantic.leaf(at) else { return Known::Unknown };
        if let Some(answer) = self.semantic.values.identities.borrow().get(&at) {
            return answer.clone();
        }
        self.semantic.values.identities.borrow_mut().insert(at, Known::Unknown);
        let answer = self.direct_identity(at);
        self.semantic.values.identities.borrow_mut().insert(at, answer.clone());
        answer
    }

    fn direct_identity(&self, at: Leaf) -> Known<Option<Identity>> {
        let Some(_nested) = self.question.budget.nest() else { return Known::Unknown };
        let Target::Node(node) = at.target else { return Known::Yes(None) };
        let Some(tree) = self.world.tree(node.file) else { return Known::Unknown };
        let Some(data) = tree.nodes.get(node.node as usize) else { return Known::Unknown };
        let Some(kind) = self.tables.kinds.get(data.matcher as usize) else { return Known::Unknown };
        let mut answer = None;
        let mut unknown = false;
        for identity in &kind.calls.identities {
            if !self.question.budget.step() { return Known::Unknown }
            match identity.when.map_or(Known::Yes(true), |condition| self.world.condition(at, condition, self.semantic)) {
                Known::Yes(false) => continue,
                Known::Unknown => { unknown = true; continue }
                Known::Yes(true) => {}
            }
            let value = match self.world.operand(at, identity.item, self.semantic) {
                Known::Unknown => { unknown = true; continue }
                Known::Yes(value) => value,
            };
            let current = match value {
                crate::expr::Value::Node(target, _) => {
                    let Known::Yes(target) = self.semantic.leaf(Leaf { target: Target::Node(NodeRef { file: node.file, node: target }), frame: at.frame }) else {
                        unknown = true;
                        continue;
                    };
                    let Target::Node(target) = target.target else { unknown = true; continue };
                    let Some(names) = self.world.names(node.file) else { unknown = true; continue };
                    Identity::Declaration(NodeRef {
                        file: node.file,
                        node: names.declaration.get(&target.node).copied().unwrap_or(target.node),
                    })
                }
                value => match self.scalar_identity(value) {
                    Some(value) => Identity::Value(value),
                    None => { unknown = true; continue }
                },
            };
            if answer.as_ref().is_some_and(|previous| previous != &current) {
                let item = self.tables.operands.get(identity.item as usize).and_then(|operand| match operand.of {
                    crate::config::ast::OperandOf::Item(item) => Some(item as u32),
                    _ => None,
                }).unwrap_or(identity.item);
                self.semantic.defect(item, at, "active identity items disagree");
                return Known::Unknown;
            }
            answer = Some(current);
        }
        if unknown { Known::Unknown } else { Known::Yes(answer) }
    }

    fn scalar_identity(&self, value: crate::expr::Value<'static>) -> Option<crate::expr::Value<'static>> {
        use crate::expr::Value as Expression;
        if !self.question.budget.step() { return None }
        match value {
            Expression::Null | Expression::Bool(_) | Expression::Str(_) => Some(value),
            Expression::Num(number) if !number.is_nan() => Some(Expression::Num(if number == 0.0 { 0.0 } else { number })),
            Expression::Record(fields) => {
                let mut canonical = Vec::with_capacity(fields.len());
                for (key, value) in fields.iter() {
                    if matches!(value, Expression::Record(_) | Expression::List(_) | Expression::Node(..)) { return None }
                    canonical.push((key.clone(), self.scalar_identity(value.clone())?));
                }
                canonical.sort_unstable_by(|(left, _), (right, _)| left.as_ref().cmp(right.as_ref()));
                if canonical.windows(2).any(|pair| pair[0].0 == pair[1].0) { return None }
                Some(Expression::Record(canonical.into()))
            }
            _ => None,
        }
    }
}
