//! Lower declarations once; algorithms read only compiled tables.

use super::{Conditional, Expected, Invoke, KindCalls, Path, Signature};
use crate::config::ast::calls::{Item as ParsedItem, Variance};
use crate::config::ast::{Name, Unwrap};
use crate::config::error::ConfigError;
use crate::grammar::names::Value;

#[derive(Clone, Debug)]
pub enum Item {
    Signature(Signature),
    Sources(Value),
    Invoke(Invoke),
    Expected(Expected),
    DiscardsResult,
    Identity(u32),
    Optional,
    ParameterVariance(Variance),
}

pub fn lower(
    item: &ParsedItem,
    path: &mut impl FnMut(&[Name], &mut Vec<ConfigError>) -> Option<Path>,
    value: &mut impl FnMut(&Unwrap, &mut Vec<ConfigError>) -> Option<Value>,
    operand: &mut impl FnMut(u8) -> u32,
    errors: &mut Vec<ConfigError>,
) -> Option<Item> {
    Some(match item {
        ParsedItem::Signature { parameters, result } => Item::Signature(Signature {
            parameters: path(parameters, errors)?,
            result: value(result, errors)?,
        }),
        ParsedItem::Sources(source) => Item::Sources(value(source, errors)?),
        ParsedItem::Invoke { callee, arguments, construct } => Item::Invoke(Invoke {
            callee: value(callee, errors)?,
            arguments: path(arguments, errors)?,
            construct: *construct,
        }),
        ParsedItem::Expected { field, value: expected } => Item::Expected(Expected {
            field: path(field, errors)?,
            value: value(expected, errors)?,
        }),
        ParsedItem::DiscardsResult => Item::DiscardsResult,
        ParsedItem::Identity(_) => Item::Identity(operand(0)),
        ParsedItem::Optional => Item::Optional,
        ParsedItem::ParameterVariance(variance) => Item::ParameterVariance(*variance),
    })
}

impl KindCalls {
    pub fn push(&mut self, item: Item, when: Option<u32>) {
        match item {
            Item::Signature(item) => self.signatures.push(Conditional { item, when }),
            Item::Sources(item) => self.sources.push(Conditional { item, when }),
            Item::Invoke(item) => self.invokes.push(Conditional { item, when }),
            Item::Expected(item) => self.expected.push(Conditional { item, when }),
            Item::DiscardsResult => self.discards.push(when),
            Item::Identity(item) => self.identities.push(Conditional { item, when }),
            Item::Optional => self.optional.push(when),
            Item::ParameterVariance(item) => self.variances.push(Conditional { item, when }),
        }
    }
}
