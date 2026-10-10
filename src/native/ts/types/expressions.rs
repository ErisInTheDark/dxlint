//! Expression value domains shared by file checks and imported initializers.

use super::{Checker, Env, Member, Ty};
use crate::native::ts::summary::Init;

impl Checker<'_> {
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
                if m.readonly != Some(true) && matches!(value, Ty::Lit(..))
                    && matches!(i, Init::Path(path) if self.fresh_literal_binding(path, env)) {
                    value.widen()
                } else { value }
            }
            (None, None) => Ty::Unknown,
        };
        if m.optional { t.or_undefined() } else { t }
    }

    fn fresh_literal_binding(&self, path: &[Box<str>], env: &Env) -> bool {
        let mut path = path.to_vec();
        let mut env = env.clone();
        for _ in 0..super::MAX_DEPTH {
            let Some((head, tail)) = path.split_first() else { return false };
            let mut refs = self.follow(self.lookup(&env, head));
            for name in tail {
                refs = self.follow(match self.value_of(&refs) {
                    Ty::Ns(symbols) => self.namespace_member(&symbols, name),
                    Ty::Module(module) => self.module_member(module.0, name),
                    _ => return false,
                });
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
