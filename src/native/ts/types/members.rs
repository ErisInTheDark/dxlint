//! Member lookup: instance members through heritage and merged interfaces,
//! statics, namespaces and modules as values, and member name lists.

use std::sync::Arc;

use super::super::modules::DeclRef;
use super::super::summary::{DeclKind, Member, MemberKind, Sig, TypeSyn, Visibility};
use super::{Callable, Checker, Env, Sym, Ty, MAX_DEPTH};

#[derive(Debug, Clone)]
pub struct MemberDeclaration {
    pub owner: Option<DeclRef>,
    pub member: Member,
    pub environment: Option<Env>,
}

#[derive(Debug, Clone)]
pub struct Resolution {
    pub ty: Ty,
    pub from_index: bool,
    pub declared: bool,
    pub declarations: Vec<MemberDeclaration>,
    pub(super) signature_keys: Vec<Ty>,
}

impl Resolution {
    pub(super) fn typed(ty: Ty) -> Self {
        Self { ty, from_index: false, declared: false, declarations: Vec::new(), signature_keys: Vec::new() }
    }

    pub(super) fn signature<const METADATA: bool>(ty: Ty, key: Ty) -> Self {
        let mut result = Self::typed(ty);
        result.from_index = true;
        if METADATA {
            match key {
                Ty::Union(parts) => result.signature_keys.extend(parts),
                key => result.signature_keys.push(key),
            }
        }
        result
    }

    pub(super) fn merge(parts: Vec<Self>, combine: impl FnOnce(Vec<Ty>) -> Ty) -> Self {
        let mut result = Self::typed(Ty::Unknown);
        result.declared = !parts.is_empty() && parts.iter().all(|part| part.declared);
        let types = parts.into_iter().map(|part| {
            result.from_index |= part.from_index;
            result.declarations.extend(part.declarations);
            result.signature_keys.extend(part.signature_keys);
            part.ty
        }).collect();
        result.ty = combine(types);
        result
    }

    fn property<const METADATA: bool>(name: &str, ty: Ty) -> Self {
        let mut result = Self::typed(ty);
        result.declared = true;
        if METADATA {
            result.declarations.push(MemberDeclaration {
                owner: None,
                environment: None,
                member: Member {
                    doc: None,
                    name: name.into(), kind: MemberKind::Prop, visibility: Visibility::Public,
                    signature: false, computed_key: None,
                    is_static: false, readonly: Some(false), optional: false, ty: None, index_key: None, sigs: Vec::new(), init: None,
                },
            });
        }
        result
    }
}

#[cfg(test)]
mod tests;

impl Checker<'_> {
    pub fn property_readonly(&self, object: &Ty, name: &str) -> Option<bool> {
        if let Some(source) = self.readonly_source(object) {
            return self.member_resolution(source, name).declared.then_some(true);
        }
        let resolution = self.member_resolution(object, name);
        if !resolution.declared { return None }
        let readonly = resolution.declarations.first()?.member.readonly?;
        resolution.declarations.iter().all(|declaration| declaration.member.readonly == Some(readonly)).then_some(readonly)
    }

    pub fn declaration_key_symbol<'m>(&self, declaration: &'m MemberDeclaration) -> Option<&'m str> {
        let key = declaration.member.computed_key.as_deref()?;
        if key.len() != 2 || &*key[0] != "Symbol" { return None }
        let symbol = self.lookup(declaration.environment.as_ref()?, "Symbol");
        let globals = self.globals.names.get("Symbol")?;
        if symbol.is_empty() || !symbol.iter().all(|decl| globals.contains(decl) && self.is_lib_decl(*decl)) { return None }
        self.member_resolution(&self.value_of(&symbol), &key[1]).declared.then_some(&key[1])
    }

    pub fn member_resolution(&self, object: &Ty, name: &str) -> Resolution {
        self.member_query::<true>(object, name)
    }

    pub(super) fn member_query<const METADATA: bool>(&self, object: &Ty, name: &str) -> Resolution {
        self.resolution_depth(|| self.member_inner::<METADATA>(object, name, object))
    }

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

    /// Constructor value identity, including a class's value-side heritage.
    pub fn is_builtin_constructor_or_base(&self, ty: &Ty, name: &str) -> bool {
        let mut standard: Vec<_> = self.globals.names.get(name).into_iter().flatten().copied().filter(|&decl| self.is_lib_decl(decl)).collect();
        for decl in self.lookup(&Env::file(self.file), name) {
            if self.is_lib_decl(decl) && !standard.contains(&decl) { standard.push(decl); }
        }
        let constructor = self.value_of(&standard);
        self.builtin_constructor_or_base(ty, &constructor, &mut Vec::new())
    }

    fn builtin_constructor_or_base(&self, ty: &Ty, constructor: &Ty, seen: &mut Vec<DeclRef>) -> bool {
        match ty {
            Ty::Named(..) => match constructor {
                Ty::Named(symbols, _) => symbols.iter().any(|&decl| self.summary_of(decl.file).is_some_and(|summary| {
                    self.builtin_type_or_base(ty, &summary.decls[decl.decl as usize].name, &mut Vec::new())
                })),
                _ => false,
            },
            Ty::Class(symbols) => {
                if let Ty::Class(standard) = constructor {
                    if symbols.iter().any(|decl| standard.contains(decl)) { return true }
                }
                for &decl in symbols.iter() {
                    if seen.contains(&decl) || seen.len() >= MAX_DEPTH as usize { continue }
                    seen.push(decl);
                    let Some(summary) = self.summary_of(decl.file) else { continue };
                    let declaration = &summary.decls[decl.decl as usize];
                    if let Some(TypeSyn::Ref(path, _)) = declaration.extends.first().filter(|_| declaration.kind == DeclKind::Class) {
                        if self.builtin_constructor_or_base(&self.value_path(path, &self.decl_env(decl)), constructor, seen) { return true }
                    }
                }
                false
            },
            Ty::Union(parts) => !parts.is_empty() && parts.iter().all(|part| self.builtin_constructor_or_base(part, constructor, &mut seen.clone())),
            Ty::Inter(parts) => parts.iter().any(|part| self.builtin_constructor_or_base(part, constructor, &mut seen.clone())),
            _ => false,
        }
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
        self.member_query::<false>(t, name).ty
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
            Ty::Prim("object") => self.global_named("Object", vec![]),
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
    pub(super) fn resolution_depth(&self, f: impl FnOnce() -> Resolution) -> Resolution {
        let mut result = Resolution::typed(Ty::Unknown);
        // Share the checker's depth budget with type-only evaluation.
        let ty = self.deeper(|| {
            result = f();
            std::mem::replace(&mut result.ty, Ty::Unknown)
        });
        result.ty = ty;
        result
    }

    fn member_inner<const METADATA: bool>(&self, t: &Ty, name: &str, receiver: &Ty) -> Resolution {
        self.member_lookup::<METADATA, true>(t, name, receiver)
    }

    fn member_lookup<const METADATA: bool, const PROTOTYPE: bool>(&self, t: &Ty, name: &str, receiver: &Ty) -> Resolution {
        self.resolution_depth(|| self.member_core::<METADATA, PROTOTYPE>(t, name, receiver))
    }

    fn member_core<const METADATA: bool, const PROTOTYPE: bool>(&self, t: &Ty, name: &str, receiver: &Ty) -> Resolution {
        if let Some(source) = self.readonly_source(t) {
            return self.member_lookup::<METADATA, PROTOTYPE>(source, name, source);
        }
        if let Some(filter) = self.property_filter(t) {
            if filter.names(self, 0).is_some_and(|names| !names.iter().any(|key| &**key == name)) { return self.object_member::<METADATA, PROTOTYPE>(name, receiver) }
            return if filter.allows(name) == Some(true) { self.member_lookup::<METADATA, PROTOTYPE>(filter.source, name, filter.source) } else { self.object_member::<METADATA, PROTOTYPE>(name, receiver) };
        }
        if let Some((key, value)) = self.record_parts(t) {
            let explicit = (METADATA || value.is_opaque()) && super::utilities::required_keys(key).is_some_and(|keys| keys.iter().any(|key| key == name));
            if explicit { return Resolution::property::<METADATA>(name, value.clone()) }
            let prototype = self.object_member::<METADATA, PROTOTYPE>(name, receiver);
            if prototype.declared {
                return if super::utilities::required_keys(key).is_some_and(|keys| keys.iter().any(|key| key == name)) {
                    Resolution::property::<METADATA>(name, value.clone())
                } else { prototype };
            }
            return if super::utilities::accepts_key(key, name) == Some(true) {
                Resolution::signature::<METADATA>(value.clone(), if METADATA { key.clone() } else { Ty::Unknown })
            } else { Resolution::typed(Ty::Unknown) };
        }
        match t {
            Ty::Any => Resolution::typed(Ty::Any),
            Ty::Array(element) if name.parse::<usize>().is_ok() => Resolution::typed((*element.element).clone()),
            Ty::Array(_) if name == "length" => Resolution::property::<METADATA>(name, Ty::Prim("number")),
            Ty::Tuple(tuple) if name == "length" => Resolution::property::<METADATA>(name, tuple.length()),
            Ty::Tuple(tuple) if name.parse::<usize>().is_ok() => {
                let at = name.parse().expect("checked index");
                let ty = tuple.at(at);
                if at < tuple.elements.iter().position(|element| element.rest).unwrap_or(tuple.len()) {
                    Resolution::property::<METADATA>(name, ty)
                } else { Resolution::typed(ty) }
            }
            Ty::Named(symbols, args) if args.len() == 1 && name.parse::<usize>().is_ok() && self.is_builtin_symbol(symbols, "ReadonlyArray") => Resolution::typed(args[0].clone()),
            Ty::Named(sym, args) => {
                // Enum members are the enum type.
                if self.is_enum(sym) {
                    return self.member_lookup::<METADATA, PROTOTYPE>(&Ty::Prim("number"), name, receiver);
                }
                self.instance_member::<METADATA, PROTOTYPE>(sym, args, name, receiver)
            }
            Ty::Union(items) => {
                let found: Vec<Resolution> = items
                    .iter()
                    .filter(|i| !matches!(i, Ty::Undefined | Ty::Null))
                    .map(|i| self.member_lookup::<METADATA, PROTOTYPE>(i, name, i))
                    .collect();
                let declared = !found.is_empty() && found.iter().all(|result| result.declared);
                let opaque = found.iter().any(|result| result.ty.is_opaque());
                let mut declarations = Vec::new();
                let mut from_index = false;
                let mut signature_keys = Vec::new();
                let types = found.into_iter().map(|result| {
                    from_index |= result.from_index;
                    if METADATA { signature_keys.extend(result.signature_keys); }
                    if METADATA && declared { declarations.extend(result.declarations); }
                    result.ty
                }).collect();
                Resolution { ty: if opaque { Ty::Unknown } else { Ty::union(types) }, declared, from_index, declarations, signature_keys }
            }
            // Every constituent that has it contributes (`T & { on: any }`
            // makes `on` any).
            Ty::Inter(items) => {
                let mut declarations = Vec::new();
                let mut declared = false;
                let mut from_index = false;
                let mut signature_keys = Vec::new();
                let found: Vec<Ty> = items.iter().map(|i| {
                    let result = self.member_lookup::<METADATA, PROTOTYPE>(i, name, receiver);
                    declared |= result.declared;
                    from_index |= result.from_index;
                    if METADATA {
                        declarations.extend(result.declarations);
                        signature_keys.extend(result.signature_keys);
                    }
                    result.ty
                }).filter(|t| !t.is_opaque()).collect();
                let ty = match found.len() {
                    0 => Ty::Unknown,
                    1 => found.into_iter().next().expect("one"),
                    _ if found.iter().any(|t| matches!(t, Ty::Any)) => Ty::Any,
                    _ => Ty::Inter(found),
                };
                Resolution { ty, declared, from_index, declarations, signature_keys }
            }
            Ty::Obj(members, env) => {
                let env = Env { this: Some(Box::new(receiver.clone())), ..(**env).clone() };
                self.member_from::<METADATA>(members.iter(), name, &env, None).unwrap_or_else(|| self.object_member::<METADATA, PROTOTYPE>(name, receiver))
            }
            Ty::Struct(props) => match props.iter().find(|(n, _)| &**n == name) {
                Some((_, t)) => Resolution::property::<METADATA>(name, t.clone()),
                None => self.object_member::<METADATA, PROTOTYPE>(name, receiver),
            },
            Ty::Class(sym) => {
                if name == "prototype" {
                    return Resolution::property::<METADATA>(name, Ty::Named(sym.clone(), Vec::new()));
                }
                self.static_member::<METADATA>(sym, name).unwrap_or_else(|| self.member_inner::<METADATA>(&self.global_named("Function", vec![]), name, receiver))
            }
            Ty::Ns(sym) => {
                if self.is_enum(sym) {
                    let enum_sym: Sym = sym.iter().copied().filter(|&r| self.kind_of(r) == Some(DeclKind::Enum)).collect();
                    return Resolution::typed(Ty::Named(enum_sym, Vec::new()));
                }
                let found = self.namespace_member(sym, name);
                if found.is_empty() { Resolution::typed(self.value_of(&found)) } else { Resolution::property::<METADATA>(name, self.value_of(&found)) }
            }
            Ty::Module(m) => {
                let found = self.module_member(m.0, name);
                if found.is_empty() { Resolution::typed(self.value_of(&found)) } else { Resolution::property::<METADATA>(name, self.value_of(&found)) }
            }
            Ty::Prim(_) | Ty::Lit(..) | Ty::Array(_) | Ty::Tuple(_) | Ty::Fn(_) => {
                let apparent = self.apparent(t);
                if apparent.is_opaque() {
                    return Resolution::typed(Ty::Unknown);
                }
                self.member_lookup::<METADATA, PROTOTYPE>(&apparent, name, receiver)
            }
            _ => Resolution::typed(Ty::Unknown),
        }
    }

    fn static_member<const METADATA: bool>(&self, sym: &[DeclRef], name: &str) -> Option<Resolution> {
        let d = self.depth.get();
        if d >= MAX_DEPTH {
            return None;
        }
        self.depth.set(d + 1);
        let found = self.static_member_core::<METADATA>(sym, name);
        self.depth.set(d);
        found
    }

    fn static_member_core<const METADATA: bool>(&self, sym: &[DeclRef], name: &str) -> Option<Resolution> {
        for &r in sym {
            let s = self.summary_of(r.file)?;
            let decl = &s.decls[r.decl as usize];
            let env = self.decl_env(r);
            if let Some(t) = self.member_from::<METADATA>(decl.members.iter().filter(|m| m.is_static), name, &env, Some(r)) {
                return Some(t);
            }
            // Statics are inherited too.
            if let Some(TypeSyn::Ref(path, _)) = decl.extends.first().filter(|_| decl.kind == DeclKind::Class) {
                if let Ty::Class(base) = self.value_path(path, &env) {
                    if base.iter().any(|b| sym.contains(b)) {
                        continue;
                    }
                    if let Some(t) = self.static_member::<METADATA>(&base, name) {
                        return Some(t);
                    }
                }
            }
        }
        // A class merged with a namespace.
        let ns = self.namespace_member(sym, name);
        (!ns.is_empty()).then(|| Resolution::property::<METADATA>(name, self.value_of(&ns)))
    }

    fn instance_member<const METADATA: bool, const PROTOTYPE: bool>(&self, sym: &[DeclRef], args: &[Ty], name: &str, receiver: &Ty) -> Resolution {
        let mut sigs: Vec<Arc<Sig>> = Vec::new();
        let mut sig_env: Option<Env> = None;
        let mut declarations = Vec::new();
        for &r in sym {
            let Some(s) = self.summary_of(r.file) else { continue };
            let decl = &s.decls[r.decl as usize];
            let mut env = self.decl_env(r);
            env.tparams = self.bind_tparams(&decl.type_params, args.to_vec(), &env);
            env.this = Some(Box::new(receiver.clone()));
            for m in decl.members.iter().filter(|m| !m.is_static && &*m.name == name) {
                if METADATA && matches!(m.kind, MemberKind::Method | MemberKind::Prop) {
                    declarations.push(MemberDeclaration { owner: Some(r), member: m.clone(), environment: Some(env.clone()) });
                }
                match m.kind {
                    MemberKind::Method => {
                        sigs.extend(m.sigs.iter().cloned());
                        sig_env.get_or_insert_with(|| env.clone());
                    }
                    MemberKind::Prop => return Resolution { ty: self.prop_type(m, &env), from_index: false, declared: true, declarations, signature_keys: Vec::new() },
                    _ => {}
                }
            }
        }
        if let Some(env) = sig_env {
            return Resolution { ty: Ty::Fn(Arc::new(Callable { sigs, env: Arc::new(env), ret: None })), from_index: false, declared: true, declarations, signature_keys: Vec::new() };
        }
        // Inherited.
        let mut unresolved_base = false;
        for &r in sym {
            let Some(s) = self.summary_of(r.file) else { continue };
            let decl = &s.decls[r.decl as usize];
            let mut env = self.decl_env(r);
            env.tparams = self.bind_tparams(&decl.type_params, args.to_vec(), &env);
            for base in self.bases(decl) {
                let base_ty = self.eval(base, &env);
                if base_ty.is_opaque() {
                    unresolved_base = true;
                    continue;
                }
                let t = self.member_lookup::<METADATA, false>(&base_ty, name, receiver);
                if t.declared || !t.ty.is_opaque() {
                    return t;
                }
            }
        }
        if unresolved_base { return Resolution::typed(Ty::Unknown) }
        let object = self.global_named("Object", Vec::new());
        if matches!(&object, Ty::Named(declarations, _) if sym.iter().any(|decl| declarations.contains(decl))) {
            Resolution::typed(Ty::Unknown)
        } else { self.object_member::<METADATA, PROTOTYPE>(name, receiver) }
    }

    fn object_member<const METADATA: bool, const PROTOTYPE: bool>(&self, name: &str, receiver: &Ty) -> Resolution {
        if PROTOTYPE { self.member_inner::<METADATA>(&self.global_named("Object", Vec::new()), name, receiver) }
        else { Resolution::typed(Ty::Unknown) }
    }

    fn member_from<'m, const METADATA: bool>(&self, members: impl Iterator<Item = &'m Member>, name: &str, env: &Env, owner: Option<DeclRef>) -> Option<Resolution> {
        let mut sigs = Vec::new();
        let mut declarations = Vec::new();
        for m in members.filter(|m| &*m.name == name) {
            if METADATA && matches!(m.kind, MemberKind::Method | MemberKind::Prop) {
                declarations.push(MemberDeclaration { owner, member: m.clone(), environment: Some(env.clone()) });
            }
            match m.kind {
                MemberKind::Method => sigs.extend(m.sigs.iter().cloned()),
                MemberKind::Prop => return Some(Resolution { ty: self.prop_type(m, env), from_index: false, declared: true, declarations, signature_keys: Vec::new() }),
                _ => {}
            }
        }
        (!sigs.is_empty()).then(|| Resolution {
            ty: Ty::Fn(Arc::new(Callable { sigs, env: Arc::new(env.clone()), ret: None })),
            from_index: false, declared: true, declarations,
            signature_keys: Vec::new(),
        })
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
        if let Some(source) = self.readonly_source(t) {
            self.member_names(source, out);
        } else if let Some(filter) = self.property_filter(t) {
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
            Ty::Obj(members, _) => out.extend(members.iter().filter(|m| m.kind != MemberKind::Index).map(|m| m.name.clone())),
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
            for m in decl.members.iter().filter(|m| !m.is_static && m.kind != MemberKind::Index) {
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
