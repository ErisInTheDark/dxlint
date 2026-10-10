//! Enum member identity and its primitive backing.

use super::{Checker, Env, Sym, Ty};
use crate::native::ts::modules::DeclRef;
use crate::native::ts::summary::{DeclKind, Init};
use std::sync::Arc;

#[derive(Clone, Debug)]
pub struct EnumMember {
    pub symbols: Sym,
    pub key: Box<str>,
    pub base: Ty,
}

impl EnumMember {
    pub fn enum_type(&self) -> Ty {
        Ty::Named(self.symbols.clone(), Vec::new())
    }

    pub fn primitive(&self) -> Option<&'static str> {
        match &self.base {
            Ty::Prim(primitive) | Ty::Lit(_, primitive) => Some(primitive),
            _ => None,
        }
    }
}

impl Checker<'_> {
    pub(super) fn enum_member_type(&self, symbols: &[DeclRef], key: &str) -> Ty {
        self.deeper(|| {
            let symbols: Sym = symbols.iter().copied().filter(|&symbol| self.kind_of(symbol) == Some(DeclKind::Enum)).collect();
            for &symbol in symbols.iter() {
                let Some(summary) = self.summary_of(symbol.file) else { return Ty::Unknown };
                let declaration = &summary.decls[symbol.decl as usize];
                let env = self.decl_env(symbol);
                let mut previous = Some(-1.0);
                let mut earlier = Vec::<(&str, Ty)>::new();
                for member in &declaration.members {
                    let base = match &member.init {
                        Some(Init::Literal(value)) => self.eval(value, &env),
                        None => previous.map_or(Ty::Prim("number"), |number| Ty::Lit(crate::expr::value::js_number_text(number + 1.0).into(), "number")),
                        Some(init) => self.enum_initializer_base(init, &env, &earlier),
                    };
                    previous = match self.literal_value(&base) {
                        Some(super::LiteralValue::Number(number)) => Some(number),
                        _ => None,
                    };
                    if &*member.name == key {
                        return if matches!(base, Ty::Prim("number" | "string") | Ty::Lit(_, "number" | "string")) {
                            Ty::EnumMember(Arc::new(EnumMember { symbols: symbols.clone(), key: member.name.clone(), base }))
                        } else { Ty::Unknown };
                    }
                    earlier.push((&member.name, base));
                }
            }
            Ty::Unknown
        })
    }

    fn enum_initializer_base(&self, init: &Init, env: &Env, earlier: &[(&str, Ty)]) -> Ty {
        let value = if let Init::Path(path) = init {
            if path.len() == 1 {
                earlier.iter().find(|(name, _)| *name == &*path[0]).map(|(_, ty)| ty.clone()).unwrap_or_else(|| self.init_type(init, env))
            } else { self.init_type(init, env) }
        } else { self.init_type(init, env) };
        match value {
            Ty::EnumMember(member) => member.base.clone().literal_base(),
            Ty::Lit(_, primitive) | Ty::Prim(primitive) if matches!(primitive, "number" | "string") => Ty::Prim(primitive),
            _ => Ty::Unknown,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::engine::tests::with_native_file;
    use crate::native::ts::types::LiteralValue;

    #[test]
    fn enum_member_type_and_value_paths_share_literal_identity() {
        let source = "enum E { A = 0, B = 1, C = 0 } type A = /*@*/E.A; const value = /*@*/E.A; type B = /*@*/E.B; type C = /*@*/E.C; type Missing = /*@*/E.Missing; type Whole = /*@*/E;";
        with_native_file(source, |file, nodes| {
            let checker = &file.typing().checker;
            let types: Vec<_> = nodes.iter().map(|&node| file.type_of(node)).collect();
            assert!(!types[0].is_opaque(), "enum member type unresolved: {:?}", types[0]);
            assert!(types[0].same_identity(&types[1]), "type/value identities differ: {:?}", &types[..2]);
            assert_eq!(checker.assignable(&types[0], &types[2]), Some(false));
            assert!(types[0].same_identity(&types[3]), "equal enum values lost identity");
            assert!(types[4].is_opaque(), "missing enum member invented a type");
            assert!(types[0].clone().literal_base().same_identity(&types[5]), "literal base erased enum identity");
        });
    }

    #[test]
    fn enum_member_literals_cover_implicit_values_and_unary_minus() {
        let source = "enum E { Zero, One, Ten = 0xa, Eleven, Negative = -1, Text = 'a' } type A = /*@*/E.Zero; type B = /*@*/E.One; type C = /*@*/E.Ten; type D = /*@*/E.Eleven; type F = /*@*/E.Negative; type G = /*@*/E.Text;";
        with_native_file(source, |file, nodes| {
            let checker = &file.typing().checker;
            for (&node, expected) in nodes[..5].iter().zip([0.0, 1.0, 10.0, 11.0, -1.0]) {
                assert!(matches!(checker.literal_value(&file.type_of(node)), Some(LiteralValue::Number(value)) if value == expected),
                    "enum member value lost: {:?}", file.type_of(node));
            }
            assert!(matches!(checker.literal_value(&file.type_of(nodes[5])), Some(LiteralValue::String(value)) if value == "a"));
        });
    }

    #[test]
    fn namespace_and_alias_paths_keep_enum_member_identity() {
        let source = "namespace N { export enum E { A = 'a', B = 'b' } } import Alias = N.E; type A = /*@*/N.E.A; type B = /*@*/Alias.A; const value = /*@*/Alias.A;";
        with_native_file(source, |file, nodes| {
            let types: Vec<_> = nodes.iter().map(|&node| file.type_of(node)).collect();
            assert!(!types[0].is_opaque(), "namespace enum member unresolved");
            assert!(types[0].same_identity(&types[1]) && types[1].same_identity(&types[2]), "alias changed enum identity: {types:?}");
        });
    }

    #[test]
    fn merged_enum_namespaces_keep_exported_type_paths() {
        let source = "enum E { A } namespace E { export type Text = string; } type Text = /*@*/E.Text; type Member = /*@*/E.A; type Whole = /*@*/E;";
        with_native_file(source, |file, nodes| {
            let checker = &file.typing().checker;
            assert!(matches!(file.type_of(nodes[0]), crate::native::ts::types::Ty::Prim("string")), "enum shortcut hid a namespace type");
            let member = file.type_of(nodes[1]);
            assert!(matches!(checker.literal_value(&member), Some(LiteralValue::Number(value)) if value == 0.0));
            assert!(member.literal_base().same_identity(&file.type_of(nodes[2])));
        });
    }

    #[test]
    fn computed_enum_members_do_not_invent_literal_values() {
        let source = "enum E { Computed = 1 + 2, Following, Known = 3, Alias = Known } type A = /*@*/E.Computed; type B = /*@*/E.Following; type C = /*@*/E.Alias;";
        with_native_file(source, |file, nodes| {
            let checker = &file.typing().checker;
            let types: Vec<_> = nodes.iter().map(|&node| file.type_of(node)).collect();
            for ty in &types {
                assert!(!ty.is_opaque(), "numeric enum member identity lost: {ty:?}");
                assert!(checker.literal_value(ty).is_none(), "computed enum value invented: {ty:?}");
            }
            assert!(!types[0].same_identity(&types[1]) && !types[1].same_identity(&types[2]), "unknown values collapsed member identity");
            assert_ne!(checker.assignable(&crate::native::ts::types::Ty::Lit("2".into(), "number"), &types[0]), Some(true),
                "an unread constant cannot prove literal assignment");
        });
    }

    #[test]
    fn enum_literal_fields_keep_context_and_explicit_return_contracts() {
        let source = "enum E { A = 0, B = 1 } type Whole = /*@*/E; type Member = /*@*/E.A; declare function fixed(): E.A; const fresh = /*@*/{ p: E.A }; const regular = /*@*/{ p: fixed() }; const contextual: { p: E } = /*@*/{ p: E.A };";
        with_native_file(source, |file, nodes| {
            let checker = &file.typing().checker;
            let whole = file.type_of(nodes[0]);
            let member = file.type_of(nodes[1]);
            assert!(checker.member(&file.type_of(nodes[2]), "p").same_identity(&whole), "fresh enum field did not widen");
            for &node in &nodes[3..] {
                let field = checker.member(&file.type_of(node), "p");
                assert!(!field.is_opaque() && field.same_identity(&member), "enum member contract lost: {field:?}");
            }
        });
    }
}
