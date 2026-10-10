//! Synchronous iteration support of constrained types.

use super::{Checker, Ty};

impl Checker<'_> {
    pub fn iteration_element(&self, ty: &Ty) -> Ty {
        self.iteration_element_at(ty, 0)
    }

    fn iteration_element_at(&self, ty: &Ty, depth: usize) -> Ty {
        if depth >= 32 { return Ty::Unknown }
        if let Some(source) = self.readonly_source(ty) { return self.iteration_element_at(source, depth + 1) }
        match ty {
            Ty::EnumMember(member) => self.iteration_element_at(&member.base, depth + 1),
            Ty::Any => Ty::Any,
            Ty::Array(array) => (*array.element).clone(),
            Ty::Tuple(_) => self.indexed_type(ty, &Ty::Prim("number")),
            Ty::Prim("string") | Ty::Lit(_, "string") => Ty::Prim("string"),
            Ty::Union(parts) => {
                let elements: Vec<_> = parts.iter().map(|part| self.iteration_element_at(part, depth + 1)).collect();
                if elements.iter().any(Ty::is_opaque) { Ty::Unknown } else { Ty::union(elements) }
            }
            Ty::Inter(parts) => {
                let elements: Vec<_> = parts.iter().map(|part| self.iteration_element_at(part, depth + 1)).filter(|ty| !ty.is_opaque()).collect();
                if elements.is_empty() { Ty::Unknown } else { Ty::union(elements) }
            }
            _ => {
                let mut names = Vec::new();
                self.member_names(ty, &mut names);
                let Some(name) = names.iter().find(|name| name.chars().filter(|c| !c.is_whitespace()).eq("[Symbol.iterator]".chars())) else { return Ty::Unknown };
                let iterator = self.member(ty, name);
                let values: Vec<_> = self.callable_returns(&iterator).into_iter().flat_map(|iterator| {
                    self.callable_returns(&self.member(&iterator, "next")).into_iter().flat_map(|result| self.iterator_values(&result, depth + 1))
                }).collect();
                if values.iter().any(Ty::is_opaque) { Ty::Unknown } else { Ty::union(values) }
            }
        }
    }

    fn iterator_values(&self, result: &Ty, depth: usize) -> Vec<Ty> {
        if depth >= 32 { return vec![Ty::Unknown] }
        if let Ty::Union(parts) = result {
            return parts.iter().flat_map(|part| self.iterator_values(part, depth + 1)).collect();
        }
        if matches!(self.literal_value(&self.member(result, "done")), Some(super::LiteralValue::Boolean(true))) {
            Vec::new()
        } else {
            vec![self.member(result, "value")]
        }
    }

    pub fn iterable_type(&self, ty: &Ty) -> Option<bool> {
        match ty {
            Ty::EnumMember(member) => self.iterable_type(&member.base),
            Ty::Any | Ty::Unknown | Ty::Param(_) => None,
            Ty::Array(_) | Ty::Tuple(_) | Ty::Prim("string") | Ty::Lit(_, "string") => Some(true),
            Ty::Union(parts) | Ty::Inter(parts) => {
                let decisive = matches!(ty, Ty::Inter(_));
                let mut unresolved = false;
                for part in parts {
                    match self.iterable_type(part) {
                        Some(result) if result == decisive => return Some(decisive),
                        None => unresolved = true,
                        _ => {}
                    }
                }
                if unresolved { None } else { Some(!decisive) }
            }
            _ => {
                let mut names = Vec::new();
                self.member_names(ty, &mut names);
                Some(names.iter().any(|name| name.chars().filter(|c| !c.is_whitespace()).eq("[Symbol.iterator]".chars())))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::engine::tests::with_native_file;

    #[test]
    fn iteration_elements_follow_the_declared_iterator_protocol() {
        let source = "interface Iterator<T> { next(): { value: T; done: boolean }; } interface Values { [Symbol.iterator](): Iterator<number>; } declare const values: Values; const item = /*@*/values;";
        with_native_file(source, |file, nodes| {
            let checker = &file.typing().checker;
            assert!(matches!(checker.iteration_element(&file.type_of(nodes[0])), super::Ty::Prim("number")));
        });
        let source = "interface Values { [Symbol.iterator](): { next(): { value: number; done: false } | { value: Missing; done: true } }; } declare const values: Values; const item = /*@*/values;";
        with_native_file(source, |file, nodes| {
            let checker = &file.typing().checker;
            assert!(matches!(checker.iteration_element(&file.type_of(nodes[0])), super::Ty::Prim("number")));
        });
        for (ty, expected) in [
            ("readonly number[]", Some("number")),
            ("readonly [number, number?]", None),
            ("number[] | string[]", None),
            ("number[] | Missing", Some("unknown")),
            ("{ [index: number]: number }", Some("unknown")),
            ("{ [Symbol.iterator](): { next(): Missing } }", Some("unknown")),
        ] {
            let source = format!("declare const values: {ty}; const item = /*@*/values;");
            with_native_file(&source, |file, nodes| {
                let checker = &file.typing().checker;
                let element = checker.iteration_element(&file.type_of(nodes[0]));
                match expected {
                    Some("number") => assert!(matches!(element, super::Ty::Prim("number")), "{ty}: {element:?}"),
                    Some("unknown") => assert!(element.is_opaque(), "{ty}: {element:?}"),
                    _ => assert!(matches!(element, super::Ty::Union(_)), "{ty}: {element:?}"),
                }
            });
        }
    }

    #[test]
    fn iterable_types_require_every_union_arm_but_only_one_intersection_arm() {
        for (ty, expected) in [
            ("unknown[]", Some(true)),
            ("readonly [number, string]", Some(true)),
            ("string", Some(true)),
            ("{ [Symbol.iterator]: unknown }", Some(true)),
            ("{ 0: unknown }", Some(false)),
            ("unknown", Some(false)),
            ("any", None),
            ("unknown[] | { 0: unknown }", Some(false)),
            ("unknown[] | { [Symbol.iterator]: unknown }", Some(true)),
            ("unknown[] & object", Some(true)),
            ("{ 0: unknown } & (() => void)", Some(false)),
        ] {
            let source = format!("declare let xs: {ty}; const value = /*@*/xs;");
            with_native_file(&source, |file, nodes| {
                let ty = file.constrained_type_of(nodes[0]);
                assert_eq!(file.typing().checker.iterable_type(&ty), expected, "{source}");
            });
        }
        with_native_file("const value = /*@*/unresolved;", |file, nodes| {
            assert_eq!(file.typing().checker.iterable_type(&file.type_of(nodes[0])), None);
        });
    }
}
