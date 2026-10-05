//! Indexed access: explicit properties first, then applicable index signatures.

use super::{Checker, Env, Ty, MAX_DEPTH};
use super::super::summary::{Member, MemberKind};
use super::members::{MemberDeclaration, Resolution};

impl Checker<'_> {
    pub fn index_resolution(&self, object: &Ty, index: &Ty) -> Resolution {
        self.index_query::<true>(object, index)
    }

    pub fn index_signature_keys(&self, object: &Ty) -> Vec<Ty> {
        self.index_signature::<true, true>(object, &Ty::Any, object).map_or_else(Vec::new, |result| result.signature_keys)
    }

    pub fn indexed_type(&self, object: &Ty, index: &Ty) -> Ty {
        self.index_query::<false>(object, index).ty
    }

    fn index_query<const METADATA: bool>(&self, object: &Ty, index: &Ty) -> Resolution {
        self.resolution_depth(|| {
            if let Ty::Union(keys) = index {
                return Resolution::merge(keys.iter().map(|key| self.index_query::<METADATA>(object, key)).collect(), Ty::union);
            }
            if let Ty::Union(parts) = object {
                let found = parts.iter().filter(|part| !matches!(part, Ty::Null | Ty::Undefined)).map(|part| self.index_query::<METADATA>(part, index)).collect();
                return Resolution::merge(found, union);
            }
            if let Some(name) = literal_key(index) {
                let found = self.member_query::<METADATA>(object, &name);
                if found.declared {
                    return found;
                }
            }
            self.index_signature::<METADATA, false>(object, index, object).unwrap_or_else(|| Resolution::typed(Ty::Unknown))
        })
    }

    fn index_signature<const METADATA: bool, const KEYS: bool>(&self, object: &Ty, index: &Ty, receiver: &Ty) -> Option<Resolution> {
        let depth = self.depth.get();
        if depth >= MAX_DEPTH {
            return Some(Resolution::typed(Ty::Unknown));
        }
        self.depth.set(depth + 1);
        let found = self.index_signature_inner::<METADATA, KEYS>(object, index, receiver);
        self.depth.set(depth);
        found
    }

    fn index_signature_inner<const METADATA: bool, const KEYS: bool>(&self, object: &Ty, index: &Ty, receiver: &Ty) -> Option<Resolution> {
        if let Some(source) = self.readonly_source(object) {
            return self.index_signature::<METADATA, KEYS>(source, index, source);
        }
        if let Some((key, value)) = self.record_parts(object) {
            if KEYS && super::utilities::required_keys(key).is_some_and(|keys| !keys.is_empty()) { return None }
            return (KEYS || self.index_key_priority(index, key) > 0).then(|| Resolution::signature::<METADATA>(value.clone(), key.clone()));
        }
        if self.property_filter(object).is_some() {
            return None;
        }
        match object {
            Ty::Any => Some(Resolution::typed(Ty::Any)),
            Ty::Unknown | Ty::IntrinsicUnknown | Ty::Param(_) => Some(Resolution::typed(Ty::Unknown)),
            Ty::Array(element) => (KEYS || self.index_key_priority(index, &Ty::Prim("number")) > 0).then(|| Resolution::signature::<METADATA>((*element.element).clone(), Ty::Prim("number"))),
            Ty::Tuple(tuple) if KEYS || self.index_key_priority(index, &Ty::Prim("number")) > 0 => {
                if let Some(at) = numeric_index(index) {
                    return Some(Resolution::signature::<METADATA>(tuple.at(at), Ty::Prim("number")));
                }
                if literal_key(index).is_some() {
                    return Some(Resolution::signature::<METADATA>(Ty::Unknown, Ty::Prim("number")));
                }
                let mut elements = Vec::new();
                for (at, element) in tuple.elements.iter().enumerate() {
                    elements.push(if element.rest {
                        self.indexed_type(&element.ty, &Ty::Prim("number"))
                    } else {
                        tuple.at(at)
                    });
                }
                Some(Resolution::signature::<METADATA>(Ty::union(elements), Ty::Prim("number")))
            }
            Ty::Obj(members, env) => {
                let env = Env { this: Some(Box::new(receiver.clone())), ..(**env).clone() };
                self.index_from::<METADATA, KEYS>(members.iter(), index, &env, None)
            }
            Ty::Named(symbols, args) => {
                let mut inherited = Vec::new();
                let mut own = Vec::new();
                for &decl in symbols.iter() {
                    let summary = self.summary_of(decl.file)?;
                    let declaration = &summary.decls[decl.decl as usize];
                    let mut env = self.decl_env(decl);
                    env.tparams = self.bind_tparams(&declaration.type_params, args.clone(), &env);
                    env.this = Some(Box::new(receiver.clone()));
                    if let Some(found) = self.index_from::<METADATA, KEYS>(declaration.members.iter().filter(|member| !member.is_static), index, &env, Some(decl)) {
                        own.push(found);
                    }
                    for base in self.bases(declaration) {
                        inherited.push(self.eval(base, &env));
                    }
                }
                if !KEYS && !own.is_empty() {
                    return Some(Resolution::merge(own, intersection));
                }
                own.extend(inherited.iter().filter_map(|base| self.index_signature::<METADATA, KEYS>(base, index, receiver)));
                (!own.is_empty()).then(|| Resolution::merge(own, intersection))
            }
            Ty::Union(parts) => {
                let found: Option<Vec<_>> = parts.iter().filter(|part| !matches!(part, Ty::Null | Ty::Undefined)).map(|part| self.index_signature::<METADATA, KEYS>(part, index, part)).collect();
                found.filter(|parts| !parts.is_empty()).map(|parts| {
                    let common = if KEYS {
                        Some(parts.iter().flat_map(|part| part.signature_keys.iter()).filter(|key| {
                            parts.iter().all(|part| part.signature_keys.iter().any(|domain| self.index_key_priority(key, domain) > 0))
                        }).cloned().collect())
                    } else { None };
                    let mut result = Resolution::merge(parts, union);
                    if let Some(keys) = common { result.signature_keys = keys; }
                    result
                })
            }
            Ty::Inter(parts) => {
                let found: Vec<_> = parts.iter().filter_map(|part| self.index_signature::<METADATA, KEYS>(part, index, receiver)).collect();
                (!found.is_empty()).then(|| Resolution::merge(found, intersection))
            }
            Ty::Prim(_) | Ty::Lit(..) => {
                let apparent = self.apparent(object);
                if matches!(apparent, Ty::Named(..)) {
                    self.index_signature::<METADATA, KEYS>(&apparent, index, receiver)
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    fn index_from<'m, const METADATA: bool, const KEYS: bool>(&self, members: impl Iterator<Item = &'m Member>, index: &Ty, env: &Env, owner: Option<super::super::modules::DeclRef>) -> Option<Resolution> {
        let found: Vec<_> = members.filter(|member| member.kind == MemberKind::Index).filter_map(|member| {
            let key = self.eval(member.index_key.as_ref()?, env);
            let priority = if KEYS { 1 } else { self.index_key_priority(index, &key) };
            (priority > 0).then(|| {
                let ty = member.ty.as_ref().map_or(Ty::Unknown, |value| self.eval(value, env));
                let mut result = Resolution::signature::<METADATA>(ty, key);
                if METADATA {
                    result.declarations.push(MemberDeclaration { owner, member: member.clone(), environment: Some(env.clone()) });
                }
                (priority, result)
            })
        }).collect();
        let best = found.iter().map(|(priority, _)| *priority).max()?;
        Some(Resolution::merge(found.into_iter().filter_map(|(priority, value)| (priority == best).then_some(value)).collect(), intersection))
    }

    fn index_key_priority(&self, index: &Ty, key: &Ty) -> u8 {
        let numeric = matches!(index, Ty::Prim("number") | Ty::Lit(_, "number")) || literal_key(index).is_some_and(|name| {
            super::super::statics::number(&name).is_some_and(|value| super::super::statics::number_string(value) == name)
        });
        if numeric && matches!(key, Ty::Prim("number")) {
            3
        } else if self.assignable(index, key) == Some(true) {
            2
        } else if numeric && matches!(key, Ty::Prim("string")) {
            1
        } else {
            0
        }
    }
}

fn literal_key(index: &Ty) -> Option<String> {
    match index {
        Ty::Lit(value, "string") => super::super::regex::cook(value),
        Ty::Lit(value, "number") => super::super::statics::number(value).map(super::super::statics::number_string),
        _ => None,
    }
}

fn numeric_index(index: &Ty) -> Option<usize> {
    literal_key(index)?.parse().ok()
}

fn intersection(parts: Vec<Ty>) -> Ty {
    if parts.iter().any(Ty::is_opaque) {
        Ty::Unknown
    } else if parts.iter().any(|part| matches!(part, Ty::Any)) {
        Ty::Any
    } else if parts.len() == 1 {
        parts.into_iter().next().expect("one")
    } else {
        Ty::Inter(parts)
    }
}

fn union(parts: Vec<Ty>) -> Ty {
    if parts.iter().any(Ty::is_opaque) { Ty::Unknown } else { Ty::union(parts) }
}

#[cfg(test)]
mod tests {
    use crate::engine::tests::native_linter;
    use crate::engine::tests::with_native_file;
    use super::Ty;
    use std::path::Path;

    #[test]
    fn readonly_dynamic_indexes_keep_source_values_and_key_domains() {
        let linter = native_linter("in ts_expression_statement.expression rule unsafe-result { when contains(default(type_flags, []), 'any')\nwarn 'unsafe result' }");
        let source = "type Readonly<T> = { readonly [P in keyof T]: T[P] }; declare const at: number; declare const key: string; declare const array: Readonly<any[]>; declare const dictionary: Readonly<{ [name: string]: any }>; array[at]; array[key]; dictionary[key];";
        let diagnostics = linter.lint(Path::new("/root/lib.fixture.d.ts"), source);
        let spans: Vec<_> = diagnostics.iter().map(|d| &source[d.start..d.end]).collect();
        assert_eq!(spans, ["array[at]", "dictionary[key]"]);
    }

    #[test]
    fn index_resolution_distinguishes_declared_unknown_properties_from_signature_fallback() {
        let source = "interface Base<T> { [key: string]: T; named: unknown; } interface Child extends Base<number> {} declare const object: Child; const result = /*@*/object;";
        with_native_file(source, |file, nodes| {
            let checker = &file.typing().checker;
            let object = file.type_of(nodes[0]);
            for (name, declared, from_index) in [("named", true, false), ("other", false, true)] {
                let key = Ty::Lit(format!("'{name}'").into(), "string");
                let resolved = checker.index_resolution(&object, &key);
                assert_eq!((resolved.declared, resolved.from_index), (declared, from_index), "{resolved:?}");
                assert_eq!(checker.print(&resolved.ty), checker.print(&checker.indexed_type(&object, &key)));
            }
            assert!(checker.index_signature_keys(&object).iter().any(|key| matches!(key, Ty::Prim("string"))));
        });
    }

    #[test]
    fn signature_key_queries_keep_inherited_domains_but_only_common_union_domains() {
        for (ty, string, number) in [
            ("Child", true, true),
            ("{ [key: string]: unknown } | { [key: number]: unknown }", false, true),
            ("{ [key: string]: unknown } | { value: number }", false, false),
        ] {
            let source = format!("interface Base {{ [key: string]: unknown; }} interface Child extends Base {{ [key: number]: number; }} declare const object: {ty}; const result = /*@*/object;");
            with_native_file(&source, |file, nodes| {
                let keys = file.typing().checker.index_signature_keys(&file.type_of(nodes[0]));
                assert_eq!(keys.iter().any(|key| matches!(key, Ty::Prim("string"))), string, "{source}: {keys:?}");
                assert_eq!(keys.iter().any(|key| matches!(key, Ty::Prim("number"))), number, "{source}: {keys:?}");
            });
        }
    }
}
