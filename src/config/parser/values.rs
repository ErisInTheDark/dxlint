use super::{Context, Parser};
use crate::config::ast::{Exclusive, Name, NameItem, Unwrap};
use crate::config::error::ConfigError;
use crate::config::source::Span;

impl Parser<'_> {
    pub(super) fn members_item(&mut self) -> Option<NameItem> {
        let classes = self.name_list("a scope class");
        if classes.is_empty() { return None }
        let inherits = if self.word("inherits") { Some(self.unwrap_value()?) } else { None };
        Some(NameItem::Members { classes, inherits })
    }

    pub(super) fn member_item(&mut self) -> Option<NameItem> {
        let classes = self.name_list("a scope class");
        if classes.is_empty() { return None }
        let key = self.unwrap_primary()?;
        if !self.word("unwraps") {
            self.error_here("expected `unwraps <value>` after the member key");
            return None;
        }
        Some(NameItem::Member { classes, key, value: self.unwrap_value()? })
    }

    pub(super) fn ordinal_value(&mut self, text: &str, span: Span) -> Option<Unwrap> {
        // Float rounding or underflow can turn a fractional value into a slot.
        let exact = (|| {
            let (mantissa, exponent) = text.split_once(['e', 'E']).unwrap_or((text, "0"));
            let (integer, fraction) = mantissa.split_once('.').unwrap_or((mantissa, ""));
            let digits = format!("{integer}{fraction}");
            let digits = digits.trim_start_matches('0');
            if digits.is_empty() { return Some(0) }
            let significant = digits.trim_end_matches('0');
            let trailing = digits.len() - significant.len();
            let power = exponent.parse::<i64>().ok()?.checked_sub(fraction.len() as i64)?.checked_add(trailing as i64)?;
            if !(0..=10).contains(&power) || significant.len() + power as usize > 10 { return None }
            let mut value = significant.parse::<u32>().ok()?;
            for _ in 0..power { value = value.checked_mul(10)? }
            Some(value)
        })();
        match exact {
            Some(value) => Some(Unwrap::Ordinal(value)),
            None => {
                self.errors.push(ConfigError::new(span, "a positional key must be a non-negative integer fitting u32"));
                None
            }
        }
    }

    pub(super) fn instance_value(&mut self) -> Option<Unwrap> {
        let (text, span) = self.string("a global declaration name")?;
        let name = Name { text, span };
        let class = if self.word("in") {
            self.ident("a declaration class")?
        } else {
            Name { text: "type".into(), span }
        };
        let mut arguments = Vec::new();
        if self.word("of") {
            arguments.push(self.unwrap_value()?);
            while self.on_line(super::Tk::Comma) {
                self.ts.next_raw();
                arguments.push(self.unwrap_value()?);
            }
        }
        Some(Unwrap::Instance { name, class, arguments })
    }

    pub(super) fn exclusive_item(&mut self, context: Option<&Context>) {
        let keyword = self.bump().expect("peeked");
        let traits = self.name_list("a trait name");
        self.end_of_item();
        if context.is_some() || !self.conditions.is_empty() {
            self.errors.push(ConfigError::new(keyword.span, "`exclusive` belongs at top level"));
            return;
        }
        if traits.iter().map(|name| &name.text).collect::<std::collections::HashSet<_>>().len() < 2 {
            self.errors.push(ConfigError::new(keyword.span, "`exclusive` needs at least two distinct traits"));
            return;
        }
        self.config.exclusive.push(Exclusive { traits, span: keyword.span });
    }
}

#[cfg(test)]
mod tests {
    use super::super::parse_str;
    use crate::config::ast::{NameItem, Unwrap};

    #[test]
    fn scoped_traits_keep_the_view_and_condition_together() {
        let (config, errors, _) = parse_str("in container { fits callable in statics when true }\n");
        assert!(errors.is_empty(), "{errors:?}");
        let NameItem::Fits { traits, views } = &config.names[0].item else { panic!("trait declaration was lost") };
        assert!(!traits.is_empty() && !views.is_empty());
        assert!(config.names[0].when.is_some());
    }

    #[test]
    fn positional_keys_remain_distinct_from_quoted_property_names() {
        let (config, errors, _) = parse_str("in container { unwraps from slots at 0; unwraps from slots at \"0\" }\n");
        assert!(errors.is_empty(), "{errors:?}");
        let keys: Vec<_> = config.names.iter().map(|definition| {
            let NameItem::Unwraps { value: Some(Unwrap::From(_, key, _)), .. } = &definition.item else {
                panic!("member projection was lost");
            };
            &**key
        }).collect();
        assert!(matches!(keys[0], Unwrap::Ordinal(0)));
        assert!(matches!(keys[1], Unwrap::Text(_)));
    }

    #[test]
    fn positional_keys_reject_indices_without_an_exact_slot() {
        for index in ["-1", "1.25", "4294967296"] {
            let (_, errors, _) = parse_str(&format!("in container {{ unwraps from slots at {index} }}\n"));
            assert!(!errors.is_empty(), "invalid positional index was accepted");
        }
    }

    #[test]
    fn positional_keys_do_not_round_fractional_or_tiny_values_to_integer_slots() {
        for index in ["4294967295.0000001", "1e-1000"] {
            let (_, errors, _) = parse_str(&format!("in container {{ unwraps from slots at {index} }}\n"));
            assert!(!errors.is_empty(), "an inexact positional index was rounded into a slot");
        }
        for (index, expected) in [("1.5e1", 15), ("1.000", 1), ("4294967295", u32::MAX)] {
            let (config, errors, _) = parse_str(&format!("in container {{ unwraps from slots at {index} }}\n"));
            assert!(errors.is_empty(), "{errors:?}");
            let NameItem::Unwraps { value: Some(Unwrap::From(_, key, _)), .. } = &config.names[0].item else {
                panic!("positional projection was lost");
            };
            assert!(matches!(&**key, Unwrap::Ordinal(value) if *value == expected));
        }
    }

    #[test]
    fn library_instances_keep_argument_slots_and_declaration_classes() {
        for source in [
            "in container { unwraps instance \"Pair\" of (left | right), tail }\n",
            "in container { unwraps instance \"Box\" in value }\n",
        ] {
            let (config, errors, _) = parse_str(source);
            assert!(errors.is_empty(), "{errors:?}");
            let NameItem::Unwraps { value: Some(Unwrap::Instance { class, arguments, .. }), .. } = &config.names[0].item else {
                panic!("library instance was lost");
            };
            if arguments.is_empty() {
                assert_eq!(class.text, "value");
            } else {
                assert_eq!(class.text, "type");
                assert_eq!(arguments.len(), 2);
                assert!(matches!(&arguments[0], Unwrap::Union(parts) if parts.len() == 2));
                assert!(matches!(&arguments[1], Unwrap::Path(_)));
            }
        }
    }

    #[test]
    fn inherited_member_views_keep_their_base_value() {
        let (config, errors, _) = parse_str("in container { members members inherits scope members from base }\n");
        assert!(errors.is_empty(), "{errors:?}");
        let NameItem::Members { classes, inherits: Some(Unwrap::Scope { classes: view, from }) } = &config.names[0].item else {
            panic!("member inheritance was lost");
        };
        assert_eq!(classes[0].text, view[0].text);
        assert!(matches!(&**from, Unwrap::Path(path) if path.len() == 1 && path[0].text == "base"));
    }

    #[test]
    fn virtual_members_keep_the_key_and_projected_view() {
        let (config, errors, _) = parse_str("in container { member statics \"prototype\" unwraps scope members from it }\n");
        assert!(errors.is_empty(), "{errors:?}");
        let NameItem::Member { classes, key, value } = &config.names[0].item else {
            panic!("virtual member was lost");
        };
        assert!(matches!(key, Unwrap::Text(text) if text == "prototype"));
        let Unwrap::Scope { classes: view, from } = value else { panic!("member view was lost") };
        assert_ne!(classes[0].text, view[0].text);
        assert!(matches!(&**from, Unwrap::Path(path) if path.len() == 1 && path[0].text == "it"));
    }

    #[test]
    fn exclusive_groups_preserve_distinct_domains_for_normalisation() {
        let (config, errors, _) = parse_str("exclusive nullable, string, number\nexclusive object, symbol\n");
        assert!(errors.is_empty(), "{errors:?}");
        assert_eq!(config.exclusive.len(), 2);
        let first: Vec<_> = config.exclusive[0].traits.iter().map(|name| name.text.as_str()).collect();
        assert!(first.contains(&"nullable") && first.contains(&"string") && first.contains(&"number"));
    }

    #[test]
    fn conditional_or_layer_local_domain_axioms_are_rejected() {
        for source in ["in node { exclusive string, number }", "in node { when true { exclusive string, number } }"] {
            let (config, errors, _) = parse_str(source);
            assert!(!errors.is_empty());
            assert!(config.exclusive.is_empty());
        }
    }
}
