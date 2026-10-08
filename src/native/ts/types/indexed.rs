//! Indexed access: explicit properties first, then applicable index signatures.

use super::{Checker, Env, Ty, MAX_DEPTH};
use super::super::summary::{Member, MemberKind};
use super::members::{MemberDeclaration, Resolution};

#[cfg(test)]
thread_local! {
    static INDEX_WORK: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

impl Checker<'_> {
    /// Declared key types for an indexed read, retaining computed-key identity.
    pub fn index_contract_keys(&self, object: &Ty, index: &Ty) -> Ty {
        self.deeper(|| {
            if let Some(source) = self.readonly_source(object) { return self.index_contract_keys(source, index) }
            if let Some(name) = literal_key(index) {
                let property = self.member_resolution(object, &name);
                if property.declared && property.declarations.iter().all(|declaration| {
                    declaration.member.kind == MemberKind::Prop && declaration.member.computed_key.is_none()
                }) { return index.clone() }
            }
            if let Some((keys, _)) = self.record_parts(object) {
                return keys.clone();
            }
            let mut keys = self.index_signature_keys(object);
            match object {
                Ty::Obj(members, env) => keys.extend(self.member_key_contracts(members.iter(), env)),
                Ty::Named(symbols, arguments) => {
                    for &reference in symbols.iter() {
                        let Some(summary) = self.summary_of(reference.file) else { continue };
                        let declaration = &summary.decls[reference.decl as usize];
                        let mut env = self.decl_env(reference);
                        env.tparams = self.bind_tparams(&declaration.type_params, arguments.clone(), &env);
                        keys.extend(self.member_key_contracts(declaration.members.iter().filter(|member| !member.is_static), &env));
                        for base in self.bases(declaration) {
                            keys.push(self.index_contract_keys(&self.eval(base, &env), index));
                        }
                    }
                }
                Ty::Union(parts) | Ty::Inter(parts) => {
                    keys.extend(parts.iter().map(|part| self.index_contract_keys(part, index)));
                }
                Ty::Unknown | Ty::IntrinsicUnknown | Ty::Any | Ty::Param(_) => return Ty::Unknown,
                _ => {}
            }
            Ty::union(keys)
        })
    }

    fn member_key_contracts<'m>(&self, members: impl Iterator<Item = &'m Member>, env: &Env) -> Vec<Ty> {
        members.filter_map(|member| {
            if member.kind == MemberKind::Mapped {
                member.index_key.as_ref().map(|key| self.eval(key, env))
            } else {
                member.computed_key.as_ref().map(|path| self.eval(&super::super::summary::TypeSyn::Query(path.clone(), Vec::new()), env))
            }
        }).collect()
    }

    fn key_domains_overlap(&self, index: &Ty, key: &Ty) -> bool {
        let domain = |ty: &Ty| match ty {
            Ty::Named(symbols, _) if self.is_enum(symbols) =>
                Ty::union(self.enum_domains(symbols).into_iter().map(Ty::Prim).collect()),
            ty => ty.clone(),
        };
        match (index, key) {
            (Ty::Union(parts), _) => parts.iter().any(|part| self.key_domains_overlap(part, key)),
            (_, Ty::Union(parts)) => parts.iter().any(|part| self.key_domains_overlap(index, part)),
            _ => self.assignable(&domain(index), &domain(key)) != Some(false),
        }
    }

    pub(super) fn mapped_resolution<const METADATA: bool>(&self, member: &Member, index: &Ty, env: &Env) -> Option<Resolution> {
        let key = self.eval(member.index_key.as_ref()?, env);
        if !self.key_domains_overlap(index, &key) { return None }
        let mut environment = env.clone();
        environment.tparams.push((member.name.clone(), index.clone()));
        let ty = member.ty.as_ref().map_or(Ty::Unknown, |value| self.eval(value, &environment));
        let ty = if member.optional { ty.or_undefined() } else { ty };
        let mut resolution = if super::utilities::open_key_domain(&key) {
            Resolution::signature::<METADATA>(ty, key)
        } else { Resolution::typed(ty) };
        resolution.declared = true;
        if METADATA {
            resolution.declarations.push(MemberDeclaration { owner: None, member: member.clone(), environment: Some(environment), origin: None });
        }
        Some(resolution)
    }

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
        #[cfg(test)]
        INDEX_WORK.with(|work| work.set(work.get() + 1));
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
        let found: Vec<_> = members.filter_map(|member| {
            if member.kind == MemberKind::Mapped {
                if KEYS { return None }
                return self.mapped_resolution::<METADATA>(member, index, env).map(|resolution| (2, resolution));
            }
            if member.kind != MemberKind::Index { return None }
            let key = self.eval(member.index_key.as_ref()?, env);
            let priority = if KEYS { 1 } else { self.index_key_priority(index, &key) };
            (priority > 0).then(|| {
                let ty = member.ty.as_ref().map_or(Ty::Unknown, |value| self.eval(value, env));
                let mut result = Resolution::signature::<METADATA>(ty, key);
                if METADATA {
                    result.declarations.push(MemberDeclaration { owner, member: member.clone(), environment: Some(env.clone()), origin: None });
                }
                (priority, result)
            })
        }).collect();
        let best = found.iter().map(|(priority, _)| *priority).max()?;
        Some(Resolution::merge(found.into_iter().filter_map(|(priority, value)| (priority == best).then_some(value)).collect(), intersection))
    }

    fn index_key_priority(&self, index: &Ty, key: &Ty) -> u8 {
        let numeric = matches!(index, Ty::Prim("number") | Ty::Lit(_, "number")) || literal_key(index).is_some_and(|name| {
            super::super::statics::number(&name).is_some_and(|value| crate::expr::value::js_number_text(value) == name)
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
        Ty::Lit(value, "number") => super::super::statics::number(value).map(crate::expr::value::js_number_text),
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
        Ty::intersection(parts)
    }
}

fn union(parts: Vec<Ty>) -> Ty {
    if parts.iter().any(Ty::is_opaque) { Ty::Unknown } else { Ty::union(parts) }
}

#[cfg(test)]
mod tests {
    use crate::engine::tests::native_linter;
    use crate::engine::tests::with_native_file;
    use super::{Ty, INDEX_WORK};
    use std::path::Path;

    #[test]
    fn mapped_values_bind_the_selected_key_and_keep_optional_readonly_metadata() {
        for (mapping, readonly, optional) in [
            ("readonly [P in \"a\" | \"b\"]?: P", true, true),
            ("+ readonly [P in \"a\" | \"b\"] + ?: P", true, true),
            ("- readonly [P in \"a\" | \"b\"] - ?: P", false, false),
            ("+ /*add*/ readonly [P in \"a\" | \"b\"] + /*add*/ ?: P", true, true),
            ("- /*remove*/ readonly [P in \"a\" | \"b\"] - /*remove*/ ?: P", false, false),
        ] {
            with_native_file(&format!("declare const value: {{ {mapping} }}; const result = /*@*/value;"), |file, nodes| {
                let checker = &file.typing().checker;
                let object = file.type_of(nodes[0]);
                let key = Ty::Lit("\"a\"".into(), "string");
                let result = checker.indexed_type(&object, &key);
                let expected = if optional { key.clone().or_undefined() } else { key };
                assert_eq!(checker.equivalent(&result, &expected), Some(true), "{mapping}");
                assert_eq!(checker.property_readonly(&object, "a"), Some(readonly), "{mapping}");
                assert!(checker.index_signature_keys(&object).is_empty());
            });
        }
    }

    #[test]
    fn key_contracts_retain_all_declared_domains_while_plain_properties_win() {
        with_native_file("enum Fruit { Apple } enum Vegetable { Asparagus = 'asparagus' } declare const value: { [Fruit.Apple]: string; [Vegetable.Asparagus]: string; other: string }; const fruit = /*@*/Fruit.Apple; const vegetable = /*@*/Vegetable.Asparagus; const object = /*@*/value;", |file, nodes| {
            let checker = &file.typing().checker;
            let object = file.type_of(nodes[2]);
            let keys = checker.index_contract_keys(&object, &Ty::Prim("number"));
            assert_eq!(checker.assignable(&file.type_of(nodes[0]), &keys), Some(true));
            assert_eq!(checker.assignable(&file.type_of(nodes[1]), &keys), Some(true));
            let plain = Ty::Lit("'other'".into(), "string");
            let keys = checker.index_contract_keys(&object, &plain);
            assert_eq!(checker.equivalent(&plain, &keys), Some(true));
        });
    }

    #[test]
    fn mapped_reads_keep_open_key_origins_separate_from_declared_properties() {
        for (domain, from_index) in [
            ("string", true), ("number", true), ("string | number", true),
            ("'a' | 'b'", false), ("Fruit", false),
        ] {
            with_native_file(&format!("enum Fruit {{ Apple }} declare const value: {{ [P in {domain}]: string }}; const object = /*@*/value;"), |file, nodes| {
                let checker = &file.typing().checker;
                let object = file.type_of(nodes[0]);
                let key = if domain == "number" || domain == "Fruit" { Ty::Lit("0".into(), "number") } else { Ty::Lit("'a'".into(), "string") };
                let resolved = checker.index_resolution(&object, &key);
                assert_eq!(resolved.from_index, from_index, "{domain}: {resolved:?}");
                assert_eq!(resolved.signature_keys.is_empty(), !from_index, "{domain}: {resolved:?}");
                assert!(resolved.declared);
                assert_eq!(checker.equivalent(&resolved.ty, &Ty::Prim("string")), Some(true));
            });
        }
    }

    #[test]
    fn self_members_keep_index_lookup_work_bounded() {
        let source = "interface A { next: this; first: number; [key: number]: number } interface B { next: this; second: string; [key: number]: number } declare const value: A & B; const result = /*@*/value;";
        with_native_file(source, |file, nodes| {
            let checker = &file.typing().checker;
            let mut ty = file.type_of(nodes[0]);
            for _ in 0..3 { ty = checker.member(&ty, "next"); }
            INDEX_WORK.with(|work| work.set(0));
            let element = checker.indexed_type(&ty, &Ty::Prim("number"));
            let work = INDEX_WORK.with(|work| work.get());
            assert!(work <= 128, "repeated self-member contracts inflated index work to {work}");
            assert_eq!(checker.equivalent(&element, &Ty::Prim("number")), Some(true));
            assert_eq!(checker.equivalent(&checker.member(&ty, "first"), &Ty::Prim("number")), Some(true));
            assert_eq!(checker.equivalent(&checker.member(&ty, "second"), &Ty::Prim("string")), Some(true));
        });
    }

    #[test]
    fn readonly_dynamic_indexes_keep_source_values_and_key_domains() {
        let linter = native_linter("in ts_expression_statement.expression rule unsafe-result { when { value (type_flags.nonempty ?? []); contains 'any' }\nwarn 'unsafe result' }");
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
