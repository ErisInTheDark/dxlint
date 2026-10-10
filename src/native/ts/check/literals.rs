//! Literal container domains, including contextual element types.

use super::{sig, NativeFile, F, K, MAX_DEPTH};
use crate::native::ts::types::{PropertyOrigin, StructProperty, Ty};

impl NativeFile<'_> {
    pub(super) fn array_literal(&self, id: u32) -> Ty {
        let s = &self.syntax;
        let elements: Vec<_> = s.all(id, F::Element).collect();
        let spread = elements.iter().any(|&element| s.is(element, K::Spread));
        let items: Vec<_> = elements.into_iter().map(|element| {
            if spread { self.type_of(element).widen() } else { self.mutable_literal_type(element) }
        }).collect();
        fn tuple_context(ty: &Ty) -> bool {
            match ty {
                Ty::Tuple(_) => true,
                Ty::Union(parts) | Ty::Inter(parts) => parts.iter().any(tuple_context),
                _ => false,
            }
        }
        if self.bindings().write_sites.contains_key(&id)
            || self.destructuring_receiver(id).is_some_and(|receiver| matches!(s.kind(receiver), Some(K::ArrayPattern | K::Array)))
            || !spread && (self.const_context(id) || tuple_context(&self.contextual_constraint_of(id))) {
            return Ty::Tuple(items.into_iter().collect());
        }
        if items.is_empty() { return Ty::array(Ty::Never) }
        if items.iter().any(Ty::is_opaque) { return Ty::Unknown }
        let mut unique = Vec::new();
        for ty in items {
            let printed = self.ck().print(&ty);
            if !unique.iter().any(|previous| self.ck().print(previous) == printed) { unique.push(ty) }
        }
        Ty::array(if unique.len() == 1 { unique.pop().expect("one") } else { Ty::Union(unique) })
    }

    /// Object properties compose in source order; uncertain spreads stay unknown.
    pub(super) fn object_literal(&self, id: u32) -> Ty {
        let s = &self.syntax;
        let mut props: Vec<StructProperty> = Vec::new();
        for property in s.all(id, F::Property).collect::<Vec<_>>() {
            if s.is(property, K::Spread) {
                let source = s.get(property, F::Arg).map_or(Ty::Unknown, |arg| self.type_of(arg));
                if matches!(source, Ty::Any) { return Ty::Any }
                let Some(spread) = self.object_spread_properties(&source) else { return Ty::Unknown };
                for property in spread {
                    props.retain(|previous| previous.name != property.name);
                    props.push(property);
                }
                continue;
            }
            let Some(key) = s.get(property, F::Key) else { return Ty::Unknown };
            let Some(name) = self.property_key_name(property) else { return Ty::Unknown };
            let name = name.into_boxed_str();
            let ty = match s.kind(property) {
                Some(K::Prop) => self.mutable_literal_type(s.get(property, F::Value).unwrap_or(key)),
                Some(K::Method) => match s.word(property, F::Kind) {
                    Some("get") => s.get(property, F::ReturnType).map_or_else(|| self.own_return(&sig(s, property), self.ck().file).unwrap_or(Ty::Unknown), |annotation| self.type_of(annotation)),
                    Some("set") => s.all(property, F::Param).next().and_then(|param| s.get(param, F::Name)).map_or(Ty::Unknown, |name| self.type_of(name)),
                    _ => self.type_of(property),
                },
                _ => Ty::Unknown,
            };
            props.retain(|previous| previous.name != name);
            props.push(StructProperty { name, ty, origin: Some(PropertyOrigin { file: self.ck().file, node: property }) });
        }
        Ty::Struct(props.into())
    }

    fn mutable_literal_type(&self, value: u32) -> Ty {
        let ty = self.type_of(value);
        if self.const_context(value) { return ty }
        fn normalise(file: &NativeFile<'_>, value: u32, ty: Ty) -> Ty {
            match ty {
                Ty::Union(parts) => Ty::union(parts.into_iter().map(|part| normalise(file, value, part)).collect()),
                Ty::Lit(..) | Ty::EnumMember(_) => {
                    let context = file.contextual_type_of(value);
                    fn literal_context(context: &Ty, literal: &Ty, parameter: bool) -> bool {
                        match (context, literal) {
                            (Ty::Union(parts) | Ty::Inter(parts), _) => parts.iter().any(|part| literal_context(part, literal, parameter)),
                            (Ty::Named(symbols, _), Ty::EnumMember(member)) => *symbols == member.symbols,
                            (Ty::EnumMember(context), Ty::EnumMember(member)) => context.symbols == member.symbols,
                            (Ty::Lit(_, kind), Ty::Lit(_, primitive)) => kind == primitive,
                            (Ty::Prim("boolean"), Ty::Lit(_, primitive)) => *primitive == "boolean",
                            (Ty::Prim(kind), Ty::Lit(_, primitive)) => parameter && kind == primitive,
                            _ => false,
                        }
                    }
                    if literal_context(&context, &ty, false)
                        || literal_context(&file.contextual_constraint_of(value), &ty, matches!(context, Ty::Param(_)))
                        || !file.fresh_literal_source(value, &ty, MAX_DEPTH) {
                        ty
                    } else { ty.literal_base() }
                }
                other => other,
            }
        }
        normalise(self, value, ty)
    }

    fn fresh_literal_source(&self, value: u32, target: &Ty, remaining: u32) -> bool {
        if remaining == 0 { return false }
        let s = &self.syntax;
        let value = s.unparen(value);
        let fresh = |value| self.fresh_literal_source(value, target, remaining - 1);
        match s.kind(value) {
            Some(K::Number | K::String | K::Bool | K::BigInt) => true,
            Some(K::Unary) if matches!(s.word(value, F::Op), Some("-" | "+")) =>
                s.get(value, F::Arg).is_some_and(fresh),
            Some(K::Ident) => {
                if let Some(declaration) = self.decl_of(value) {
                    if self.binding(declaration).is_some_and(|binding| binding.kind == "const") {
                        if let Some(owner) = s.parent(declaration).filter(|&owner| s.is(owner, K::Declarator)) {
                            return s.get(owner, F::Type).is_none() && s.get(owner, F::Init).is_some_and(fresh);
                        }
                    } else if self.binding(declaration).is_some_and(|binding| matches!(binding.kind, "var" | "let" | "parameter")) {
                        return false;
                    }
                }
                self.ck().fresh_literal_binding(&crate::native::ts::summary::path_of(s, value), &self.env_at(value))
            }
            Some(K::Member | K::Index) => {
                if let Some(resolution) = self.access_resolution(value) {
                    if !resolution.declarations.is_empty() {
                        return resolution.declarations.iter().all(|declaration| declaration.environment.as_ref()
                            .is_some_and(|env| self.ck().fresh_literal_member(&declaration.member, env)));
                    }
                }
                self.ck().fresh_literal_binding(&crate::native::ts::summary::path_of(s, value), &self.env_at(value))
            }
            Some(K::Conditional) => {
                let branches: Vec<_> = [F::Consequent, F::Alternate].into_iter().filter_map(|field| s.get(value, field))
                    .filter(|&branch| self.literal_branch_matches(&self.type_of(branch), target)).collect();
                !branches.is_empty() && branches.into_iter().all(fresh)
            }
            Some(K::Logical) => {
                let (Some(left), Some(right)) = (s.get(value, F::Left), s.get(value, F::Right)) else { return false };
                let left_ty = self.type_of(left);
                let chosen = match s.word(value, F::Op) {
                    Some("||") => self.ck().literal_falsy(&left_ty).map(|falsy| if falsy { right } else { left }),
                    Some("&&") => self.ck().literal_falsy(&left_ty).map(|falsy| if falsy { left } else { right }),
                    Some("??") if matches!(left_ty, Ty::Null | Ty::Undefined | Ty::Void) => Some(right),
                    Some("??") if !left_ty.may_be_nullish() => Some(left),
                    _ => None,
                };
                if let Some(chosen) = chosen { return fresh(chosen) }
                let branches: Vec<_> = [left, right].into_iter()
                    .filter(|&branch| self.literal_branch_matches(&self.type_of(branch), target)).collect();
                !branches.is_empty() && branches.into_iter().all(fresh)
            }
            Some(K::Sequence) => s.all(value, F::Expr).last().is_some_and(fresh),
            Some(K::Satisfies) => s.get(value, F::Expr).is_some_and(fresh),
            _ => false,
        }
    }

    fn literal_branch_matches(&self, ty: &Ty, target: &Ty) -> bool {
        match ty {
            Ty::Union(parts) => parts.iter().any(|part| self.literal_branch_matches(part, target)),
            Ty::Lit(..) => self.ck().equivalent(ty, target) == Some(true),
            Ty::EnumMember(_) => ty.same_identity(target),
            _ => false,
        }
    }

    fn const_context(&self, value: u32) -> bool {
        let s = &self.syntax;
        let mut value = value;
        while let Some(parent) = s.parent(value) {
            match s.kind(parent) {
                Some(K::As | K::TypeAssertion) => return s.get(parent, F::Type).is_some_and(|ty| s.text(ty) == "const"),
                Some(K::Paren | K::Array | K::Object | K::Spread | K::Prop) => value = parent,
                _ => break,
            }
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use crate::engine::tests::with_native_file;
    use crate::native::ts::types::Ty;

    #[test]
    fn tuple_assertions_preserve_contextual_tuple_domains() {
        let source = "type Tuple = [3, 'hi', 'bye']; const asserted = /*@*/[3, 'hi', 'bye'] as Tuple; const angle = <Tuple>/*@*/[3, 'hi', 'bye']; const declared: Tuple = /*@*/[3, 'hi', 'bye']; const constant = /*@*/[3, 'hi', 'bye'] as const;";
        with_native_file(source, |file, nodes| {
            for &node in nodes {
                assert!(matches!(file.type_of(node), Ty::Tuple(_)), "tuple inference lost: {:?}", file.type_of(node));
                assert_eq!(file.typing().checker.equivalent(
                    &file.typing().checker.member(&file.type_of(node), "0"),
                    &Ty::Lit("3".into(), "number"),
                ), Some(true));
            }
        });
    }

    #[test]
    fn contextual_object_domains_survive_generic_inference() {
        let source = "interface C { days: number; ready: boolean } declare function take<T>(value: T): T; take<C>(/*@*/{ days: 0, ready: true });";
        with_native_file(source, |file, nodes| {
            let checker = &file.typing().checker;
            let object = nodes[0];
            let value = file.type_of(object);
            let call = file.syntax.parent(object).unwrap();
            let (params, _, signature) = file.generic_target(call).unwrap();
            let candidate = file.inferred(call, &params, signature.as_deref()).remove(0);
            let mut failures = Vec::new();
            for (label, actual, expected) in [
                ("numeric property", checker.member(&value, "days"), Ty::Prim("number")),
                ("contextual boolean property", checker.member(&value, "ready"), Ty::Lit("true".into(), "boolean")),
                ("inferred boolean property", checker.member(&candidate, "ready"), Ty::Lit("true".into(), "boolean")),
            ] {
                if checker.equivalent(&actual, &expected) != Some(true) { failures.push((label, actual)); }
            }
            assert!(failures.is_empty(), "contextual domains lost: {failures:?}");
        });
    }

    #[test]
    fn contextual_arrays_preserve_tuple_positions() {
        let source = "declare function take<T extends readonly unknown[]>(value: T): T; take<[1, 2]>(/*@*/[1, 2]);";
        with_native_file(source, |file, nodes| {
            let checker = &file.typing().checker;
            let array = nodes[0];
            let value = file.type_of(array);
            let call = file.syntax.parent(array).unwrap();
            let (params, _, signature) = file.generic_target(call).unwrap();
            let candidate = file.inferred(call, &params, signature.as_deref()).remove(0);
            let mut failures = Vec::new();
            for (index, expected) in [("0", "1"), ("1", "2")] {
                for (label, ty) in [("expression", &value), ("inference", &candidate)] {
                    let actual = checker.member(ty, index);
                    if checker.equivalent(&actual, &Ty::Lit(expected.into(), "number")) != Some(true) {
                        failures.push((label, index, actual));
                    }
                }
            }
            assert!(failures.is_empty(), "tuple positions lost: {failures:?}");
        });
    }

    #[test]
    fn common_overload_arguments_do_not_require_common_returns() {
        let source = "interface C { days: number; ready: boolean } declare function take<T>(id: number, value: T): { required: T }; declare function take<T>(id: number, value?: T): { optional?: T }; take<C>(0, /*@*/{ days: 0, ready: true });";
        with_native_file(source, |file, nodes| {
            let checker = &file.typing().checker;
            let value = file.type_of(nodes[0]);
            assert_eq!(checker.equivalent(&checker.member(&value, "ready"), &Ty::Lit("true".into(), "boolean")), Some(true), "the common argument contract must survive differing returns");
            let call = file.syntax.parent(nodes[0]).unwrap();
            let callee = file.syntax.get(call, super::F::Callee).unwrap();
            assert!(checker.signature_for(&file.type_of(callee), &[Ty::Prim("number"), Ty::Unknown], false).is_none(), "a common argument contract must not select a return contract");
        });
        let source = "declare function take(value: { ready: boolean }): { numeric: number }; declare function take(value: { ready: string }): { textual: string }; take(/*@*/{ ready: true });";
        with_native_file(source, |file, nodes| {
            let call = file.syntax.parent(nodes[0]).unwrap();
            let callee = file.syntax.get(call, super::F::Callee).unwrap();
            let checker = &file.typing().checker;
            let callee = file.type_of(callee);
            assert!(checker.argument_context_parameters(&callee, &[Ty::Unknown], Vec::new(), false, false).is_none(), "differing argument contracts remain unresolved");
            assert!(checker.signature_for(&callee, &[Ty::Unknown], false).is_none(), "context recovery must not select a return contract");
        });
    }

    #[test]
    fn mutable_object_fields_widen_fresh_sources_not_literal_contracts() {
        let source = "const fresh = true; const annotated: true = true; declare function returnsTrue(): true; class Holder { readonly copied = fresh; readonly fixed: true = true; } declare const holder: Holder; const value = /*@*/{ fresh, annotated, call: returnsTrue(), asserted: fresh as true, readonlyFresh: holder.copied, readonlyFixed: holder.fixed };";
        with_native_file(source, |file, nodes| {
            let checker = &file.typing().checker;
            let value = file.type_of(nodes[0]);
            let mut failures = Vec::new();
            for (field, expected) in [
                ("fresh", Ty::Prim("boolean")),
                ("readonlyFresh", Ty::Prim("boolean")),
                ("annotated", Ty::Lit("true".into(), "boolean")),
                ("call", Ty::Lit("true".into(), "boolean")),
                ("asserted", Ty::Lit("true".into(), "boolean")),
                ("readonlyFixed", Ty::Lit("true".into(), "boolean")),
            ] {
                let actual = checker.member(&value, field);
                if checker.equivalent(&actual, &expected) != Some(true) { failures.push((field, actual)); }
            }
            assert!(failures.is_empty(), "literal freshness lost: {failures:?}");
        });
    }
}
