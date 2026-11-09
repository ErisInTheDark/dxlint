//! Products: the `products` shapes declared in a config. Values built from
//! them live in `dx-semantic`.

use dx_config::ast::{Name, Unwrap};
use dx_config::error::ConfigError;
use dx_ir::expr::AttrNames;
use dx_grammar::Grammar;
use super::compile::{lower_value, Value};
use super::super::{class_bits, lower_path, Vocabulary, PRODUCT_ITEMS};
use super::Key;

#[derive(Clone, Debug)]
pub struct Shape {
    pub traits: u64,
    pub description: dx_ir::value::Value<'static>,
    pub inherits: Option<Value>,
    pub readonly_inherits: Option<Value>,
    pub optional: Option<Value>,
    /// The member a product with fixed positions reads its element count
    /// from: the count literal, a union of them past optional elements.
    pub count: Option<Key>,
}

pub(in crate) fn lower_shape(
    shape: &dx_config::ast::values::ProductShape,
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
    let traits = vocabulary.with_implied(traits);
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
        count: shape.count.as_deref().map(|key| Key::Text(key.into())),
    })
}
