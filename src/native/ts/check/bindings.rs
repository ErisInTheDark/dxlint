//! One pattern projection for expected contracts and declared binding values.

use super::{F, K, NativeFile, Ty};

#[derive(Clone, Copy)]
enum Root<'a> {
    Expected(bool),
    Value(&'a str),
}

impl NativeFile<'_> {
    pub(super) fn pattern_type(&self, node: u32, bound: bool) -> Ty {
        self.pattern_projection(node, Root::Expected(bound))
    }

    pub(super) fn binding_type(&self, declaration: u32, kind: &str) -> Ty {
        self.pattern_projection(declaration, Root::Value(kind))
    }

    fn pattern_projection(&self, node: u32, root: Root<'_>) -> Ty {
        let s = &self.syntax;
        if let Some(annotation) = s.get(node, F::Type) {
            return match root {
                Root::Expected(bound) => self.expected_type(annotation, bound),
                Root::Value(_) => self.annotation(annotation),
            };
        }
        let Some(owner) = s.parent(node) else { return Ty::Unknown };
        match (s.kind(owner), s.field(node)) {
            (Some(K::Declarator | K::Param), Some(F::Name)) => self.pattern_root(owner, node, root),
            (Some(K::AssignPattern), Some(F::Left)) => self.pattern_projection(owner, root),
            (Some(K::PatternProp), Some(F::Value | F::Key)) => {
                if s.field(node) == Some(F::Key) && s.get(owner, F::Value).is_some_and(|value| !s.is(value, K::AssignPattern) || s.get(value, F::Left).is_some()) {
                    return Ty::Unknown;
                }
                let Some(pattern) = s.parent(owner) else { return Ty::Unknown };
                let Some(name) = super::super::statics::property_name(s, owner) else { return Ty::Unknown };
                let value = self.ck().member(&self.pattern_projection(pattern, root), &name);
                let default = if s.is(node, K::AssignPattern) { s.get(node, F::Right) }
                    else if s.field(node) == Some(F::Key) { s.get(owner, F::Value).and_then(|value| s.get(value, F::Right)) }
                    else { None };
                self.pattern_default(value, default, root)
            }
            (Some(K::ArrayPattern), Some(F::Element)) => {
                if s.is(node, K::Rest) { return Ty::Unknown }
                let Some(index) = s.all(owner, F::Element).position(|element| element == node) else { return Ty::Unknown };
                let value = self.contextual_element(&self.pattern_projection(owner, root), index);
                self.pattern_default(value, s.is(node, K::AssignPattern).then(|| s.get(node, F::Right)).flatten(), root)
            }
            _ => Ty::Unknown,
        }
    }

    fn pattern_root(&self, owner: u32, name: u32, root: Root<'_>) -> Ty {
        let s = &self.syntax;
        if let Root::Expected(bound) = root {
            return s.get(owner, F::Type).map_or(Ty::Unknown, |annotation| self.expected_type(annotation, bound));
        }
        if let Some(annotation) = s.get(owner, F::Type) {
            if s.is(owner, K::Declarator) && s.is(name, K::Ident) {
                if let Some(statement) = s.parent(owner) {
                    let declarations = self.summary_decls(s.text(name), statement);
                    if !declarations.is_empty() { return self.ck().value_of(&declarations) }
                }
            }
            let value = self.annotation(annotation);
            return if s.is(owner, K::Param) && s.get(owner, F::Optional).is_some() { value.or_undefined() } else { value };
        }
        if s.is(owner, K::Param) { return self.contextual_param(owner) }
        match s.get(owner, F::Init) {
            // Empty arrays evolve with later writes.
            Some(init) if s.is(init, K::Array) && s.all(init, F::Element).next().is_none() => Ty::Unknown,
            Some(init) => {
                let value = self.type_of(init);
                if matches!(root, Root::Value("const")) { value } else { value.widen() }
            }
            None => Ty::Unknown,
        }
    }

    fn pattern_default(&self, value: Ty, default: Option<u32>, root: Root<'_>) -> Ty {
        let Some(default) = default.filter(|_| matches!(root, Root::Value(_))) else { return value };
        fn replace_undefined(value: Ty, fallback: &Ty) -> Ty {
            match value {
                Ty::Undefined => fallback.clone(),
                Ty::Union(parts) => Ty::union(parts.into_iter().map(|part| replace_undefined(part, fallback)).collect()),
                value => value,
            }
        }
        if value.is_opaque() { return value }
        let fallback = self.type_of(default);
        let fallback = if matches!(root, Root::Value("const")) { fallback } else { fallback.widen() };
        replace_undefined(value, &fallback)
    }
}
