//! Value forms and their config lowering.

use crate::config::ast::{Name, Unwrap};
use crate::config::error::ConfigError;

use super::super::{class_bits, intern, PathOf, Vocabulary};

#[derive(Clone, Debug)]
pub enum Value {
    Path(Vec<u16>),
    Text(Box<str>),
    Either(Box<Value>, Box<Value>),
    Union(Vec<Value>),
    Intersection(Vec<Value>),
    Spread { intersection: bool, path: Vec<u16> },
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
        Unwrap::Path(p) => Value::Path(path(errors, p)?),
        Unwrap::Text(text) => Value::Text(text.as_str().into()),
        Unwrap::Either(a, b) => Value::Either(Box::new(lower(a, vocabulary, errors)?), Box::new(lower(b, vocabulary, errors)?)),
        Unwrap::Union(parts) => Value::Union(parts.iter().map(|part| lower(part, vocabulary, errors)).collect::<Option<_>>()?),
        Unwrap::Intersection(parts) => Value::Intersection(parts.iter().map(|part| lower(part, vocabulary, errors)).collect::<Option<_>>()?),
        Unwrap::Spread { intersection, path: p } => Value::Spread { intersection: *intersection, path: path(errors, p)? },
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
    })
}
