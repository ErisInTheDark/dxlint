//! Lower declarations once; algorithms read only compiled tables.

use super::{Conditional, Expected, Invoke, KindCalls, Path, Signature};
use crate::config::ast::calls::{Item as ParsedItem, Variance};
use crate::config::ast::{Name, Unwrap};
use crate::config::error::ConfigError;
use crate::grammar::names::Value;

#[derive(Clone, Debug)]
pub enum Item {
    Signature(Signature),
    Sources(Value, Option<Value>),
    Invoke(Invoke),
    Expected(Expected),
    DiscardsResult,
    Identity(u32),
    Optional,
    Flag(Box<str>),
    ParameterVariance(Variance),
    InferLastOverload,
    ExactFirstOverload,
}

pub fn lower(
    item: &ParsedItem,
    path: &mut impl FnMut(&[Name], &mut Vec<ConfigError>) -> Option<Path>,
    value: &mut impl FnMut(&Unwrap, &mut Vec<ConfigError>) -> Option<Value>,
    operand: &mut impl FnMut(u8) -> u32,
    errors: &mut Vec<ConfigError>,
) -> Option<Item> {
    Some(match item {
        ParsedItem::Signature { parameters, receiver, result } => Item::Signature(Signature {
            parameters: path(parameters, errors)?,
            receiver: match receiver { Some(receiver) => Some(path(receiver, errors)?), None => None },
            result: value(result, errors)?,
        }),
        ParsedItem::Sources(source, result) => Item::Sources(
            value(source, errors)?,
            match result { Some(result) => Some(value(result, errors)?), None => None },
        ),
        ParsedItem::Invoke { callee, arguments, construct, receiver } => Item::Invoke(Invoke {
            callee: value(callee, errors)?,
            arguments: path(arguments, errors)?,
            construct: *construct,
            receiver: match receiver { Some(receiver) => Some(value(receiver, errors)?), None => None },
        }),
        ParsedItem::Expected { field, value: expected, at } => Item::Expected(Expected {
            field: path(field, errors)?,
            value: value(expected, errors)?,
            at: *at,
        }),
        ParsedItem::DiscardsResult => Item::DiscardsResult,
        ParsedItem::Identity(_) => Item::Identity(operand(0)),
        ParsedItem::Optional => Item::Optional,
        ParsedItem::Flag(name) => Item::Flag(name.text.as_str().into()),
        ParsedItem::ParameterVariance(variance) => Item::ParameterVariance(*variance),
        ParsedItem::InferLastOverload => Item::InferLastOverload,
        ParsedItem::ExactFirstOverload => Item::ExactFirstOverload,
    })
}

impl KindCalls {
    pub fn push(&mut self, item: Item, when: Option<u32>) {
        match item {
            Item::Signature(item) => self.signatures.push(Conditional { item, when }),
            Item::Sources(item, result) => {
                self.source_results.resize(self.sources.len(), None);
                self.sources.push(Conditional { item, when });
                self.source_results.push(result);
            }
            Item::Invoke(item) => self.invokes.push(Conditional { item, when }),
            Item::Expected(item) => self.expected.push(Conditional { item, when }),
            Item::DiscardsResult => self.discards.push(when),
            Item::Identity(item) => self.identities.push(Conditional { item, when }),
            Item::Optional => self.optional.push(when),
            Item::Flag(item) => self.flags.push(Conditional { item, when }),
            Item::ParameterVariance(item) => self.variances.push(Conditional { item, when }),
            Item::InferLastOverload => self.infer_last.push(when),
            Item::ExactFirstOverload => self.exact_first.push(when),
        }
    }
}
