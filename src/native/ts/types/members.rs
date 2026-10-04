//! Member lookup: instance members through heritage and merged interfaces,
//! statics, namespaces and modules as values, and member name lists.

use std::sync::Arc;

use super::super::modules::DeclRef;
use super::super::summary::{DeclKind, Member, MemberKind, Sig, TypeSyn};
use super::{Callable, Checker, Env, Sym, Ty, MAX_DEPTH};

impl Checker<'_> {
    /// Primitive backing a specific enum member, without erasing enum identity in `member`.
    pub fn enum_member_base(&self, ty: &Ty, name: &str) -> Option<Ty> {
        let sym = match ty { Ty::Ns(sym) | Ty::Named(sym, _) => sym, _ => return None };
        for &decl in sym.iter() {
            let summary = self.summary_of(decl.file)?;
            let owner = &summary.decls[decl.decl as usize];
            if owner.kind != DeclKind::Enum { continue }
            if let Some(member) = owner.members.iter().find(|member| &*member.name == name) {
                return Some(member.ty.as_ref().map_or(Ty::Prim("number"), |ty| self.eval(ty, &self.decl_env(decl))));
            }
        }
        None
    }

    pub fn is_lib_decl(&self, decl: DeclRef) -> bool {
        let path = self.project.path(decl.file);
        path.file_name().and_then(|name| name.to_str()).is_some_and(|name| name.starts_with("lib.") && name.ends_with(".d.ts"))
    }

    pub fn is_builtin_symbol(&self, sym: &[DeclRef], name: &str) -> bool {
        sym.iter().any(|&decl| self.is_lib_decl(decl) && self.summary_of(decl.file).is_some_and(|summary| &*summary.decls[decl.decl as usize].name == name))
    }

    pub fn is_builtin_type_or_base(&self, ty: &Ty, name: &str) -> bool {
        self.builtin_type_or_base(ty, name, &mut Vec::new())
    }

    fn builtin_type_or_base(&self, ty: &Ty, name: &str, seen: &mut Vec<DeclRef>) -> bool {
        match ty {
            Ty::Named(sym, args) => {
                if self.is_builtin_symbol(sym, name) { return true }
                for &r in sym.iter() {
                    if seen.contains(&r) || seen.len() >= MAX_DEPTH as usize { continue }
                    seen.push(r);
                    let Some(summary) = self.summary_of(r.file) else { continue };
                    let decl = &summary.decls[r.decl as usize];
                    let mut env = self.decl_env(r);
                    env.tparams = self.bind_tparams(&decl.type_params, args.clone(), &env);
                    if self.bases(decl).iter().any(|base| self.builtin_type_or_base(&self.eval(base, &env), name, seen)) { return true }
                }
                false
            }
            Ty::Inter(parts) => parts.iter().any(|part| self.builtin_type_or_base(part, name, seen)),
            _ => false,
        }
    }

    /// The type of member `name` of a value of type `t`.
    pub fn member(&self, t: &Ty, name: &str) -> Ty {
        self.deeper(|| self.member_inner(t, name, t))
    }

    pub fn global_named(&self, name: &str, args: Vec<Ty>) -> Ty {
        let types: Vec<DeclRef> = self
            .globals
            .names
            .get(name)
            .map(|r| r.iter().copied().filter(|&d| matches!(self.kind_of(d), Some(DeclKind::Interface | DeclKind::Class))).collect())
            .unwrap_or_default();
        if types.is_empty() {
            Ty::Unknown
        } else {
            Ty::Named(types.into(), args)
        }
    }

    /// The interface primitives and arrays get their members from.
    pub fn apparent(&self, t: &Ty) -> Ty {
        match t {
            Ty::Prim("string") | Ty::Lit(_, "string") => self.global_named("String", vec![]),
            Ty::Prim("number") | Ty::Lit(_, "number") => self.global_named("Number", vec![]),
            Ty::Prim("boolean") | Ty::Lit(_, "boolean") => self.global_named("Boolean", vec![]),
            Ty::Prim("bigint") | Ty::Lit(_, "bigint") => self.global_named("BigInt", vec![]),
            Ty::Prim("symbol") => self.global_named("Symbol", vec![]),
            Ty::Array(e) => self.global_named(if e.readonly { "ReadonlyArray" } else { "Array" }, vec![(*e.element).clone()]),
            Ty::Tuple(items) => self.global_named(if items.readonly { "ReadonlyArray" } else { "Array" }, vec![Ty::union(items.iter().cloned().collect())]),
            Ty::Fn(_) | Ty::Class(_) => self.global_named("Function", vec![]),
            other => other.clone(),
        }
    }

    /// Every step of a member lookup counts against the depth limit:
    /// heritage can loop (`interface A extends B`, `B extends A`).
    fn member_inner(&self, t: &Ty, name: &str, receiver: &Ty) -> Ty {
        self.deeper(|| self.member_core(t, name, receiver))
    }

    fn member_core(&self, t: &Ty, name: &str, receiver: &Ty) -> Ty {
        if let Some(filter) = self.property_filter(t) {
            if filter.names(self, 0).is_some_and(|names| !names.iter().any(|key| &**key == name)) { return Ty::Unknown }
            return if filter.allows(name) == Some(true) { self.member_inner(filter.source, name, filter.source) } else { Ty::Unknown };
        }
        if let Some((key, value)) = self.record_parts(t) {
            return if super::utilities::accepts_key(key, name) == Some(true) { value.clone() } else { Ty::Unknown };
        }
        match t {
            Ty::Any => Ty::Any,
            Ty::Array(element) if name.parse::<usize>().is_ok() => (*element.element).clone(),
            Ty::Tuple(tuple) if name == "length" => tuple.length(),
            Ty::Tuple(tuple) if name.parse::<usize>().is_ok() => tuple.at(name.parse().expect("checked index")),
            Ty::Named(symbols, args) if args.len() == 1 && name.parse::<usize>().is_ok() && self.is_builtin_symbol(symbols, "ReadonlyArray") => args[0].clone(),
            Ty::Named(sym, args) => {
                // Enum members are the enum type.
                if self.is_enum(sym) {
                    return self.member_inner(&Ty::Prim("number"), name, receiver);
                }
                self.instance_member(sym, args, name, receiver)
            }
            Ty::Union(items) => {
                let found: Vec<Ty> = items
                    .iter()
                    .filter(|i| !matches!(i, Ty::Undefined | Ty::Null))
                    .map(|i| self.member_inner(i, name, i))
                    .collect();
                if found.iter().any(Ty::is_opaque) {
                    Ty::Unknown
                } else {
                    Ty::union(found)
                }
            }
            // Every constituent that has it contributes (`T & { on: any }`
            // makes `on` any).
            Ty::Inter(items) => {
                let found: Vec<Ty> = items.iter().map(|i| self.member_inner(i, name, receiver)).filter(|t| !t.is_opaque()).collect();
                match found.len() {
                    0 => Ty::Unknown,
                    1 => found.into_iter().next().expect("one"),
                    _ if found.iter().any(|t| matches!(t, Ty::Any)) => Ty::Any,
                    _ => Ty::Inter(found),
                }
            }
            Ty::Obj(members, env) => {
                let env = Env { this: Some(Box::new(receiver.clone())), ..(**env).clone() };
                self.member_from(members.iter(), name, &env).unwrap_or(Ty::Unknown)
            }
            Ty::Struct(props) => match props.iter().find(|(n, _)| &**n == name) {
                Some((_, t)) => t.clone(),
                None => self.member_inner(&self.global_named("Object", vec![]), name, receiver),
            },
            Ty::Class(sym) => {
                if name == "prototype" {
                    return Ty::Named(sym.clone(), Vec::new());
                }
                self.static_member(sym, name).unwrap_or_else(|| self.member_inner(&self.global_named("Function", vec![]), name, receiver))
            }
            Ty::Ns(sym) => {
                if self.is_enum(sym) {
                    let enum_sym: Sym = sym.iter().copied().filter(|&r| self.kind_of(r) == Some(DeclKind::Enum)).collect();
                    return Ty::Named(enum_sym, Vec::new());
                }
                let found = self.namespace_member(sym, name);
                self.value_of(&found)
            }
            Ty::Module(m) => {
                let found = self.module_member(m.0, name);
                self.value_of(&found)
            }
            Ty::Prim(_) | Ty::Lit(..) | Ty::Array(_) | Ty::Tuple(_) | Ty::Fn(_) => {
                let apparent = self.apparent(t);
                if apparent.is_opaque() {
                    return Ty::Unknown;
                }
                self.member_inner(&apparent, name, receiver)
            }
            _ => Ty::Unknown,
        }
    }

    fn static_member(&self, sym: &[DeclRef], name: &str) -> Option<Ty> {
        let d = self.depth.get();
        if d >= MAX_DEPTH {
            return None;
        }
        self.depth.set(d + 1);
        let found = self.static_member_core(sym, name);
        self.depth.set(d);
        found
    }

    fn static_member_core(&self, sym: &[DeclRef], name: &str) -> Option<Ty> {
        for &r in sym {
            let s = self.summary_of(r.file)?;
            let decl = &s.decls[r.decl as usize];
            let env = self.decl_env(r);
            if let Some(t) = self.member_from(decl.members.iter().filter(|m| m.is_static), name, &env) {
                return Some(t);
            }
            // Statics are inherited too.
            if let Some(TypeSyn::Ref(path, _)) = decl.extends.first().filter(|_| decl.kind == DeclKind::Class) {
                if let Ty::Class(base) = self.value_path(path, &env) {
                    if base.iter().any(|b| sym.contains(b)) {
                        continue;
                    }
                    if let Some(t) = self.static_member(&base, name) {
                        return Some(t);
                    }
                }
            }
        }
        // A class merged with a namespace.
        let ns = self.namespace_member(sym, name);
        (!ns.is_empty()).then(|| self.value_of(&ns))
    }

    fn instance_member(&self, sym: &[DeclRef], args: &[Ty], name: &str, receiver: &Ty) -> Ty {
        let mut sigs: Vec<Arc<Sig>> = Vec::new();
        let mut sig_env: Option<Env> = None;
        for &r in sym {
            let Some(s) = self.summary_of(r.file) else { continue };
            let decl = &s.decls[r.decl as usize];
            let mut env = self.decl_env(r);
            env.tparams = self.bind_tparams(&decl.type_params, args.to_vec(), &env);
            env.this = Some(Box::new(receiver.clone()));
            for m in decl.members.iter().filter(|m| !m.is_static && &*m.name == name) {
                match m.kind {
                    MemberKind::Method => {
                        sigs.extend(m.sigs.iter().cloned());
                        sig_env.get_or_insert_with(|| env.clone());
                    }
                    MemberKind::Prop => return self.prop_type(m, &env),
                    _ => {}
                }
            }
        }
        if let Some(env) = sig_env {
            return Ty::Fn(Arc::new(Callable { sigs, env: Arc::new(env), ret: None }));
        }
        // Inherited.
        for &r in sym {
            let Some(s) = self.summary_of(r.file) else { continue };
            let decl = &s.decls[r.decl as usize];
            let mut env = self.decl_env(r);
            env.tparams = self.bind_tparams(&decl.type_params, args.to_vec(), &env);
            for base in self.bases(decl) {
                let base_ty = self.eval(base, &env);
                if base_ty.is_opaque() {
                    continue;
                }
                let t = self.member_inner(&base_ty, name, receiver);
                if !t.is_opaque() {
                    return t;
                }
            }
        }
        Ty::Unknown
    }

    fn member_from<'m>(&self, members: impl Iterator<Item = &'m Member>, name: &str, env: &Env) -> Option<Ty> {
        let mut sigs = Vec::new();
        for m in members.filter(|m| &*m.name == name) {
            match m.kind {
                MemberKind::Method => sigs.extend(m.sigs.iter().cloned()),
                MemberKind::Prop => return Some(self.prop_type(m, env)),
                _ => {}
            }
        }
        (!sigs.is_empty()).then(|| Ty::Fn(Arc::new(Callable { sigs, env: Arc::new(env.clone()), ret: None })))
    }

    /// Every member name of `t`, inherited ones included (for unions, the
    /// names of any constituent).
    pub fn member_names(&self, t: &Ty, out: &mut Vec<Box<str>>) {
        if let Some((keys, _)) = self.record_parts(t) {
            if let Some(names) = super::utilities::required_keys(keys) {
                out.extend(names.into_iter().map(String::into_boxed_str));
            }
            return;
        }
        if self.depth.get() >= MAX_DEPTH {
            return;
        }
        self.depth.set(self.depth.get() + 1);
        if let Some(filter) = self.property_filter(t) {
            if let Some(names) = filter.names(self, 0) { out.extend(names); }
        } else { match t {
            // Named types repeat a lot (and their heritage is deep): the
            // project remembers their member names.
            // An enum value has its primitive's members, not the enum's.
            Ty::Named(sym, _) if self.is_enum(sym) => {
                let prim = Ty::Prim(if self.is_string_enum(sym) { "string" } else { "number" });
                self.member_names(&prim, out);
            }
            Ty::Named(sym, args) => {
                let key = (self.globals.id, sym.to_vec(), args.iter().map(|a| self.print(a)).collect::<Vec<_>>());
                let names = self.project.members_or(key, self.file, || {
                    let mut names = Vec::new();
                    self.named_member_names(sym, args, &mut names);
                    names.into()
                });
                if out.is_empty() {
                    out.extend(names.iter().cloned());
                } else {
                    let seen: std::collections::HashSet<Box<str>> = out.iter().cloned().collect();
                    out.extend(names.iter().filter(|n| !seen.contains(*n)).cloned());
                }
            }
            Ty::Union(items) | Ty::Inter(items) => items.iter().for_each(|i| self.member_names(i, out)),
            Ty::Obj(members, _) => out.extend(members.iter().map(|m| m.name.clone())),
            Ty::Struct(props) => out.extend(props.iter().map(|(n, _)| n.clone())),
            Ty::Prim(_) | Ty::Lit(..) | Ty::Array(_) | Ty::Tuple(_) | Ty::Fn(_) => {
                let a = self.apparent(t);
                if !matches!(a, Ty::Prim(_) | Ty::Lit(..) | Ty::Array(_) | Ty::Tuple(_) | Ty::Fn(_)) {
                    self.member_names(&a, out);
                }
            }
            _ => {}
        } }
        self.depth.set(self.depth.get() - 1);
    }

    fn named_member_names(&self, sym: &[DeclRef], args: &[Ty], out: &mut Vec<Box<str>>) {
        for &r in sym {
            let Some(s) = self.summary_of(r.file) else { continue };
            let decl = &s.decls[r.decl as usize];
            for m in decl.members.iter().filter(|m| !m.is_static) {
                if !out.contains(&m.name) {
                    out.push(m.name.clone());
                }
            }
            let mut env = self.decl_env(r);
            env.tparams = self.bind_tparams(&decl.type_params, args.to_vec(), &env);
            for base in self.bases(decl) {
                let b = self.eval(base, &env);
                self.member_names(&b, out);
            }
        }
    }

    /// The types a declaration inherits members from: a class's `extends`
    /// (its `implements` list adds none), every interface `extends`.
    pub(crate) fn bases<'d>(&self, decl: &'d super::super::summary::Decl) -> Vec<&'d TypeSyn> {
        match decl.kind {
            DeclKind::Class => decl.extends.first().into_iter().collect(),
            _ => decl.extends.iter().collect(),
        }
    }

    pub fn is_enum(&self, sym: &[DeclRef]) -> bool {
        sym.first().is_some_and(|&r| self.kind_of(r) == Some(DeclKind::Enum))
    }

}
