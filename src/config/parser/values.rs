use super::{Context, Parser};
use crate::config::ast::{Exclusive, Name, NameItem, Unwrap};
use crate::config::error::ConfigError;
use crate::config::source::Span;

impl Parser<'_> {
    pub(super) fn product_value(&mut self) -> Option<Unwrap> {
        let shape = self.ident("a product shape")?;
        if !self.word("of") { self.error_here("expected `product <shape> of <sites>`"); return None }
        let sites = self.name_path("product element sites")?;
        let value = if self.word("using") { Some(Box::new(self.unwrap_value()?)) } else { None };
        let optional = if self.word("optional") { Some(self.name_path("an optional element flag")?) } else { None };
        let (mut repeated, mut values) = (None, None);
        while self.word("repeated") {
            if self.word("values") {
                values = Some(self.name_path("a repeated value flag")?);
            } else {
                repeated = Some(self.name_path("a repeated element flag")?);
            }
        }
        let readonly = if self.word("readonly") { Some(self.name_path("a readonly product flag")?) } else { None };
        Some(Unwrap::Product { shape, sites, value, optional, repeated, values, readonly })
    }

    pub(super) fn product_shape_item(&mut self, context: Option<&Context>) {
        let keyword = self.bump().expect("peeked");
        let Some(name) = self.ident("a product shape name") else { return };
        let Some(items) = self.ident("the product items label") else { return };
        if !self.word("fits") { self.error_here("expected `fits <traits>`"); return }
        let traits = self.name_list("a product trait");
        if !self.word("as") { self.error_here("expected `as <constant carrier record>`"); return }
        let Some(open) = self.bump() else { return };
        if open.kind != crate::config::lexer::Tk::LBrace {
            self.errors.push(ConfigError::new(open.span, "product carriers are constant records"));
            return;
        }
        let Some(expression) = self.record(open) else { return };
        let description = match super::schema::literal(&expression) {
            Ok(value @ crate::expr::Value::Record(_)) => value,
            _ => {
                self.errors.push(ConfigError::new(expression.span, "product carriers are constant records"));
                return;
            }
        };
        let inherits = if self.word("inherits") { self.unwrap_value() } else { None };
        let readonly_inherits = if self.word("readonly") {
            if !self.word("inherits") { self.error_here("expected `readonly inherits <value>`"); return }
            self.unwrap_value()
        } else { None };
        let optional = if self.word("optional") { self.unwrap_value() } else { None };
        self.end_of_item();
        if context.is_some() || !self.conditions.is_empty() {
            self.errors.push(ConfigError::new(keyword.span, "product shapes belong at top level"));
            return;
        }
        if let Some(previous) = self.config.products.iter().find(|shape| shape.name.text == name.text) {
            self.errors.push(ConfigError::new(name.span, format!("product shape `{}` is already defined", name.text))
                .related(previous.name.span, "first defined here"));
            return;
        }
        self.config.products.push(crate::config::ast::values::ProductShape {
            name, items, traits, description, inherits, readonly_inherits, optional,
        });
    }

    pub(super) fn apparent_item(&mut self, context: Option<&Context>) {
        let keyword = self.bump().expect("peeked");
        let Some(name) = self.ident("a trait") else { return };
        if !self.word("inherits") { self.error_here("expected `inherits <value>`"); return }
        let Some(inherits) = self.unwrap_value() else { return };
        self.end_of_item();
        if context.is_some() || !self.conditions.is_empty() {
            self.errors.push(ConfigError::new(keyword.span, "apparent members belong at top level"));
            return;
        }
        if let Some(previous) = self.config.apparent.iter().find(|apparent| apparent.name.text == name.text) {
            self.errors.push(ConfigError::new(name.span, format!("apparent members of `{}` are already defined", name.text))
                .related(previous.name.span, "first defined here"));
            return;
        }
        self.config.apparent.push(crate::config::ast::values::Apparent { name, inherits });
    }

    pub(super) fn select_value(&mut self) -> Option<Unwrap> {
        let check = Box::new(self.unwrap_value()?);
        if !self.word("against") { self.error_here("expected `against <value>`"); return None }
        let pattern = Box::new(self.unwrap_value()?);
        if !self.word("then") { self.error_here("expected `then <value>`"); return None }
        let yes = Box::new(self.unwrap_value()?);
        if !self.word("else") { self.error_here("expected `else <value>`"); return None }
        let no = Box::new(self.unwrap_value()?);
        let distribute = if self.word("distribute") {
            Some(self.name_path("a formal parameter site")?)
        } else { None };
        let dynamic_both = self.word("dynamic");
        if dynamic_both && !self.word("both") {
            self.error_here("expected `dynamic both`");
            return None;
        }
        Some(Unwrap::Select { check, pattern, yes, no, distribute, dynamic_both })
    }

    pub(super) fn key_domains_item(&mut self, context: Option<&Context>) {
        let keyword = self.bump().expect("peeked");
        let Some(domain) = self.ident("a key domain") else { return };
        let Some(domain) = crate::config::ast::values::KeyDomain::parse(&domain.text) else {
            self.error_here("expected key domain `text`, `ordinal`, or `symbol`");
            return;
        };
        let traits = self.name_list("a trait name");
        self.end_of_item();
        if context.is_some() || !self.conditions.is_empty() {
            self.errors.push(ConfigError::new(keyword.span, "key domains belong at top level"));
            return;
        }
        self.config.key_domains.push((domain, traits));
    }

    pub(super) fn indexed_item(&mut self) -> Option<NameItem> {
        use crate::config::ast::values::{IndexedMode, KeyDomain};
        let runtime = self.word("runtime");
        let domain = self.peek().and_then(|token| KeyDomain::parse(&token.text));
        let keyed = domain.is_none() && self.word("keyed");
        if domain.is_some() { self.bump(); }
        let keys = domain.is_none() && !keyed && self.word("keys");
        let repeated = domain.is_none() && !keyed && !keys && self.word("repeated");
        let path = self.name_path("an indexed value field")?;
        let mode = if let Some(domain) = domain {
            IndexedMode::Domain(domain)
        } else if keys {
            let bind = if self.word("bind") { Some(self.name_path("a bound formal field")?) } else { None };
            IndexedMode::Keys { bind }
        } else if keyed {
            if !self.word("by") { self.error_here("expected `by <key value>`"); return None; }
            IndexedMode::Keyed(self.unwrap_primary()?)
        } else {
            IndexedMode::Slots { repeated, strings: self.word("strings") }
        };
        let value = if self.word("using") { Some(self.unwrap_value()?) } else { None };
        let views = if self.word("in") { self.name_list("a scope class") } else { Vec::new() };
        Some(NameItem::Indexed { path, mode, value, views, runtime })
    }

    pub(super) fn traits_value(&mut self) -> Option<Unwrap> {
        let traits = self.name_list("a trait name");
        (!traits.is_empty()).then_some(Unwrap::Traits(traits))
    }

    pub(super) fn returned_value(&mut self) -> Option<Unwrap> {
        if !self.word("default") {
            self.error_here("expected `returned default <value>`");
            return None;
        }
        Some(Unwrap::Returned { default: Box::new(self.unwrap_unary()?) })
    }

    pub(super) fn members_item(&mut self) -> Option<NameItem> {
        let classes = self.name_list("a scope class");
        if classes.is_empty() { return None }
        let closed = self.word("closed");
        let inherits = if self.word("inherits") { Some(self.unwrap_value()?) } else { None };
        Some(NameItem::Members { classes, closed, inherits })
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

    pub(super) fn modified_value(&mut self) -> Option<Unwrap> {
        let source = Box::new(self.unwrap_unary()?);
        let mut required = None;
        let mut readonly = None;
        let mut flags = Vec::new();
        loop {
            enum Edit { Optional, Readonly, Flag(Name) }
            let edit = if self.word("optional") { Edit::Optional }
                else if self.word("readonly") { Edit::Readonly }
                else if self.word("flag") { Edit::Flag(self.ident("a declaration flag name")?) }
                else { break };
            let add = if self.word("add") { true } else if self.word("remove") { false } else {
                self.error_here("expected `add` or `remove`");
                return None;
            };
            match edit {
                Edit::Optional => required = Some(!add),
                Edit::Readonly => readonly = Some(add),
                Edit::Flag(name) => flags.push((name, add)),
            }
        }
        Some(Unwrap::Modified { source, required, readonly, flags })
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

    /// `trait a implies b, c`.
    pub(super) fn implication_item(&mut self, context: Option<&Context>) {
        let keyword = self.bump().expect("peeked");
        let Some(name) = self.ident("a trait name") else { return };
        if !self.word("implies") {
            self.error_here("expected `implies <traits>` after the trait name");
            return;
        }
        let implied = self.name_list("a trait name");
        self.end_of_item();
        if context.is_some() || !self.conditions.is_empty() {
            self.errors.push(ConfigError::new(keyword.span, "trait implications belong at top level"));
            return;
        }
        if implied.is_empty() {
            self.errors.push(ConfigError::new(name.span, "`implies` needs at least one trait"));
            return;
        }
        self.config.implications.push((name, implied));
    }

    pub(super) fn algebra_item(&mut self, context: Option<&Context>) {
        let keyword = self.bump().expect("peeked");
        let traits = self.name_list("a trait name");
        self.end_of_item();
        if context.is_some() || !self.conditions.is_empty() {
            self.errors.push(ConfigError::new(keyword.span, "value algebra belongs at top level"));
            return;
        }
        if &*keyword.text == "top" {
            self.config.top.extend(traits);
        } else {
            self.config.dynamic.extend(traits);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::parse_str;
    use crate::config::ast::{NameItem, Unwrap};

    #[test]
    fn nullary_carriers_do_not_need_an_argument_field() {
        let (config, errors, _) = parse_str("in container { carries as { kind: \"enum\" } }\n");
        assert!(errors.is_empty(), "{errors:?}");
        assert!(matches!(&config.names[0].item, NameItem::Carries { .. }));
    }

    #[test]
    fn scoped_traits_keep_the_view_and_condition_together() {
        let (config, errors, _) = parse_str("in container { fits callable in statics when true }\n");
        assert!(errors.is_empty(), "{errors:?}");
        let NameItem::Fits { traits, views, .. } = &config.names[0].item else { panic!("trait declaration was lost") };
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
        let NameItem::Members { classes, inherits: Some(Unwrap::Scope { classes: view, from }), .. } = &config.names[0].item else {
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
