//! A bounded assignability relation. It answers only what declared types
//! can prove: `Some(true)`, `Some(false)`, or `None` when TypeScript's
//! structural rules (or a type zlint cannot read) would decide.

use super::super::modules::DeclRef;
use super::super::summary::{DeclKind, TypeSyn};
use super::{Checker, Tuple, Ty, MAX_DEPTH};

/// Nested structural comparisons allowed (`{ a: { b: X } }` is two).
const MAX_STRUCTURAL: u32 = 2;
/// Larger property bags are not compared structurally.
const MAX_PROPS: usize = 24;
/// Intersections can branch exponentially even while nesting stays shallow.
const MAX_RELATION_WORK: usize = 1024;

#[cfg(test)]
thread_local! {
    static RELATION_WORK: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

impl Ty {
    /// Whether any can flow through a position inspected by `unsafe_assignment_to`.
    pub fn carries_transfer_any(&self) -> bool {
        fn check(sender: &Ty, depth: usize) -> bool {
            if depth >= 32 { return false }
            match sender {
                Ty::Any => true,
                Ty::Named(_, args) => args.iter().any(|arg| check(arg, depth + 1)),
                Ty::Array(array) => check(&array.element, depth + 1),
                Ty::Tuple(tuple) => tuple.iter().any(|element| check(element, depth + 1)),
                _ => false,
            }
        }
        check(self, 0)
    }

    /// Any flowing into a known non-any generic position of the same type.
    pub fn unsafe_assignment_to(&self, receiver: &Ty) -> bool {
        fn check(sender: &Ty, receiver: &Ty, depth: usize) -> bool {
            if depth >= 32 { return false }
            match (sender, receiver) {
                (Ty::Any, Ty::Any | Ty::Unknown | Ty::IntrinsicUnknown) => false,
                (Ty::Any, _) => true,
                (Ty::Named(a, aa), Ty::Named(b, ba)) if a == b && aa.len() == ba.len() =>
                    aa.iter().zip(ba).any(|(a, b)| check(a, b, depth + 1)),
                (Ty::Array(a), Ty::Array(b)) if a.readonly == b.readonly => check(&a.element, &b.element, depth + 1),
                (Ty::Tuple(a), Ty::Tuple(b)) if a.readonly == b.readonly && a.len() == b.len()
                    && a.elements.iter().zip(&b.elements).all(|(a, b)| a.optional == b.optional && a.rest == b.rest) =>
                    a.iter().zip(b.iter()).any(|(a, b)| check(a, b, depth + 1)),
                _ => false,
            }
        }
        check(self, receiver, 0)
    }
}

impl Checker<'_> {
    pub fn with_relation_work<T>(&self, f: impl FnOnce() -> T) -> T {
        if self.relation_work.get().is_some() { return f() }
        self.relation_work.set(Some(MAX_RELATION_WORK));
        let result = f();
        self.relation_work.set(None);
        result
    }

    /// Can a value of type `a` be assigned where `b` is expected?
    pub fn assignable(&self, a: &Ty, b: &Ty) -> Option<bool> {
        #[cfg(test)]
        RELATION_WORK.with(|work| work.set(work.get() + 1));
        if a.same_identity(b) { return Some(true) }
        let d = self.depth.get();
        if d >= MAX_DEPTH {
            return None;
        }
        let remaining = self.relation_work.get();
        let root = remaining.is_none();
        let remaining = remaining.unwrap_or(MAX_RELATION_WORK);
        if remaining == 0 { return None }
        self.relation_work.set(Some(remaining - 1));
        self.depth.set(d + 1);
        let r = self.relate(a, b);
        self.depth.set(d);
        if root { self.relation_work.set(None); }
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

    /// Fresh object literals additionally reject known excess properties.
    pub fn assignable_fresh_object(&self, a: &Ty, b: &Ty) -> Option<bool> {
        let props = self.props(a)?;
        let known = if let Some((keys, _)) = self.record_parts(b) {
            all(props.iter().map(|(name, _, _)| super::utilities::accepts_key(keys, name)))?
        } else {
            let target = self.props(b)?;
            props.iter().all(|(name, _, _)| target.iter().any(|(key, _, _)| name == key))
        };
        if !known { return Some(false) }
        self.assignable(a, b)
    }

    fn relate(&self, a: &Ty, b: &Ty) -> Option<bool> {
        if !matches!(a, Ty::Any | Ty::Never | Ty::Unknown) {
            if let Some((keys, value)) = self.record_parts(b) {
                return self.relate_record(a, keys, value);
            }
        }
        match (a, b) {
            (Ty::Unknown, _) | (_, Ty::Unknown) => None,
            (Ty::Any | Ty::Never, _) | (_, Ty::Any) => Some(true),
            (_, Ty::IntrinsicUnknown) => Some(true),
            (Ty::IntrinsicUnknown, Ty::Param(_)) => None,
            (Ty::IntrinsicUnknown, _) => Some(false),
            (Ty::Param(x), Ty::Param(y)) if x == y => Some(true),
            // A source union needs every member; a target union one.
            (Ty::Union(items), _) => all(items.iter().map(|i| self.assignable(i, b))),
            (_, Ty::Union(items)) => any(items.iter().map(|i| self.assignable(a, i))),
            (_, Ty::Inter(items)) => all(items.iter().map(|i| self.assignable(a, i))),
            (Ty::Inter(items), _) => match any(items.iter().map(|i| self.assignable(i, b))) {
                Some(true) => Some(true),
                _ => None,
            },
            (Ty::Param(_), _) | (_, Ty::Param(_)) => None,
            (_, Ty::Never) => Some(false),
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
            (Ty::Array(x), Ty::Array(y)) if x.readonly && !y.readonly => Some(false),
            (Ty::Array(x), Ty::Array(y)) => self.assignable(&x.element, &y.element),
            (Ty::Tuple(items), Ty::Array(y)) if items.readonly && !y.readonly => Some(false),
            (Ty::Tuple(items), Ty::Array(y)) => all((0..items.len()).map(|index| self.assignable(&items.at(index), &y.element))),
            (Ty::Tuple(xs), Ty::Tuple(ys)) => self.relate_tuples(xs, ys),
            (Ty::Named(sym, args), Ty::Array(target)) if args.len() == 1 && self.is_builtin_symbol(sym, "ReadonlyArray") => {
                if target.readonly { self.assignable(&args[0], &target.element) } else { Some(false) }
            },
            (Ty::Array(_), Ty::Tuple(_)) => Some(false),
            (Ty::Array(e), Ty::Named(sym, args)) if self.is_named(sym, &["Array", "ReadonlyArray", "Iterable"]) && args.len() == 1 => {
                self.assignable(&e.element, &args[0])
            }
            (Ty::Fn(_) | Ty::Class(_), Ty::Named(sym, _)) if self.is_named(sym, &["Function"]) => Some(true),
            (Ty::Named(sym, _), Ty::Fn(_)) if self.is_builtin_symbol(sym, "Function") && !self.callable_type(a) => Some(false),
            (Ty::Fn(x), Ty::Fn(y)) => self.relate_fns(x, y),
            (Ty::Named(x, xargs), Ty::Named(y, yargs)) => self.relate_named(x, xargs, y, yargs).or_else(|| self.structural(a, b)),
            _ => self.structural(a, b),
        }
    }

    fn relate_tuples(&self, source: &Tuple, target: &Tuple) -> Option<bool> {
        if source.readonly && !target.readonly { return Some(false) }
        let bounds = |tuple: &Tuple| {
            if tuple.elements.iter().enumerate().any(|(index, element)| element.rest && index + 1 != tuple.len()) { return None }
            let min = tuple.required_length();
            let max = (!tuple.elements.iter().any(|element| element.rest)).then_some(tuple.len());
            Some((min, max))
        };
        let (source_min, source_max) = bounds(source)?;
        let (target_min, target_max) = bounds(target)?;
        if source_min < target_min || target_max.is_some_and(|max| source_max.is_none_or(|length| length > max)) { return Some(false) }
        all((0..source.len().max(target.len())).filter(|&index| source_max.is_none_or(|length| index < length))
            .map(|index| self.assignable(&source.at(index), &target.at(index))))
    }

    fn relate_record(&self, a: &Ty, keys: &Ty, value: &Ty) -> Option<bool> {
        use super::utilities::{accepts_key, keys_overlap, required_keys};
        let required = required_keys(keys)?;
        if let Some((source_keys, source_value)) = self.record_parts(a) {
            if !all(required.iter().map(|key| accepts_key(source_keys, key)))? {
                return Some(false);
            }
            return match keys_overlap(source_keys, keys) {
                Some(true) => self.assignable(source_value, value),
                Some(false) => Some(true),
                None => None,
            };
        }
        if let Ty::Union(parts) = a {
            return all(parts.iter().map(|part| self.relate_record(part, keys, value)));
        }
        let props = self.props(a)?;
        let required_present = required.iter().all(|key| props.iter().any(|(name, _, optional)| &**name == key && !optional));
        if !required_present { return Some(false) }
        all(props.iter().map(|(name, ty, _)| match accepts_key(keys, name) {
            Some(true) => self.assignable(ty, value),
            Some(false) => Some(true),
            None => None,
        }))
    }

    /// A function fits a single-signature function type when it needs no
    /// more parameters than it is given; the result only matters when the
    /// target returns something other than `void`.
    fn relate_fns(&self, x: &super::Callable, y: &super::Callable) -> Option<bool> {
        let ([a], [b]) = (&x.sigs[..], &y.sigs[..]) else { return None };
        let (a, b) = (&a.syntax, &b.syntax);
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
        if let Some(filter) = self.property_filter(t) {
            return Some(filter.names(self, 0)?.len());
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
        if let Some(filter) = self.property_filter(t) {
            let names = filter.names(self, 0)?;
            if names.is_empty() { return Some(Vec::new()) }
            if self.depth.get() >= MAX_DEPTH { return None }
            self.depth.set(self.depth.get() + 1);
            let properties = self.props(filter.source);
            self.depth.set(self.depth.get() - 1);
            return properties?.into_iter().try_fold(Vec::new(), |mut selected, property| {
                if filter.allows(&property.0)? { selected.push(property); }
                Some(selected)
            });
        }
        match t {
            Ty::Struct(props) => Some(props.iter().map(|property| (property.name.clone(), property.ty.clone(), false)).collect()),
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
                MemberKind::Method => out.push((m.name.clone(), Ty::Fn(std::sync::Arc::new(super::Callable::new(m.sigs.clone(), std::sync::Arc::new(env.clone()), None))), m.optional)),
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::tests::with_native_file;

    #[test]
    fn transfer_sender_prefilter_keeps_every_comparable_any_position() {
        with_native_file("interface Box<T> { value: T; } declare const value: Box<any>; const copy = /*@*/value;", |file, nodes| {
            let named = file.type_of(nodes[0]);
            let Ty::Named(symbol, _) = named else { panic!("named generic") };
            let mut senders = vec![
                Ty::Any, Ty::Unknown, Ty::IntrinsicUnknown, Ty::Prim("number"),
                Ty::Named(symbol.clone(), vec![Ty::Any]),
                Ty::Named(symbol.clone(), vec![Ty::Unknown]),
                Ty::Union(vec![Ty::Any, Ty::Prim("number")]),
                Ty::Inter(vec![Ty::Any, Ty::Prim("number")]),
            ];
            let mut receivers = vec![Ty::Any, Ty::Unknown, Ty::IntrinsicUnknown, Ty::Prim("number"), Ty::Named(symbol, vec![Ty::Prim("number")])];
            for readonly in [false, true] {
                for element in [Ty::Any, Ty::Prim("number")] {
                    senders.push(Ty::Array(super::super::Array { element: Box::new(element.clone()), readonly }));
                    receivers.push(Ty::Array(super::super::Array { element: Box::new(element), readonly }));
                }
                let mut tuple: Tuple = [Ty::Any, Ty::Prim("number")].into_iter().collect();
                tuple.readonly = readonly;
                senders.push(Ty::Tuple(tuple));
                let mut tuple: Tuple = [Ty::Prim("string"), Ty::Prim("number")].into_iter().collect();
                tuple.readonly = readonly;
                receivers.push(Ty::Tuple(tuple));
            }
            for sender in &senders {
                for receiver in &receivers {
                    assert!(!sender.unsafe_assignment_to(receiver) || sender.carries_transfer_any());
                }
            }
            let mut sender = Ty::Any;
            let mut receiver = Ty::Prim("number");
            for _ in 0..33 {
                assert!(!sender.unsafe_assignment_to(&receiver) || sender.carries_transfer_any());
                sender = Ty::Array(super::super::Array { element: Box::new(sender), readonly: false });
                receiver = Ty::Array(super::super::Array { element: Box::new(receiver), readonly: false });
            }
        });
    }

    #[test]
    fn a_relation_scope_bounds_multiple_comparisons_together() {
        with_native_file("const x = /*@*/1;", |file, _| {
            let checker = &file.typing().checker;
            let source = Ty::Struct(Vec::new().into());
            let mut target = Ty::Unknown;
            for _ in 0..12 { target = Ty::Inter(vec![target.clone(), target]); }
            RELATION_WORK.with(|work| work.set(0));
            checker.with_relation_work(|| {
                for _ in 0..4 { assert_eq!(checker.assignable(&source, &target), None); }
            });
            assert!(RELATION_WORK.with(|work| work.get()) < 2048);
            assert_eq!(checker.assignable(&Ty::Prim("number"), &Ty::Prim("string")), Some(false));
        });
    }

    #[test]
    fn uncertain_intersections_have_a_total_work_bound() {
        with_native_file("const x = /*@*/1;", |file, _| {
            let source = Ty::Struct(Vec::new().into());
            let mut target = Ty::Unknown;
            for _ in 0..12 {
                target = Ty::Inter(vec![target.clone(), target]);
            }
            RELATION_WORK.with(|work| work.set(0));
            assert_eq!(file.typing().checker.assignable(&source, &target), None);
            assert!(RELATION_WORK.with(|work| work.get()) < 2048);
            assert_eq!(file.typing().checker.assignable(&Ty::Prim("number"), &Ty::Prim("string")), Some(false));
        });
    }

    #[test]
    fn repeated_identical_intersections_do_not_redistribute() {
        with_native_file("const x = /*@*/1;", |file, _| {
            let mut ty = Ty::Prim("number");
            for _ in 0..8 {
                ty = Ty::Inter(vec![ty.clone(), Ty::Prim("object")]);
            }
            RELATION_WORK.with(|work| work.set(0));
            assert_eq!(file.typing().checker.assignable(&ty, &ty), Some(true));
            assert!(RELATION_WORK.with(|work| work.get()) <= 2);
            assert_eq!(file.typing().checker.assignable(&Ty::Unknown, &Ty::Unknown), None);
            assert_eq!(file.typing().checker.assignable(&Ty::Prim("string"), &Ty::Prim("number")), Some(false));
        });
    }
}
