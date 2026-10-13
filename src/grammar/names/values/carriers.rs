use super::{Carrier, Known, Leaf, NodeRef, Target, Values};

impl Values<'_> {
    pub fn carrier(&self, at: Leaf) -> Known<Option<Carrier>> {
        let Known::Yes(at) = self.semantic.leaf(at) else { return Known::Unknown };
        if let Some(answer) = self.semantic.values.carriers.borrow().get(&at) {
            return answer.clone();
        }
        self.semantic.values.carriers.borrow_mut().insert(at, Known::Unknown);
        let answer = self.direct_carrier(at);
        self.semantic.values.carriers.borrow_mut().insert(at, answer.clone());
        answer
    }

    fn direct_carrier(&self, at: Leaf) -> Known<Option<Carrier>> {
        let Some(_nested) = self.question.budget.nest() else { return Known::Unknown };
        if matches!(at.target, Target::Product(_)) { return self.product_carrier(at) }
        let node = match at.target {
            Target::Traits(_) | Target::Product(_) => return Known::Yes(None),
            Target::Node(node) => node,
            Target::Scope(scope) => {
                let Some(info) = self.world.names(scope.file).and_then(|names| names.scopes.get(scope.scope as usize)) else {
                    return Known::Unknown;
                };
                NodeRef { file: scope.file, node: info.node }
            }
        };
        let Some(tree) = self.world.tree(node.file) else { return Known::Unknown };
        let Some(data) = tree.nodes.get(node.node as usize) else { return Known::Unknown };
        let Some(kind) = self.tables.kinds.get(data.matcher as usize) else { return Known::Unknown };
        let mut answer = None;
        for recipe in &kind.carriers {
            if !self.question.budget.step() { return Known::Unknown }
            match recipe.when.map_or(Known::Yes(true), |when| self.world.condition(at, when, self.semantic)) {
                Known::Yes(false) => continue,
                Known::Unknown => return Known::Unknown,
                Known::Yes(true) => {}
            }
            let Known::Yes(description) = self.world.operand(at, recipe.description, self.semantic) else {
                return Known::Unknown;
            };
            let arguments = match &recipe.arguments {
                Some(path) => {
                    let Known::Yes(arguments) = self.arguments_using(at, path, recipe.value.as_ref()) else { return Known::Unknown };
                    arguments
                }
                None => Vec::new(),
            };
            let current = Carrier { declaration: node, description, arguments };
            if answer.as_ref().is_some_and(|previous| previous != &current) { return Known::Unknown }
            answer = Some(current);
        }
        Known::Yes(answer)
    }
}
