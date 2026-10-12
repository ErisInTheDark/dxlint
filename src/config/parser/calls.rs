//! Parsing call declarations; conditions stay with the shared name item.

use super::Parser;
use crate::config::ast::calls::{Item, Path, Variance};

impl Parser<'_> {
    pub(super) fn call_item(&mut self, keyword: &str) -> Option<Item> {
        match keyword {
            "signature" => {
                let parameters = self.name_path("the signature's parameter field")?;
                if !self.word("returns") {
                    self.error_here("expected `returns`: `signature parameter returns return_type`");
                    return None;
                }
                let result = self.unwrap_value()?;
                Some(Item::Signature { parameters, result })
            }
            "calls" | "constructs" => {
                let callee = self.unwrap_value()?;
                if !self.word("with") {
                    self.error_here("expected `with`: `calls callee with argument`");
                    return None;
                }
                let arguments = self.name_path("the invocation's argument field")?;
                Some(Item::Invoke { callee, arguments, construct: keyword == "constructs" })
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
        self.unwrap_value().map(|value| Item::Expected { field, value })
    }
}
