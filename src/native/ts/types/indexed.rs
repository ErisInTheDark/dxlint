//! Indexed access: explicit properties first, then applicable index signatures.

use super::{Checker, Env, Ty, MAX_DEPTH};
use super::super::summary::{Member, MemberKind};

impl Checker<'_> {
    pub fn indexed_type(&self, object: &Ty, index: &Ty) -> Ty {
        self.deeper(|| {
            if let Ty::Union(keys) = index {
                return Ty::union(keys.iter().map(|key| self.indexed_type(object, key)).collect());
            }
            if let Ty::Union(parts) = object {
                let found: Vec<_> = parts.iter().filter(|part| !matches!(part, Ty::Null | Ty::Undefined)).map(|part| self.indexed_type(part, index)).collect();
                return if found.iter().any(Ty::is_opaque) { Ty::Unknown } else { Ty::union(found) };
            }
            if let Some(name) = literal_key(index) {
                let found = self.member(object, &name);
                if !found.is_opaque() {
                    return found;
                }
                let mut names = Vec::new();
                self.member_names(object, &mut names);
                if names.iter().any(|key| **key == name) {
                    return found;
                }
            }
            self.index_signature(object, index, object).unwrap_or(Ty::Unknown)
        })
    }

    fn index_signature(&self, object: &Ty, index: &Ty, receiver: &Ty) -> Option<Ty> {
        let depth = self.depth.get();
        if depth >= MAX_DEPTH {
            return Some(Ty::Unknown);
        }
        self.depth.set(depth + 1);
        let found = self.index_signature_inner(object, index, receiver);
        self.depth.set(depth);
        found
    }

    fn index_signature_inner(&self, object: &Ty, index: &Ty, receiver: &Ty) -> Option<Ty> {
        if let Some((key, value)) = self.record_parts(object) {
            return (self.index_key_priority(index, key) > 0).then(|| value.clone());
        }
        if self.property_filter(object).is_some() {
            return None;
        }
        match object {
            Ty::Any => Some(Ty::Any),
            Ty::Unknown | Ty::IntrinsicUnknown | Ty::Param(_) => Some(Ty::Unknown),
            Ty::Array(element) => (self.index_key_priority(index, &Ty::Prim("number")) > 0).then(|| (*element.element).clone()),
            Ty::Tuple(tuple) if self.index_key_priority(index, &Ty::Prim("number")) > 0 => {
                if let Some(at) = numeric_index(index) {
                    return Some(tuple.at(at));
                }
                if literal_key(index).is_some() {
                    return Some(Ty::Unknown);
                }
                let mut elements = Vec::new();
                for (at, element) in tuple.elements.iter().enumerate() {
                    elements.push(if element.rest {
                        self.indexed_type(&element.ty, &Ty::Prim("number"))
                    } else {
                        tuple.at(at)
                    });
                }
                Some(Ty::union(elements))
            }
            Ty::Obj(members, env) => {
                let env = Env { this: Some(Box::new(receiver.clone())), ..(**env).clone() };
                self.index_from(members.iter(), index, &env)
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
                    if let Some(found) = self.index_from(declaration.members.iter().filter(|member| !member.is_static), index, &env) {
                        own.push(found);
                    }
                    for base in self.bases(declaration) {
                        inherited.push(self.eval(base, &env));
                    }
                }
                if !own.is_empty() {
                    return Some(intersection(own));
                }
                let found: Vec<_> = inherited.iter().filter_map(|base| self.index_signature(base, index, receiver)).collect();
                (!found.is_empty()).then(|| intersection(found))
            }
            Ty::Union(parts) => {
                let found: Option<Vec<_>> = parts.iter().filter(|part| !matches!(part, Ty::Null | Ty::Undefined)).map(|part| self.index_signature(part, index, part)).collect();
                found.filter(|parts| !parts.is_empty()).map(|parts| if parts.iter().any(Ty::is_opaque) { Ty::Unknown } else { Ty::union(parts) })
            }
            Ty::Inter(parts) => {
                let found: Vec<_> = parts.iter().filter_map(|part| self.index_signature(part, index, receiver)).collect();
                (!found.is_empty()).then(|| intersection(found))
            }
            Ty::Prim(_) | Ty::Lit(..) => {
                let apparent = self.apparent(object);
                if matches!(apparent, Ty::Named(..)) {
                    self.index_signature(&apparent, index, receiver)
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    fn index_from<'m>(&self, members: impl Iterator<Item = &'m Member>, index: &Ty, env: &Env) -> Option<Ty> {
        let found: Vec<_> = members.filter(|member| member.kind == MemberKind::Index).filter_map(|member| {
            let key = self.eval(member.index_key.as_ref()?, env);
            let priority = self.index_key_priority(index, &key);
            (priority > 0).then(|| (priority, member.ty.as_ref().map_or(Ty::Unknown, |value| self.eval(value, env))))
        }).collect();
        let best = found.iter().map(|(priority, _)| *priority).max()?;
        Some(intersection(found.into_iter().filter_map(|(priority, value)| (priority == best).then_some(value)).collect()))
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
