//! Value forms and their config lowering.

use crate::config::ast::{Name, Unwrap};
use crate::config::error::ConfigError;

use super::super::{class_bits, intern, PathOf, Vocabulary};
use super::super::question::{NodeValue, ProductShapeId};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProductRecipe {
    pub shape: ProductShapeId,
    pub sites: Vec<u16>,
    pub value: Option<Box<Value>>,
    pub optional: Option<Vec<u16>>,
    pub repeated: Option<Vec<u16>>,
    pub values: Option<Vec<u16>>,
    pub readonly: Option<Vec<u16>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Selection {
    pub check: Value,
    pub pattern: Value,
    pub yes: Value,
    pub no: Value,
    pub distribute: Option<Vec<u16>>,
    pub dynamic_both: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Value {
    Current,
    Unknown,
    Path(Vec<u16>),
    Text(Box<str>),
    Traits(u64),
    Returned { default: Box<Value> },
    Either(Box<Value>, Box<Value>),
    Union(Vec<Value>),
    Intersection(Vec<Value>),
    Spread { intersection: bool, path: Vec<u16>, value: Option<Box<Value>> },
    /// Scope mask zero uses the source kind's member defaults.
    From(Box<Value>, Box<Value>, u64),
    /// Project scopes without expanding the source values.
    Scope { classes: u64, from: Box<Value> },
    Not(Box<Value>),
    Excludes { traits: u64, from: Option<Box<Value>> },
    Includes { value: Box<Value>, from: Option<Box<Value>> },
    Has { key: Box<Value>, from: Option<Box<Value>> },
    Ordinal(u32),
    Instance { name: Box<str>, classes: u64, arguments: Vec<Value> },
    Select(Box<Selection>),
    Product(ProductRecipe),
    Captured(Box<NodeValue>),
}

pub(in crate::grammar::names) fn lower_value(
    value: &Unwrap,
    path: &PathOf<'_>,
    vocabulary: &mut Vocabulary,
    errors: &mut Vec<ConfigError>,
    requested_classes: &mut Vec<Name>,
) -> Option<Value> {
    let mut lower = |value: &Unwrap, vocabulary: &mut Vocabulary, errors: &mut Vec<ConfigError>| {
        lower_value(value, path, vocabulary, errors, requested_classes)
    };
    Some(match value {
        Unwrap::Current => Value::Current,
        Unwrap::Unknown => Value::Unknown,
        Unwrap::Path(p) => Value::Path(path(errors, p)?),
        Unwrap::Text(text) => Value::Text(text.as_str().into()),
        Unwrap::Traits(traits) => {
            let mut bits = 0;
            for name in traits {
                let bit = vocabulary.trait_bit(&name.text);
                if bit == 0 {
                    errors.push(ConfigError::new(name.span, format!("nothing fits trait `{}`", name.text))
                        .suggest(&name.text, vocabulary.traits.iter().map(String::as_str)));
                    return None;
                }
                bits |= bit;
            }
            Value::Traits(bits)
        }
        Unwrap::Returned { default } => Value::Returned { default: Box::new(lower(default, vocabulary, errors)?) },
        Unwrap::Either(a, b) => Value::Either(Box::new(lower(a, vocabulary, errors)?), Box::new(lower(b, vocabulary, errors)?)),
        Unwrap::Union(parts) => Value::Union(parts.iter().map(|part| lower(part, vocabulary, errors)).collect::<Option<_>>()?),
        Unwrap::Intersection(parts) => Value::Intersection(parts.iter().map(|part| lower(part, vocabulary, errors)).collect::<Option<_>>()?),
        Unwrap::Spread { intersection, path: p, value } => Value::Spread {
            intersection: *intersection, path: path(errors, p)?,
            value: match value {
                Some(value) => Some(Box::new(lower(value, vocabulary, errors)?)),
                None => None,
            },
        },
        Unwrap::From(of, key, classes) => {
            let bits = class_bits(&mut vocabulary.scope_classes, classes, errors)?;
            Value::From(Box::new(lower(of, vocabulary, errors)?), Box::new(lower(key, vocabulary, errors)?), bits)
        }
        Unwrap::Scope { classes, from } => {
            let bits = class_bits(&mut vocabulary.scope_classes, classes, errors)?;
            Value::Scope { classes: bits, from: Box::new(lower(from, vocabulary, errors)?) }
        }
        Unwrap::Not(value) => Value::Not(Box::new(lower(value, vocabulary, errors)?)),
        Unwrap::Excludes { traits, from } => {
            let mut bits = 0u64;
            for name in traits {
                let index = intern(&mut vocabulary.traits, &name.text);
                if index >= 64 {
                    errors.push(ConfigError::new(name.span, "at most 64 traits"));
                    return None;
                }
                bits |= 1 << index;
            }
            let from = match from {
                Some(value) => Some(Box::new(lower(value, vocabulary, errors)?)),
                None => None,
            };
            Value::Excludes { traits: bits, from }
        }
        Unwrap::Includes { value: part, from } | Unwrap::Has { key: part, from } => {
            let is_has = matches!(value, Unwrap::Has { .. });
            let part = Box::new(lower(part, vocabulary, errors)?);
            let from = match from {
                Some(value) => Some(Box::new(lower(value, vocabulary, errors)?)),
                None => None,
            };
            if is_has { Value::Has { key: part, from } } else { Value::Includes { value: part, from } }
        }
        Unwrap::Ordinal(index) => Value::Ordinal(*index),
        Unwrap::Instance { name, class, arguments } => {
            let classes = class_bits(&mut vocabulary.declaration_classes, std::slice::from_ref(class), errors)?;
            let arguments = arguments.iter().map(|argument| lower(argument, vocabulary, errors)).collect::<Option<_>>()?;
            requested_classes.push(class.clone());
            Value::Instance { name: name.text.as_str().into(), classes, arguments }
        }
        Unwrap::Select { check, pattern, yes, no, distribute, dynamic_both } => Value::Select(Box::new(Selection {
            check: lower(check, vocabulary, errors)?,
            pattern: lower(pattern, vocabulary, errors)?,
            yes: lower(yes, vocabulary, errors)?,
            no: lower(no, vocabulary, errors)?,
            distribute: match distribute {
                Some(sites) => Some(path(errors, sites)?),
                None => None,
            },
            dynamic_both: *dynamic_both,
        })),
        Unwrap::Product { shape, sites, value, optional, repeated, values, readonly } => {
            let Some(index) = vocabulary.products.iter().position(|name| *name == shape.text) else {
                errors.push(ConfigError::new(shape.span, format!("unknown product shape `{}`", shape.text)));
                return None;
            };
            let mut flag = |flag: &Option<Vec<Name>>| match flag {
                Some(flag) => path(errors, flag).map(Some),
                None => Some(None),
            };
            let optional = flag(optional)?;
            let repeated = flag(repeated)?;
            let values = flag(values)?;
            let readonly = flag(readonly)?;
            Value::Product(ProductRecipe {
                shape: ProductShapeId(index as u32),
                sites: path(errors, sites)?,
                value: match value {
                    Some(value) => Some(Box::new(lower(value, vocabulary, errors)?)),
                    None => None,
                },
                optional, repeated, values, readonly,
            })
        }
    })
}
