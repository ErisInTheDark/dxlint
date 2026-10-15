//! Parsing call declarations; conditions stay with the shared name item.

use super::Parser;
use crate::config::ast::calls::{ExpectedAt, Item, Path, Variance};

impl Parser<'_> {
    pub(super) fn call_item(&mut self, keyword: &str) -> Option<Item> {
        match keyword {
            "signature" => {
                let parameters = self.name_path("the signature's parameter field")?;
                let receiver = if self.word("receiver") {
                    Some(self.name_path("the signature's receiver field")?)
                } else { None };
                if !self.word("returns") {
                    self.error_here("expected `returns`: `signature parameter returns return_type`");
                    return None;
                }
                let result = self.unwrap_value()?;
                Some(Item::Signature { parameters, receiver, result })
            }
            "signatures" => self.unwrap_value().map(Item::Sources),
            "calls" | "constructs" => {
                let callee = self.unwrap_value()?;
                if !self.word("with") {
                    self.error_here("expected `with`: `calls callee with argument`");
                    return None;
                }
                let arguments = self.name_path("the invocation's argument field")?;
                let receiver = if self.word("this") { Some(self.unwrap_value()?) } else { None };
                Some(Item::Invoke { callee, arguments, construct: keyword == "constructs", receiver })
            }
            "discards" => {
                if !self.word("result") {
                    self.error_here("expected `result` after `discards`");
                    return None;
                }
                Some(Item::DiscardsResult)
            }
            "identity" => self.expr().map(Item::Identity),
            "optional" => Some(Item::Optional),
            "flag" => self.ident("a declaration flag name").map(Item::Flag),
            "overloads" => {
                if self.word("infer") && self.word("last") { return Some(Item::InferLastOverload) }
                if self.word("exact") && self.word("first") { return Some(Item::ExactFirstOverload) }
                self.error_here("expected `infer last` or `exact first` after `overloads`");
                None
            }
            "variance" => {
                if !self.word("parameters") {
                    self.error_here("expected `parameters` after `variance`");
                    return None;
                }
                let variance = if self.word("bivariant") { Variance::Bivariant }
                    else if self.word("contravariant") { Variance::Contravariant }
                    else {
                        self.error_here("expected `bivariant` or `contravariant` parameter variance");
                        return None;
                    };
                Some(Item::ParameterVariance(variance))
            }
            _ => None,
        }
    }

    pub(super) fn call_expected(&mut self, field: Path) -> Option<Item> {
        let at = if self.word("each") { ExpectedAt::Subject } else { ExpectedAt::Owner };
        self.unwrap_value().map(|value| Item::Expected { field, value, at })
    }

    pub(super) fn generics_item(&mut self) -> Option<Path> {
        self.name_path("the generic declaration's ordered formal field")
    }
}
