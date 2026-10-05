//! Synchronous iteration support of constrained types.

use super::{Checker, Ty};

impl Checker<'_> {
    pub fn iterable_type(&self, ty: &Ty) -> Option<bool> {
        match ty {
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
