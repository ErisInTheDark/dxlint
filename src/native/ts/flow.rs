//! Control flow, as far as rules need it, asked lazily per question:
//!
//! - narrowing: a reference's declared type refined by the conditions that
//!   guard it in the same function (`if (x)`, `x && x.y`, `typeof`,
//!   `=== null`, `instanceof`, `Array.isArray`, early `return`/`throw`).
//!   Any other test leaves the type alone; an assignment to the reference
//!   between a guard and the use cancels the guards above it.
//! - completion: whether a statement always leaves its block (code paths
//!   proper are in `paths`).

use super::facts::NativeFile;
use super::kinds::{Field as F, Kind as K};
use super::types::{Checker, Ty};

/// A condition known to hold (or not, `sense == false`) at a use.
struct Guard {
    test: u32,
    sense: bool,
}

impl NativeFile<'_> {
    /// `t`, the declared type of reference `id`, narrowed at `id`.
    pub(super) fn narrow(&self, id: u32, t: Ty) -> Ty {
        if self.is_write_target(id) {
            return t;
        }
        let Some(key) = self.ref_key(id) else { return t };
        let guards = self.guards(id, &key);
        guards.iter().rev().fold(t, |t, g| self.apply(g.test, g.sense, t, &key))
    }

    /// The text a reference is narrowed under: `x`, `this.a.b`.
    pub(super) fn ref_key(&self, id: u32) -> Option<String> {
        let s = &self.syntax;
        match s.kind(id)? {
            K::Ident => Some(s.text(id).to_string()),
            K::This => Some("this".into()),
            K::Paren | K::NonNull => self.ref_key(s.get(id, F::Expr)?),
            K::Member if s.get(id, F::Optional).is_none() || !self.optional_chain_can_short_circuit(id) => {
                let object = self.ref_key(s.get(id, F::Object)?)?;
                Some(format!("{object}.{}", s.text(s.get(id, F::Property)?)))
            }
            _ => None,
        }
    }

    /// The left side of an assignment, or an update's argument.
    fn is_write_target(&self, id: u32) -> bool {
        let s = &self.syntax;
        let Some(p) = s.parent(id) else { return false };
        (s.is(p, K::Assign) && s.field(id) == Some(F::Left)) || s.is(p, K::Update)
    }

    /// Guards on the way up from `id` to its function, innermost first.
    ///
    /// An assignment to the reference cancels every guard tested before it.
    /// Statements are only searched for assignments where a guard above
    /// would depend on it (most references have none, and scanning every
    /// earlier statement of a long block for each would be quadratic).
    fn guards(&self, id: u32, key: &str) -> Vec<Guard> {
        let s = &self.syntax;
        // Each guard, and the statement ranges (`block`, `from..to` child
        // indices) an assignment in would cancel it and every guard above.
        enum Step {
            Guard(Guard),
            Region(u32, usize, usize),
            /// The statements of a block before its child (found lazily).
            Before(u32, u32),
        }
        let mut steps = Vec::new();
        let mut child = id;
        while let Some(node) = s.parent(child) {
            let field = s.field(child);
            match s.kind(node) {
                Some(K::Function | K::Arrow | K::Method | K::StaticBlock | K::ClassProp) | None => break,
                Some(K::If | K::Conditional) => match (field, s.get(node, F::Test)) {
                    (Some(F::Consequent), Some(test)) => steps.push(Step::Guard(Guard { test, sense: true })),
                    (Some(F::Alternate), Some(test)) => steps.push(Step::Guard(Guard { test, sense: false })),
                    _ => {}
                },
                Some(K::Logical) if field == Some(F::Right) => {
                    if let Some(test) = s.get(node, F::Left) {
                        match s.word(node, F::Op) {
                            Some("&&") => steps.push(Step::Guard(Guard { test, sense: true })),
                            Some("||") => steps.push(Step::Guard(Guard { test, sense: false })),
                            _ => {}
                        }
                    }
                }
                // A loop body runs again after its own assignments.
                Some(K::While | K::For) if field == Some(F::Body) => {
                    if let Some(test) = s.get(node, F::Test) {
                        steps.push(Step::Region(node, usize::MAX, usize::MAX));
                        steps.push(Step::Guard(Guard { test, sense: true }));
                    }
                }
                Some(K::Block | K::Program | K::Case) if field == Some(F::Body) => {
                    let exits = self.early_exits(node);
                    if exits.is_empty() {
                        steps.push(Step::Before(node, child));
                        child = node;
                        continue;
                    }
                    let at = s.all(node, F::Body).position(|st| st == child).unwrap_or(0);
                    // Early exits before the use, nearest first, each
                    // guarding what follows it.
                    let mut upto = at;
                    for g in exits.into_iter().rev().filter(|&g| g < at) {
                        steps.push(Step::Region(node, g + 1, upto));
                        if let Some(test) = s.all(node, F::Body).nth(g).and_then(|st| s.get(st, F::Test)) {
                            steps.push(Step::Guard(Guard { test, sense: false }));
                        }
                        upto = g + 1;
                    }
                    steps.push(Step::Region(node, 0, upto));
                }
                _ => {}
            }
            child = node;
        }
        let mut out = Vec::new();
        let mut remaining = steps.iter().filter(|st| matches!(st, Step::Guard(_))).count();
        for step in steps {
            match step {
                Step::Guard(g) => {
                    out.push(g);
                    remaining -= 1;
                }
                // Only worth scanning when guards remain above it.
                Step::Region(node, from, to) if remaining > 0 => {
                    let written = if from == usize::MAX {
                        self.writes(node, key)
                    } else {
                        s.all(node, F::Body).skip(from).take(to.saturating_sub(from)).any(|st| self.writes(st, key))
                    };
                    if written {
                        break;
                    }
                }
                Step::Before(node, child) if remaining > 0 => {
                    if s.all(node, F::Body).take_while(|&st| st != child).any(|st| self.writes(st, key)) {
                        break;
                    }
                }
                Step::Region(..) | Step::Before(..) => {}
            }
        }
        out
    }

    /// Indices of a block's `if (...) return/throw/...;` statements without
    /// `else` (memoised per block).
    fn early_exits(&self, block: u32) -> Vec<usize> {
        let typing = self.typing();
        if let Some(v) = typing.exits.borrow().get(&block) {
            return v.clone();
        }
        let s = &self.syntax;
        let found: Vec<usize> = s
            .all(block, F::Body)
            .enumerate()
            .filter(|&(_, st)| s.is(st, K::If) && s.get(st, F::Alternate).is_none() && s.get(st, F::Consequent).is_some_and(|c| self.always_exits(c)))
            .map(|(i, _)| i)
            .collect();
        typing.exits.borrow_mut().insert(block, found.clone());
        found
    }

    /// `node`'s text assigns to `key` (`key = `, `key += `, `key++`).
    fn writes(&self, node: u32, key: &str) -> bool {
        let text = self.syntax.text(node);
        let bytes = text.as_bytes();
        let word = |c: u8| c.is_ascii_alphanumeric() || c == b'_' || c == b'$';
        let mut from = 0;
        while let Some(i) = text[from..].find(key).map(|i| i + from) {
            from = i + key.len();
            if i > 0 && (word(bytes[i - 1]) || bytes[i - 1] == b'.') {
                continue;
            }
            let rest = text[from..].trim_start();
            let assigns = rest.starts_with("++")
                || rest.starts_with("--")
                || (rest.starts_with('=') && !rest.starts_with("==") && !rest.starts_with("=>"))
                || ["+=", "-=", "*=", "/=", "%=", "**=", "&&=", "||=", "??=", "|=", "&=", "^=", "<<=", ">>="].iter().any(|op| rest.starts_with(op));
            if assigns && !rest.starts_with(|c: char| c.is_alphanumeric()) {
                return true;
            }
        }
        false
    }

    /// The statement never completes normally: `return`, `throw`,
    /// `break`, `continue`, or a block / `if` whose every path does.
    pub(super) fn always_exits(&self, st: u32) -> bool {
        let s = &self.syntax;
        match s.kind(st) {
            Some(K::Return | K::Throw | K::Break | K::Continue) => true,
            Some(K::Block) => s.all(st, F::Body).last().is_some_and(|l| self.always_exits(l)),
            Some(K::If) => {
                s.get(st, F::Consequent).is_some_and(|c| self.always_exits(c)) && s.get(st, F::Alternate).is_some_and(|a| self.always_exits(a))
            }
            _ => false,
        }
    }

    /// `t` where `test` evaluated to `sense`.
    fn apply(&self, test: u32, sense: bool, t: Ty, key: &str) -> Ty {
        let s = &self.syntax;
        let is_key = |n: u32| self.ref_key(n).as_deref() == Some(key);
        match s.kind(test) {
            Some(K::Paren) => s.get(test, F::Expr).map_or(t.clone(), |e| self.apply(e, sense, t, key)),
            Some(K::Unary) if s.word(test, F::Op) == Some("!") => s.get(test, F::Arg).map_or(t.clone(), |a| self.apply(a, !sense, t, key)),
            Some(K::Logical) => {
                let (Some(l), Some(r)) = (s.get(test, F::Left), s.get(test, F::Right)) else { return t };
                match (s.word(test, F::Op), sense) {
                    (Some("&&"), true) | (Some("||"), false) => {
                        let t = self.apply(l, sense, t, key);
                        self.apply(r, sense, t, key)
                    }
                    _ => t,
                }
            }
            _ if is_key(test) => truthy(t, sense, &self.typing().checker),
            Some(K::Binary) => {
                let (Some(l), Some(r)) = (s.get(test, F::Left), s.get(test, F::Right)) else { return t };
                let op = s.word(test, F::Op).unwrap_or("");
                let (positive, loose) = match op {
                    "===" => (sense, false),
                    "!==" => (!sense, false),
                    "==" => (sense, true),
                    "!=" => (!sense, true),
                    "instanceof" if is_key(l) && sense => {
                        return match self.type_of(r) {
                            Ty::Class(sym) => Ty::Named(sym, Vec::new()),
                            _ => t,
                        }
                    }
                    _ => return t,
                };
                // `typeof key === "kind"`
                let typeof_of = |n: u32| s.is(n, K::Unary) && s.word(n, F::Op) == Some("typeof") && s.get(n, F::Arg).is_some_and(is_key);
                let literal = |n: u32| s.is(n, K::String).then(|| s.text(n).trim_matches(|c| c == '"' || c == '\'' || c == '`'));
                if let Some(kind) = if typeof_of(l) { literal(r) } else if typeof_of(r) { literal(l) } else { None } {
                    if positive && matches!(t, Ty::Unknown | Ty::IntrinsicUnknown | Ty::Any) {
                        match kind {
                            "string" => return Ty::Prim("string"),
                            "number" => return Ty::Prim("number"),
                            "boolean" => return Ty::Prim("boolean"),
                            "bigint" => return Ty::Prim("bigint"),
                            "symbol" => return Ty::Prim("symbol"),
                            "undefined" => return Ty::Undefined,
                            "function" if matches!(t, Ty::Unknown | Ty::IntrinsicUnknown) => return self.typing().checker.global_named("Function", Vec::new()),
                            "object" if matches!(t, Ty::Unknown | Ty::IntrinsicUnknown) => return Ty::union(vec![Ty::Prim("object"), Ty::Null]),
                            _ => {}
                        }
                    }
                    return self.keep(t, |m| self.typeof_kind(m) == Some(kind), positive);
                }
                let other = if is_key(l) { r } else if is_key(r) { l } else { return t };
                let nullish = match s.kind(other) {
                    Some(K::Null) => Some(Ty::Null),
                    Some(K::Ident) if s.text(other) == "undefined" => Some(Ty::Undefined),
                    _ => None,
                };
                match nullish {
                    Some(n) => self.keep(t, |m| matches!(m, Ty::Null | Ty::Undefined | Ty::Void) && (loose || same_nullish(m, &n)), positive),
                    None => t,
                }
            }
            // `Array.isArray(key)`
            Some(K::Call) => {
                let callee = s.get(test, F::Callee).map(|c| s.text(c));
                let arg = s.all(test, F::Arg).next();
                if callee == Some("Array.isArray") && arg.is_some_and(is_key) {
                    if matches!(t, Ty::Any) {
                        return if sense { Ty::array(Ty::Any) } else { t };
                    }
                    return self.keep(t, |m| matches!(m, Ty::Array(_) | Ty::Tuple(_)), sense);
                }
                if let Some(target) = self.predicate_type(test, key) {
                    return predicate(t, &target, sense, &self.typing().checker);
                }
                t
            }
            _ => t,
        }
    }

    /// Narrow only the local immutable nongeneric guard route already supported.
    fn predicate_type(&self, call: u32, key: &str) -> Option<Ty> {
        let s = &self.syntax;
        let callee = s.unparen(s.get(call, F::Callee)?);
        if !s.is(callee, K::Ident) { return None }
        let decl = self.decl_of(callee)?;
        if self.binding(decl)?.writes > 0 { return None }
        let function = s.parent(decl).filter(|&parent| s.is(parent, K::Function))?;
        if s.all(function, F::TypeParam).next().is_some()
            || matches!(self.type_of(callee), Ty::Fn(callable) if callable.sigs.len() > 1) { return None }
        let (argument, predicate) = self.resolved_predicate(call)?;
        if predicate.asserts || self.ref_key(argument).as_deref() != Some(key) { return None }
        let ty = predicate.target?;
        (!matches!(ty, Ty::Unknown | Ty::IntrinsicUnknown | Ty::Param(_))).then_some(ty)
    }

    /// Members of `t` for which `pred` is `want`; `any` and unknowns stay.
    fn keep(&self, t: Ty, pred: impl Fn(&Ty) -> bool, want: bool) -> Ty {
        match t {
            t if guard_opaque(&t) => t,
            Ty::Union(items) => Ty::union(items.into_iter().filter(|m| guard_opaque(m) || pred(m) == want).collect()),
            t if pred(&t) == want => t,
            _ => Ty::Never,
        }
    }

    /// What `typeof` says for a (non-union) type, when it is certain.
    fn typeof_kind(&self, t: &Ty) -> Option<&'static str> {
        let ck = &self.typing().checker;
        Some(match t {
            Ty::Prim("string") | Ty::Lit(_, "string") => "string",
            Ty::Prim("number") | Ty::Lit(_, "number") => "number",
            Ty::Prim("boolean") | Ty::Lit(_, "boolean") => "boolean",
            Ty::Prim("bigint") | Ty::Lit(_, "bigint") => "bigint",
            Ty::Prim("symbol") => "symbol",
            Ty::Undefined | Ty::Void => "undefined",
            Ty::Fn(_) | Ty::Class(_) => "function",
            Ty::Named(sym, _) if ck.is_enum(sym) => {
                if ck.is_string_enum(sym) {
                    "string"
                } else {
                    "number"
                }
            }
            Ty::Null | Ty::Array(_) | Ty::Tuple(_) | Ty::Obj(..) | Ty::Named(..) | Ty::Prim("object") => "object",
            _ => return None,
        })
    }
}

fn same_nullish(m: &Ty, n: &Ty) -> bool {
    matches!((m, n), (Ty::Null, Ty::Null) | (Ty::Undefined | Ty::Void, Ty::Undefined))
}

fn predicate(t: Ty, target: &Ty, sense: bool, ck: &Checker<'_>) -> Ty {
    if let Ty::Union(parts) = t {
        return Ty::union(parts.into_iter().map(|part| predicate(part, target, sense, ck)).collect());
    }
    if matches!(t, Ty::Any | Ty::Unknown | Ty::IntrinsicUnknown) { return if sense { target.clone() } else { t } }
    let forward = ck.assignable(&t, target);
    if forward == Some(true) { return if sense { t } else { Ty::Never } }
    if !sense { return t }
    match ck.assignable(target, &t) {
        Some(true) => target.clone(),
        _ => Ty::Inter(vec![t, target.clone()]),
    }
}

fn guard_opaque(t: &Ty) -> bool {
    matches!(t, Ty::Any | Ty::Unknown | Ty::IntrinsicUnknown | Ty::Param(_) | Ty::Inter(_))
}

/// `t` where it tested truthy (`sense`) or falsy.
pub(super) fn truthy(t: Ty, sense: bool, ck: &Checker<'_>) -> Ty {
    let items = match t {
        Ty::Union(items) => items,
        t if guard_opaque(&t) => return t,
        t => vec![t],
    };
    let kept = items
        .into_iter()
        .filter_map(|m| {
            if guard_opaque(&m) { return Some(m) }
            match (ck.literal_falsy(&m), sense) {
                // Truthy: drop what is always falsy.
                (Some(true), true) => None,
                (Some(false), false) => None,
                (None, false) if matches!(m, Ty::Prim("boolean")) => Some(Ty::Lit("false".into(), "boolean")),
                (None, true) if matches!(m, Ty::Prim("boolean")) => Some(Ty::Lit("true".into(), "boolean")),
                _ => Some(m),
            }
        })
        .collect();
    Ty::union(kept)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::tests::with_native_file;

    #[test]
    fn known_union_arms_narrow_without_resolving_opaque_members() {
        with_native_file("", |file, _| {
            let ck = &file.typing().checker;
            let opaque = [
                Ty::Param("T".into()), Ty::Unknown, Ty::IntrinsicUnknown,
                Ty::Inter(vec![Ty::Param("T".into()), Ty::Prim("object")]),
            ];
            for member in opaque {
                let nullable = Ty::Union(vec![member.clone(), Ty::Null]);
                let known_non_null = Ty::union(vec![member.clone()]);
                let positive = truthy(nullable.clone(), true, ck);
                let negative = truthy(nullable.clone(), false, ck);
                let compared = file.keep(nullable, |part| matches!(part, Ty::Null), false);
                if known_non_null.is_unknown() {
                    assert!(positive.is_unknown() && compared.is_unknown());
                    assert!(!matches!(negative, Ty::Never | Ty::Null));
                } else {
                    assert!(positive.same_identity(&known_non_null));
                    assert!(negative.same_identity(&Ty::union(vec![member, Ty::Null])));
                    assert!(compared.same_identity(&known_non_null));
                }
            }
        });
    }

    #[test]
    fn generic_guards_refine_known_null_arms_on_each_guard_route() {
        let source = r#"
            function truth<T>(x: T | null) { if (x) { void /*@*/x; } }
            function early<T>(x: T | null) { if (!x) return; void /*@*/x; }
            function equal<T>(x: T | null) { if (x !== null) { void /*@*/x; } }
            function exit<T>(x: T | null) { if (x === null) return; void /*@*/x; }
            function falsy<T>(x: T | null) { if (!x) { void /*@*/x; } }
        "#;
        with_native_file(source, |file, marks| {
            let parameter = Ty::Param("T".into());
            for &mark in &marks[..4] {
                assert!(file.type_of(mark).same_identity(&parameter));
            }
            assert!(file.type_of(marks[4]).same_identity(&Ty::union(vec![parameter, Ty::Null])));
        });
    }

    #[test]
    fn guard_refinement_does_not_cross_writes_functions_or_other_references() {
        let source = r#"
            function written<T>(x: T | null) { if (!x) return; x = null; void /*@*/x; }
            function nested<T>(x: T | null) { if (!x) return; const child = () => /*@*/x; }
            function other<T>(x: T | null, y: T | null) { if (!y) return; void /*@*/x; }
        "#;
        with_native_file(source, |file, marks| {
            let nullable = Ty::union(vec![Ty::Param("T".into()), Ty::Null]);
            for &mark in marks {
                assert!(file.type_of(mark).same_identity(&nullable));
            }
        });
    }
}
