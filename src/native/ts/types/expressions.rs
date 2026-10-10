//! Expression value domains shared by file checks and imported initializers.

use super::{Checker, Env, Member, Ty};
use crate::native::ts::summary::Init;

impl Checker<'_> {
    pub(crate) fn import_type(&self, specifier: &str, env: &Env) -> Ty {
        let Some(module) = self.project.resolve(env.file, specifier) else { return Ty::Unknown };
        let namespace = Ty::Module(super::ModRefKey(module));
        // Ambient CommonJS declarations may expose their namespace as a
        // synthetic default. TypeScript 6 enables this unless explicitly off.
        let namespace = if matches!(module, crate::native::ts::modules::ModRef::Ambient(..))
            && self.project.compiler_flag(env.file, "allowSyntheticDefaultImports").unwrap_or(true)
            && self.project.export(module, "default").is_empty()
            && self.project.export(module, "__esModule").is_empty() {
                Ty::Inter(vec![namespace.clone(), Ty::Struct(vec![super::StructProperty {
                    name: "default".into(), ty: namespace, origin: None,
                }].into())])
            } else { namespace };
        self.global_named("Promise", vec![namespace])
    }

    pub(crate) fn binary_type(&self, op: Option<&str>, left: impl FnOnce() -> Ty, right: impl FnOnce() -> Ty) -> Ty {
        match op {
            Some("==" | "!=" | "===" | "!==" | "<" | ">" | "<=" | ">=" | "instanceof" | "in") => Ty::Prim("boolean"),
            Some("+") => {
                let (a, b) = (left().widen(), right().widen());
                let belongs = |ty: &Ty, kind| !matches!(ty, Ty::Any | Ty::Never) && self.assignable(ty, &Ty::Prim(kind)) == Some(true);
                if belongs(&a, "string") || belongs(&b, "string") {
                    Ty::Prim("string")
                } else if belongs(&a, "number") && belongs(&b, "number") {
                    Ty::Prim("number")
                } else if belongs(&a, "bigint") && belongs(&b, "bigint") {
                    Ty::Prim("bigint")
                } else {
                    Ty::Unknown
                }
            }
            Some(op) => if op != ">>>" && matches!(left().widen(), Ty::Prim("bigint")) && matches!(right().widen(), Ty::Prim("bigint")) { Ty::Prim("bigint") } else { Ty::Prim("number") },
            None => Ty::Unknown,
        }
    }

    pub(super) fn init_type(&self, init: &Init, env: &Env) -> Ty {
        match init {
            Init::Class(start) => self.class_value_at(*start, env.file),
            Init::Instantiate(value, args) => self.instantiate(&self.init_type(value, env), args.iter().map(|argument| self.eval(argument, env)).collect()),
            Init::New(path, invocation) => {
                let targs = invocation.type_args.iter().map(|t| self.eval(t, env)).collect::<Vec<_>>();
                let class = self.value_path(path, env);
                let args = invocation.args.iter().map(|value| self.init_type(value, env)).collect::<Vec<_>>();
                self.construct(&class, &args, targs)
            }
            Init::Call(callee, invocation) => {
                let targs = invocation.type_args.iter().map(|t| self.eval(t, env)).collect::<Vec<_>>();
                let f = self.init_type(callee, env);
                let args = if self.needs_args(&f) { invocation.args.iter().map(|value| self.init_type(value, env)).collect() }
                    else { vec![Ty::Unknown; invocation.args.len()] };
                self.call(&f, &args, targs)
            }
            Init::Binary(op, left, right) => self.binary_type(Some(op), || self.init_type(left, env), || self.init_type(right, env)),
            Init::Member(object, name) => {
                let o = self.init_type(object, env);
                self.member(&o, name)
            }
            Init::Await(inner) => {
                let t = self.init_type(inner, env);
                self.awaited(&t)
            }
            Init::Import(specifier) => self.import_type(specifier, env),
            Init::Path(path) if path.first().is_some_and(|p| &**p == "this") => {
                let t = env.this.as_deref().cloned().unwrap_or(Ty::Unknown);
                path[1..].iter().fold(t, |t, seg| self.member(&t, seg))
            }
            Init::Path(path) => self.value_path(path, env),
            Init::Type(t) => self.eval(t, env),
            Init::Literal(t) => self.eval(t, env),
            Init::Other => Ty::Unknown,
        }
    }

    pub(super) fn prop_type(&self, m: &Member, env: &Env) -> Ty {
        let t = match (&m.ty, &m.init) {
            (Some(t), _) => self.eval(t, env),
            (None, Some(i)) => {
                let value = self.init_type(i, env);
                if m.readonly != Some(true) && matches!(value, Ty::Lit(..) | Ty::EnumMember(_))
                    && matches!(i, Init::Path(path) if self.fresh_literal_binding(path, env)) {
                    value.widen()
                } else { value }
            }
            (None, None) => Ty::Unknown,
        };
        if m.optional { t.or_undefined() } else { t }
    }

    pub(crate) fn fresh_literal_member(&self, member: &Member, env: &Env) -> bool {
        member.ty.is_none() && member.init.as_ref().is_some_and(|init| self.fresh_literal_init(init, env, super::MAX_DEPTH))
    }

    fn fresh_literal_init(&self, init: &Init, env: &Env, remaining: u32) -> bool {
        match init {
            Init::Literal(_) => true,
            Init::Path(path) if remaining > 0 => self.fresh_literal_path(path, env, remaining - 1),
            _ => false,
        }
    }

    pub(crate) fn fresh_literal_binding(&self, path: &[Box<str>], env: &Env) -> bool {
        self.fresh_literal_path(path, env, super::MAX_DEPTH)
    }

    fn fresh_literal_path(&self, path: &[Box<str>], env: &Env, remaining: u32) -> bool {
        let mut path = path.to_vec();
        let mut env = env.clone();
        for depth in 0..remaining {
            let Some((head, tail)) = path.split_first() else { return false };
            let mut refs = self.follow(self.lookup(&env, head));
            let mut object = if tail.is_empty() {
                Ty::Unknown
            } else if &**head == "this" {
                env.this.as_deref().cloned().unwrap_or(Ty::Unknown)
            } else {
                self.value_of(&refs)
            };
            for (index, name) in tail.iter().enumerate() {
                refs = self.follow(match object {
                    Ty::Ns(ref symbols) if self.is_enum(symbols) && index + 1 == tail.len() =>
                        return matches!(self.enum_member_type(symbols, name), Ty::EnumMember(_)),
                    Ty::Ns(symbols) => self.namespace_member(&symbols, name),
                    Ty::Module(module) => self.module_member(module.0, name),
                    _ if index + 1 == tail.len() => {
                        let members = self.member_resolution(&object, name).declarations;
                        return !members.is_empty() && members.iter().all(|declaration| declaration.member.ty.is_none()
                            && declaration.environment.as_ref().is_some_and(|env| declaration.member.init.as_ref()
                                .is_some_and(|init| self.fresh_literal_init(init, env, remaining - depth - 1))));
                    }
                    _ => {
                        object = self.member(&object, name);
                        continue;
                    }
                });
                object = self.value_of(&refs);
            }
            let Some(&binding) = refs.first() else { return false };
            let Some(summary) = self.summary_of(binding.file) else { return false };
            let declaration = &summary.decls[binding.decl as usize];
            if declaration.ty.is_some() || declaration.kind != crate::native::ts::summary::DeclKind::Var { return false }
            match &declaration.init {
                Some(Init::Literal(_)) => return true,
                Some(Init::Path(next)) => {
                    path = next.to_vec();
                    env = self.decl_env(binding);
                }
                _ => return false,
            }
        }
        false
    }
}
