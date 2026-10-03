//! A bounded assignability relation. It answers only what declared types
//! can prove: `Some(true)`, `Some(false)`, or `None` when TypeScript's
//! structural rules (or a type zlint cannot read) would decide.

use super::super::modules::DeclRef;
use super::super::summary::{DeclKind, TypeSyn};
use super::{Checker, Ty, MAX_DEPTH};

/// Nested structural comparisons allowed (`{ a: { b: X } }` is two).
const MAX_STRUCTURAL: u32 = 2;
/// Larger property bags are not compared structurally.
const MAX_PROPS: usize = 24;

impl Checker<'_> {
    /// Can a value of type `a` be assigned where `b` is expected?
    pub fn assignable(&self, a: &Ty, b: &Ty) -> Option<bool> {
        let d = self.depth.get();
        if d >= MAX_DEPTH {
            return None;
        }
        self.depth.set(d + 1);
        let r = self.relate(a, b);
        self.depth.set(d);
        r
    }

    /// Assignable both ways: what typescript-eslint treats as "the same
    /// type" (a numeric enum and `number` are).
    pub fn equivalent(&self, a: &Ty, b: &Ty) -> Option<bool> {
        match (self.assignable(a, b)?, self.assignable(b, a)?) {
            (true, true) => Some(true),
            _ => Some(false),
        }
    }

    fn relate(&self, a: &Ty, b: &Ty) -> Option<bool> {
        match (a, b) {
            (Ty::Unknown, _) | (_, Ty::Unknown) => None,
            (Ty::Any | Ty::Never, _) | (_, Ty::Any) => Some(true),
            (Ty::Param(x), Ty::Param(y)) if x == y => Some(true),
            (Ty::Param(_), _) | (_, Ty::Param(_)) => None,
            // A source union needs every member; a target union one.
            (Ty::Union(items), _) => all(items.iter().map(|i| self.assignable(i, b))),
            (_, Ty::Union(items)) => any(items.iter().map(|i| self.assignable(a, i))),
            (_, Ty::Inter(items)) => all(items.iter().map(|i| self.assignable(a, i))),
            (Ty::Inter(items), _) => match any(items.iter().map(|i| self.assignable(i, b))) {
                Some(true) => Some(true),
                _ => None,
            },
            (Ty::Undefined, Ty::Undefined | Ty::Void) | (Ty::Null, Ty::Null) | (Ty::Void, Ty::Void) => Some(true),
            (Ty::Undefined | Ty::Null | Ty::Void, _) | (_, Ty::Undefined | Ty::Null | Ty::Void) => Some(false),
            (Ty::Named(sym, _), Ty::Prim(p) | Ty::Lit(_, p)) if self.is_enum(sym) => {
                let string = self.is_string_enum(sym);
                Some(matches!(b, Ty::Prim(_)) && (*p == "number" && !string || *p == "string" && string))
            }
            (Ty::Prim(p) | Ty::Lit(_, p), Ty::Named(sym, _)) if self.is_enum(sym) => Some(*p == "number" && !self.is_string_enum(sym)),
            (Ty::Lit(x, p), Ty::Lit(y, q)) => Some(p == q && x == y),
            (Ty::Lit(_, p), Ty::Prim(q)) | (Ty::Prim(p), Ty::Prim(q)) if *q != "object" => Some(p == q),
            (Ty::Prim(_), Ty::Lit(..)) => Some(false),
            (Ty::Prim(_) | Ty::Lit(..), Ty::Prim("object")) => Some(false),
            (Ty::Named(..) | Ty::Obj(..) | Ty::Struct(_) | Ty::Array(_) | Ty::Tuple(_) | Ty::Fn(_) | Ty::Class(_), Ty::Prim("object")) => Some(true),
            (Ty::Named(..) | Ty::Array(_) | Ty::Tuple(_) | Ty::Fn(_) | Ty::Obj(..) | Ty::Struct(_), Ty::Prim(_) | Ty::Lit(..)) => Some(false),
            (Ty::Array(x), Ty::Array(y)) => self.assignable(x, y),
            (Ty::Tuple(items), Ty::Array(y)) => all(items.iter().map(|i| self.assignable(i, y))),
            (Ty::Tuple(xs), Ty::Tuple(ys)) if xs.len() == ys.len() => all(xs.iter().zip(ys).map(|(x, y)| self.assignable(x, y))),
            (Ty::Array(_), Ty::Tuple(_)) => Some(false),
            (Ty::Array(e), Ty::Named(sym, args)) if self.is_named(sym, &["Array", "ReadonlyArray", "Iterable"]) && args.len() == 1 => {
                self.assignable(e, &args[0])
            }
            (Ty::Fn(_) | Ty::Class(_), Ty::Named(sym, _)) if self.is_named(sym, &["Function"]) => Some(true),
            (Ty::Fn(x), Ty::Fn(y)) => self.relate_fns(x, y),
            (Ty::Named(x, xargs), Ty::Named(y, yargs)) => self.relate_named(x, xargs, y, yargs).or_else(|| self.structural(a, b)),
            _ => self.structural(a, b),
        }
    }

    /// A function fits a single-signature function type when it needs no
    /// more parameters than it is given; the result only matters when the
    /// target returns something other than `void`.
    fn relate_fns(&self, x: &super::Callable, y: &super::Callable) -> Option<bool> {
        let ([a], [b]) = (&x.sigs[..], &y.sigs[..]) else { return None };
        let required = a.params.iter().filter(|p| !p.optional && !p.rest).count();
        if required > b.params.len() && !b.params.iter().any(|p| p.rest) {
            return Some(false);
        }
        matches!(&b.ret, Some(TypeSyn::Kw(k)) if &**k == "void").then_some(true)
    }

    /// Property by property, when both sides are plain property bags
    /// (object literals, object types, interfaces without call, construct
    /// or index signatures).
    fn structural(&self, a: &Ty, b: &Ty) -> Option<bool> {
        // Property types nest: past a couple of levels the answer is not
        // worth what it costs (TypeScript caches; zlint gives up).
        let d = self.structural_depth.get();
        if d >= MAX_STRUCTURAL {
            return None;
        }
        // Big interfaces (UI components with hundreds of members) are never
        // worth evaluating every member of: counted before anything is typed.
        if self.prop_count(a, 0)? > MAX_PROPS || self.prop_count(b, 0)? > MAX_PROPS {
            return None;
        }
        let (ap, bp) = (self.props(a)?, self.props(b)?);
        self.structural_depth.set(d + 1);
        let r = all(bp.iter().map(|(name, bt, optional)| match ap.iter().find(|(n, ..)| n == name) {
            Some((_, at, _)) => self.assignable(at, bt),
            None => Some(*optional),
        }));
        self.structural_depth.set(d);
        r
    }

    /// How many members `props` would evaluate (without typing any), or
    /// `None` when it would refuse; stops counting past the limit.
    fn prop_count(&self, t: &Ty, level: u32) -> Option<usize> {
        if level > 8 {
            return None;
        }
        match t {
            Ty::Struct(props) => Some(props.len()),
            Ty::Obj(members, _) => Some(members.len()),
            Ty::Inter(items) => items.iter().try_fold(0, |n, i| Some(n + self.prop_count(i, level + 1)?)),
            Ty::Named(sym, args) => {
                let mut n = 0;
                for &r in sym.iter() {
                    let s = self.summary_of(r.file)?;
                    let decl = &s.decls[r.decl as usize];
                    if !matches!(decl.kind, DeclKind::Interface | DeclKind::Class) {
                        return None;
                    }
                    n += decl.members.len();
                    if n > MAX_PROPS {
                        return Some(n);
                    }
                    if !decl.extends.is_empty() {
                        let mut env = self.decl_env(r);
                        env.tparams = self.bind_tparams(&decl.type_params, args.clone(), &env);
                        for base in self.bases(decl) {
                            n += self.prop_count(&self.eval(base, &env), level + 1)?;
                            if n > MAX_PROPS {
                                return Some(n);
                            }
                        }
                    }
                }
                Some(n)
            }
            _ => None,
        }
    }

    /// `(name, type, optional)` for every property of a plain object type.
    fn props(&self, t: &Ty) -> Option<Vec<(Box<str>, Ty, bool)>> {
        match t {
            Ty::Struct(props) => Some(props.iter().map(|(n, t)| (n.clone(), t.clone(), false)).collect()),
            Ty::Obj(members, env) => self.member_props(members.iter(), env),
            Ty::Inter(items) => {
                let mut out = Vec::new();
                for i in items {
                    out.extend(self.props(i)?);
                }
                Some(out)
            }
            Ty::Named(sym, args) => {
                if self.depth.get() >= MAX_DEPTH {
                    return None;
                }
                let mut out = Vec::new();
                for &r in sym.iter() {
                    let s = self.summary_of(r.file)?;
                    let decl = &s.decls[r.decl as usize];
                    if !matches!(decl.kind, DeclKind::Interface | DeclKind::Class) {
                        return None;
                    }
                    let mut env = self.decl_env(r);
                    env.tparams = self.bind_tparams(&decl.type_params, args.clone(), &env);
                    out.extend(self.member_props(decl.members.iter(), &env)?);
                    for base in self.bases(decl) {
                        self.depth.set(self.depth.get() + 1);
                        let inherited = self.props(&self.eval(base, &env));
                        self.depth.set(self.depth.get() - 1);
                        out.extend(inherited?);
                    }
                }
                Some(out)
            }
            _ => None,
        }
    }

    fn member_props<'m>(&self, members: impl Iterator<Item = &'m super::super::summary::Member>, env: &super::Env) -> Option<Vec<(Box<str>, Ty, bool)>> {
        use super::super::summary::MemberKind;
        let mut out = Vec::new();
        for m in members.filter(|m| !m.is_static) {
            match m.kind {
                MemberKind::Prop => out.push((m.name.clone(), self.prop_type(m, env), m.optional)),
                MemberKind::Method => out.push((m.name.clone(), Ty::Fn(std::sync::Arc::new(super::Callable { sigs: m.sigs.clone(), env: std::sync::Arc::new(env.clone()), ret: None })), m.optional)),
                _ => return None,
            }
        }
        Some(out)
    }

    /// The same declaration relates its arguments; otherwise heritage can
    /// prove it. Anything else is structural: unknown.
    fn relate_named(&self, x: &[DeclRef], xargs: &[Ty], y: &[DeclRef], yargs: &[Ty]) -> Option<bool> {
        if x.first() == y.first() {
            if self.is_enum(x) || xargs.is_empty() && yargs.is_empty() {
                return Some(true);
            }
            if xargs.len() != yargs.len() {
                return None;
            }
            return all(xargs.iter().zip(yargs).map(|(a, b)| self.assignable(a, b)));
        }
        if self.is_enum(x) || self.is_enum(y) {
            return Some(false);
        }
        for &r in x {
            let s = self.summary_of(r.file)?;
            let decl = &s.decls[r.decl as usize];
            let mut env = self.decl_env(r);
            env.tparams = self.bind_tparams(&decl.type_params, xargs.to_vec(), &env);
            for base in self.bases(decl) {
                if self.assignable(&self.eval(base, &env), &Ty::Named(y.into(), yargs.to_vec())) == Some(true) {
                    return Some(true);
                }
            }
        }
        None
    }

    fn is_named(&self, sym: &[DeclRef], names: &[&str]) -> bool {
        self.decl_name(sym).is_some_and(|n| names.contains(&&*n))
    }

    /// An enum with string members.
    pub fn is_string_enum(&self, sym: &[DeclRef]) -> bool {
        sym.iter().any(|&r| {
            self.summary_of(r.file).is_some_and(|s| {
                let decl = &s.decls[r.decl as usize];
                decl.kind == DeclKind::Enum && decl.members.iter().any(|m| matches!(&m.ty, Some(TypeSyn::Lit(_))))
            })
        })
    }
}

/// Every answer true: true; any false: false; else unknown.
fn all(answers: impl Iterator<Item = Option<bool>>) -> Option<bool> {
    let mut known = true;
    for a in answers {
        match a {
            Some(false) => return Some(false),
            None => known = false,
            Some(true) => {}
        }
    }
    known.then_some(true)
}

/// Any answer true: true; every one false: false; else unknown.
fn any(answers: impl Iterator<Item = Option<bool>>) -> Option<bool> {
    let mut known = true;
    for a in answers {
        match a {
            Some(true) => return Some(true),
            None => known = false,
            Some(false) => {}
        }
    }
    known.then_some(false)
}
