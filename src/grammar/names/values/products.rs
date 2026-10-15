use crate::config::ast::{Name, Unwrap};
use crate::config::error::ConfigError;
use crate::expr::AttrNames;
use crate::grammar::Grammar;
use super::compile::{lower_value, Value};
use super::super::{class_bits, lower_path, Vocabulary, PRODUCT_ITEMS};
use super::{Carrier, Key, Known, Leaf, Member, MemberOrigin, MemberSource, NodeRef, NodeValue, Target, Values};
use super::compile::ProductRecipe;
pub use super::super::question::{Product, ProductElement, ProductShapeId};

#[derive(Clone, Debug)]
pub struct Shape {
    pub traits: u64,
    pub description: crate::expr::Value<'static>,
    pub inherits: Option<Value>,
    pub readonly_inherits: Option<Value>,
    pub optional: Option<Value>,
}

pub(in crate::grammar::names) fn lower_shape(
    shape: &crate::config::ast::values::ProductShape,
    grammar: &Grammar,
    attrs: &AttrNames,
    record: &dyn Fn(Option<Vec<u16>>) -> Option<Vec<u16>>,
    vocabulary: &mut Vocabulary,
    errors: &mut Vec<ConfigError>,
    requested_classes: &mut Vec<Name>,
) -> Option<Shape> {
    let path = |errors: &mut Vec<ConfigError>, path: &[Name]| {
        if path.first().is_some_and(|name| name.text == shape.items.text) {
            if path.len() == 1 { return Some(vec![PRODUCT_ITEMS]) }
            errors.push(ConfigError::new(path[1].span, "product items hold values, not syntax fields"));
            return None;
        }
        record(lower_path(grammar, attrs, errors, path))
    };
    let traits = class_bits(&mut vocabulary.traits, &shape.traits, errors)?;
    let mut lower = |recipe: &Option<Unwrap>| match recipe {
        Some(recipe) => lower_value(recipe, &path, vocabulary, errors, requested_classes).map(Some),
        None => Some(None),
    };
    Some(Shape {
        traits,
        description: shape.description.clone(),
        inherits: lower(&shape.inherits)?,
        readonly_inherits: lower(&shape.readonly_inherits)?,
        optional: lower(&shape.optional)?,
    })
}

impl Values<'_> {
    pub fn product_info(&self, at: Leaf) -> Known<Option<Product>> {
        let Target::Product(id) = at.target else { return Known::Yes(None) };
        if !self.question.budget.step() { return Known::Unknown }
        match self.question.product(id) {
            Some(product) if self.tables.products.get(product.shape.0 as usize).is_some_and(Option::is_some) => Known::Yes(Some(product)),
            _ => Known::Unknown,
        }
    }

    pub fn product(&self, shape: ProductShapeId, source: Leaf, elements: Vec<ProductElement>, readonly: bool) -> NodeValue {
        self.product_of(shape, source, elements, readonly, false)
    }

    fn product_of(&self, shape: ProductShapeId, source: Leaf, elements: Vec<ProductElement>, readonly: bool, array: bool) -> NodeValue {
        let Known::Yes(source) = self.semantic.leaf(source) else { return NodeValue::Unknown };
        if !matches!(source.target, Target::Node(_) | Target::Scope(_))
            || !self.tables.products.get(shape.0 as usize).is_some_and(Option::is_some) {
            return NodeValue::Unknown;
        }
        let mut ordered = Vec::new();
        for element in elements {
            if !self.question.budget.step() { return NodeValue::Unknown }
            if !matches!(element.site.target, Target::Node(_) | Target::Scope(_)) { return NodeValue::Unknown }
            if element.repeated {
                if let NodeValue::Alternatives(arms) = &element.value {
                    if let [arm] = arms.as_slice() {
                        if let [leaf] = arm.as_slice() {
                            match self.product_info(*leaf) {
                                Known::Yes(Some(nested)) => {
                                    ordered.extend(nested.elements.into_iter().map(|mut inner| {
                                        inner.optional |= element.optional;
                                        inner
                                    }));
                                    continue;
                                }
                                Known::Unknown => return NodeValue::Unknown,
                                Known::Yes(None) => {}
                            }
                        }
                    }
                }
            }
            ordered.push(element);
        }
        let id = self.question.intern_product(Product { shape, source, array, elements: ordered, readonly });
        NodeValue::leaf(Leaf { target: Target::Product(id), frame: source.frame })
    }

    pub fn produce_product(&self, at: Leaf, recipe: &ProductRecipe, mut evaluate: impl FnMut(Leaf) -> NodeValue) -> NodeValue {
        let Known::Yes(sites) = self.follow(at, &recipe.sites) else { return NodeValue::Unknown };
        let Known::Yes(readonly) = self.product_flag(at, recipe.readonly.as_deref()) else { return NodeValue::Unknown };
        let mut elements = Vec::with_capacity(sites.len());
        for site in sites {
            let Known::Yes(optional) = self.product_flag(site, recipe.optional.as_deref()) else { return NodeValue::Unknown };
            let Known::Yes(repeated) = self.product_flag(site, recipe.repeated.as_deref()) else { return NodeValue::Unknown };
            let Known::Yes(values) = self.product_flag(site, recipe.values.as_deref()) else { return NodeValue::Unknown };
            elements.push(ProductElement { site, value: evaluate(site), optional, repeated: repeated && !values, values });
        }
        self.product_of(recipe.shape, at, elements, readonly, recipe.values.is_some())
    }

    pub fn pack_shape(&self, at: Leaf) -> Known<Option<ProductShapeId>> {
        let Known::Yes(at) = self.semantic.leaf(at) else { return Known::Unknown };
        let Target::Node(node) = at.target else { return Known::Yes(None) };
        let Some(tree) = self.world.tree(node.file) else { return Known::Unknown };
        let Some(data) = tree.nodes.get(node.node as usize) else { return Known::Unknown };
        let Some(kind) = self.tables.kinds.get(data.matcher as usize) else { return Known::Unknown };
        let mut answer = None;
        for &(shape, when) in &kind.packs {
            if !self.question.budget.step() { return Known::Unknown }
            match when.map_or(Known::Yes(true), |when| self.world.condition(at, when, self.semantic)) {
                Known::Yes(false) => continue,
                Known::Unknown => return Known::Unknown,
                Known::Yes(true) => {}
            }
            if answer.is_some_and(|previous| previous != shape)
                || !self.tables.products.get(shape.0 as usize).is_some_and(Option::is_some) {
                return Known::Unknown;
            }
            answer = Some(shape);
        }
        Known::Yes(answer)
    }

    fn product_flag(&self, at: Leaf, path: Option<&[u16]>) -> Known<bool> {
        let Some(path) = path else { return Known::Yes(false) };
        let Some((&last, prefix)) = path.split_last() else { return Known::Unknown };
        let Known::Yes(sites) = self.follow(at, prefix) else { return Known::Unknown };
        let mut answer = false;
        for site in sites {
            if !self.question.budget.step() { return Known::Unknown }
            let Some(attribute) = super::super::attribute_step(last) else {
                match self.follow(site, &[last]) {
                    Known::Yes(sites) if sites.is_empty() => continue,
                    _ => return Known::Unknown,
                }
            };
            match self.world.attribute(site, attribute, self.semantic) {
                Known::Yes(crate::expr::Value::Null | crate::expr::Value::Bool(false)) => {}
                Known::Yes(crate::expr::Value::Bool(true)) => answer = true,
                _ => return Known::Unknown,
            }
        }
        Known::Yes(answer)
    }

    fn product_shape(&self, product: &Product) -> Option<&Shape> {
        self.tables.products.get(product.shape.0 as usize)?.as_ref()
    }

    pub(super) fn product_inherited(&self, at: Leaf, product: &Product) -> Known<Option<NodeValue>> {
        let Some(shape) = self.product_shape(product) else { return Known::Unknown };
        let recipe = if product.readonly { shape.readonly_inherits.as_ref().or(shape.inherits.as_ref()) } else { shape.inherits.as_ref() };
        match recipe {
            // Like apparent members: a program without that global (no
            // library) has no inherited layer, not an unknown one.
            Some(super::Value::Instance { name, classes, .. }) if self.global_absent(at, name, *classes) => Known::Yes(None),
            Some(recipe) => {
                let value = self.evaluate(at, recipe);
                if value == NodeValue::Unknown { Known::Unknown } else { Known::Yes(Some(value)) }
            }
            None => Known::Yes(None),
        }
    }

    pub(super) fn product_fits(&self, at: Leaf, trait_id: u16) -> Known<bool> {
        let Known::Yes(Some(product)) = self.product_info(at) else { return Known::Unknown };
        let Some(shape) = self.product_shape(&product) else { return Known::Unknown };
        if shape.traits & (1 << trait_id) != 0 { return Known::Yes(true) }
        match self.product_inherited(at, &product) {
            Known::Yes(Some(value)) => self.always(&value, 1 << trait_id),
            Known::Yes(None) => Known::Yes(false),
            Known::Unknown => Known::Unknown,
        }
    }

    pub(super) fn product_carrier(&self, at: Leaf) -> Known<Option<Carrier>> {
        let Known::Yes(Some(product)) = self.product_info(at) else { return Known::Unknown };
        let Some(shape) = self.product_shape(&product) else { return Known::Unknown };
        let declaration = match product.source.target {
            Target::Node(node) => node,
            Target::Scope(scope) => {
                let Some(info) = self.world.names(scope.file).and_then(|names| names.scopes.get(scope.scope as usize)) else { return Known::Unknown };
                NodeRef { file: scope.file, node: info.node }
            }
            _ => return Known::Unknown,
        };
        // An array's one carrier argument is the union of its items (spreads
        // unpacked, optional items widened); tuples keep one per element.
        let arguments = if product.array {
            let Known::Yes(items) = self.product_items(at) else { return Known::Unknown };
            vec![self.combine(items, false)]
        } else {
            product.elements.into_iter().map(|element| element.value).collect()
        };
        Known::Yes(Some(Carrier { declaration, description: shape.description.clone(), arguments }))
    }

    fn product_element(&self, product: &Product, element: &ProductElement) -> NodeValue {
        if !element.optional { return element.value.clone() }
        let Some(shape) = self.product_shape(product) else { return NodeValue::Unknown };
        match &shape.optional {
            Some(optional) => self.combine([element.value.clone(), self.evaluate(product.source, optional)], false),
            None => element.value.clone(),
        }
    }

    pub(super) fn product_items(&self, at: Leaf) -> Known<Vec<NodeValue>> {
        let Known::Yes(Some(product)) = self.product_info(at) else { return Known::Unknown };
        let mut items = Vec::with_capacity(product.elements.len());
        for element in &product.elements {
            if !self.question.budget.step() { return Known::Unknown }
            let value = self.product_element(&product, element);
            items.push(if element.repeated && !element.values {
                self.member(&value, &Key::Domain(crate::config::ast::values::KeyDomain::Ordinal))
            } else { value });
        }
        Known::Yes(items)
    }

    pub(super) fn product_members(&self, at: Leaf, wanted: Option<&Key>) -> Known<Vec<Member>> {
        let Known::Yes(Some(product)) = self.product_info(at) else { return Known::Unknown };
        let mut members = Vec::new();
        let mut repeated = false;
        let mut selected = Vec::new();
        let mut selected_required = false;
        let mut selected_repeated = false;
        if let Some(index) = wanted.and_then(Key::ordinal) {
            if let Some(start) = product.elements.iter().position(|element| element.repeated || element.values) {
                if index as usize >= start && product.elements[start + 1..].iter().any(|element| !element.repeated && !element.values) {
                    return Known::Unknown;
                }
            }
        }
        for (position, element) in product.elements.iter().enumerate() {
            if !self.question.budget.step() { return Known::Unknown }
            let Ok(position) = u32::try_from(position) else { return Known::Unknown };
            repeated |= element.repeated || element.values;
            let uncertain_position = repeated;
            let key = if uncertain_position {
                Key::Domain(crate::config::ast::values::KeyDomain::Ordinal)
            } else { Key::Ordinal(position) };
            if let Some(wanted) = wanted {
                if let Some(index) = wanted.ordinal() {
                    // Any array item may sit at any index; tuple positions are fixed.
                    if !product.array && ((!uncertain_position && index != position) || (uncertain_position && index < position)) { continue }
                } else if !Self::product_key(&key, wanted) { continue }
            }
            let value = self.product_element(&product, element);
            let value = if element.repeated && !element.values {
                self.member(&value, &Key::Domain(crate::config::ast::values::KeyDomain::Ordinal))
            } else { value };
            if wanted.is_some() || uncertain_position {
                selected.push(value);
                selected_required |= !element.optional && !uncertain_position;
                selected_repeated |= uncertain_position;
                continue;
            }
            let source = MemberSource {
                site: element.site,
                value: Some(Value::Captured(Box::new(value))),
                origin: MemberOrigin::Ordinal { repeated: element.repeated || element.values },
            };
            members.push(Member { key, sources: vec![source], required: Known::Yes(!element.optional && !element.repeated && !element.values) });
        }
        if let Some(wanted) = wanted {
            if !selected.is_empty() || matches!(wanted, Key::Domain(crate::config::ast::values::KeyDomain::Ordinal)) {
                return Known::Yes(vec![Member {
                    key: wanted.clone(),
                    sources: vec![MemberSource {
                        site: product.source,
                        value: Some(Value::Captured(Box::new(self.combine(selected, false)))),
                        origin: MemberOrigin::Ordinal { repeated: selected_repeated },
                    }],
                    required: Known::Yes(selected_required),
                }]);
            }
        } else if !selected.is_empty() {
            members.push(Member {
                key: Key::Domain(crate::config::ast::values::KeyDomain::Ordinal),
                sources: vec![MemberSource {
                    site: product.source,
                    value: Some(Value::Captured(Box::new(self.combine(selected, false)))),
                    origin: MemberOrigin::Ordinal { repeated: true },
                }],
                required: Known::Yes(false),
            });
        }
        let inherited = match self.product_inherited(at, &product) {
            Known::Yes(Some(value)) => value,
            Known::Yes(None) => return Known::Yes(members),
            Known::Unknown => return Known::Unknown,
        };
        if let Some(wanted) = wanted {
            if !members.is_empty() { return Known::Yes(members) }
            let Known::Yes(rows) = self.keyed_members(&inherited, wanted) else { return Known::Unknown };
            for member in rows.into_iter().flatten() { members.push(member) }
        } else {
            let Known::Yes(rows) = self.members(&inherited) else { return Known::Unknown };
            for member in rows.into_iter().flatten() {
                if !members.iter().any(|own| own.key == member.key) { members.push(member) }
            }
        }
        Known::Yes(members)
    }

    fn product_key(key: &Key, wanted: &Key) -> bool {
        key == wanted || match (key, wanted) {
            (Key::Ordinal(index), Key::Text(_)) => wanted.ordinal() == Some(*index),
            (Key::Ordinal(_), Key::Domain(crate::config::ast::values::KeyDomain::Ordinal)) => true,
            (Key::Domain(crate::config::ast::values::KeyDomain::Ordinal), Key::Ordinal(_)) => true,
            (Key::Domain(crate::config::ast::values::KeyDomain::Ordinal), Key::Text(_)) => wanted.ordinal().is_some(),
            _ => false,
        }
    }
}
