//! Code paths, as eslint's code path analysis sees them, for one function
//! (or the program) at a time: where control can go through statements and
//! short-circuiting expressions, with an abstract state carried along.
//!
//! [`walk`] runs a [`Domain`] over a body: the domain says how its state
//! joins where paths meet and what each node does to it. `None` is "no path
//! gets here": unreachable code. Loops are run until their state settles.
//! Like eslint, `if` tests are not evaluated, but loop tests that are plain
//! literals are (`while (true)` only ends by `break`).
//!
//! Nested functions and classes are separate code paths: their bodies are
//! not entered (class heritage, computed keys and decorators run in place).

use std::collections::HashMap;

use super::kinds::{Field as F, Kind as K};
use super::syntax::Syntax;

pub trait Domain {
    type S: Clone + PartialEq;
    /// Where paths meet.
    fn join(&self, a: &Self::S, b: &Self::S) -> Self::S;
    /// After `node` (an expression, or a statement's own effect) ran, its
    /// children first. Loops run until their state settles, so a node can
    /// be stepped more than once: the last time has its final state.
    fn step(&mut self, _s: &Syntax, _node: u32, _state: &mut Self::S) {}
    /// Before an expression's children run (`x += await y` reads `x`
    /// before the `await`, though `step` sees the target after the value).
    fn before_expr(&mut self, _s: &Syntax, _node: u32, _state: &mut Self::S) {}
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Jump<'t> {
    Break(Option<&'t str>),
    Continue(Option<&'t str>),
    Return,
    Throw,
}

/// How a statement can end: normally (with the state then), or by jumps.
struct Out<'t, S> {
    normal: Option<S>,
    jumps: Vec<(Jump<'t>, S)>,
}

impl<S> Out<'_, S> {
    fn normal(s: Option<S>) -> Self {
        Out { normal: s, jumps: Vec::new() }
    }
}

/// Runs `d` over a function's body (a block, an arrow's expression, or the
/// program), from `start`.
pub fn walk<D: Domain>(s: &Syntax, d: &mut D, body: u32, start: D::S) -> Option<D::S> {
    let mut w = Walker { s, d, first_throw: Vec::new() };
    let out = if is_expression(s, body) {
        let st = w.expr(body, start);
        Out::normal(Some(st))
    } else if s.is(body, K::Block) || s.is(body, K::Program) {
        w.list(s.all(body, F::Body).collect(), Some(start))
    } else {
        w.stmt(body, Some(start))
    };
    let mut fall = out.normal;
    for (j, st) in out.jumps {
        match j {
            Jump::Return | Jump::Throw => {}
            // Stray `break`/`continue` (a syntax error) end the body.
            _ => fall = join_opt(&*w.d, fall.take(), Some(st)),
        }
    }
    fall
}

fn join_opt<D: Domain + ?Sized>(d: &D, a: Option<D::S>, b: Option<D::S>) -> Option<D::S> {
    match (a, b) {
        (Some(a), Some(b)) => Some(d.join(&a, &b)),
        (a, None) => a,
        (None, b) => b,
    }
}

fn is_expression(s: &Syntax, node: u32) -> bool {
    !matches!(
        s.kind(node),
        Some(
            K::Program
                | K::Block
                | K::ExprStmt
                | K::Var
                | K::If
                | K::For
                | K::ForIn
                | K::ForOf
                | K::While
                | K::DoWhile
                | K::Return
                | K::Throw
                | K::Break
                | K::Continue
                | K::Switch
                | K::Try
                | K::Labeled
                | K::Empty
                | K::Debugger
                | K::With
                | K::Function
                | K::Class
                | K::Import
                | K::ImportEquals
                | K::Export
                | K::ExportAssign
                | K::Interface
                | K::TypeAlias
                | K::Enum
                | K::Namespace
        )
    )
}

/// A loop test that is a plain literal: its truthiness.
fn constant_test(s: &Syntax, test: u32) -> Option<bool> {
    let text = s.text(test);
    match s.kind(test)? {
        K::Bool => Some(text == "true"),
        K::Null => Some(false),
        K::Number => super::statics::number(text).map(|n| n != 0.0 && !n.is_nan()),
        K::String | K::Template if s.all(test, F::Expr).next().is_none() => Some(text.len() > 2),
        K::Regex | K::BigInt => Some(!matches!(text, "0n")),
        _ => None,
    }
}

struct Walker<'a, 's, 't, D: Domain> {
    s: &'s Syntax<'t>,
    d: &'a mut D,
    /// Enclosing `try` blocks that have a `catch`, innermost last: the
    /// state at the first node in them that can throw (as eslint marks it).
    first_throw: Vec<Option<Option<D::S>>>,
}

impl<'t, D: Domain> Walker<'_, '_, 't, D> {
    fn join(&self, a: Option<D::S>, b: Option<D::S>) -> Option<D::S> {
        join_opt(&*self.d, a, b)
    }

    fn list(&mut self, stmts: Vec<u32>, mut st: Option<D::S>) -> Out<'t, D::S> {
        let mut jumps = Vec::new();
        for c in stmts {
            let out = self.stmt(c, st);
            jumps.extend(out.jumps);
            st = out.normal;
        }
        Out { normal: st, jumps }
    }

    fn opt_expr(&mut self, node: Option<u32>, st: Option<D::S>) -> Option<D::S> {
        match (node, st) {
            (Some(n), Some(st)) => Some(self.expr(n, st)),
            (_, st) => st,
        }
    }

    fn stmt(&mut self, node: u32, st: Option<D::S>) -> Out<'t, D::S> {
        let s = self.s;
        let Some(kind) = s.kind(node) else { return Out::normal(st) };
        let out = match kind {
            K::Block => self.list(s.all(node, F::Body).collect(), st),
            K::Empty | K::Debugger | K::Function | K::Interface | K::TypeAlias | K::Import | K::ImportEquals => Out::normal(st),
            K::ExprStmt | K::ExportAssign => Out::normal(self.opt_expr(s.get(node, F::Expr), st)),
            K::Var => {
                let mut st = st;
                for d in s.all(node, F::Decl).collect::<Vec<_>>() {
                    st = self.opt_expr(s.get(d, F::Init), st);
                    if let Some(x) = st.as_mut() {
                        self.d.step(s, d, x);
                    }
                }
                Out::normal(st)
            }
            K::Class | K::Enum | K::Namespace => {
                let st = st.map(|mut x| {
                    self.class_parts(node, &mut x);
                    self.d.step(s, node, &mut x);
                    x
                });
                Out::normal(st)
            }
            K::Export => match s.get(node, F::Decl).or_else(|| s.get(node, F::Expr)) {
                Some(d) if !is_expression(s, d) => self.stmt(d, st),
                Some(e) => Out::normal(self.opt_expr(Some(e), st)),
                None => Out::normal(st),
            },
            K::If => {
                let st = self.opt_expr(s.get(node, F::Test), st);
                let cons = match s.get(node, F::Consequent) {
                    Some(c) => self.stmt(c, st.clone()),
                    None => Out::normal(st.clone()),
                };
                let alt = match s.get(node, F::Alternate) {
                    Some(a) => self.stmt(a, st),
                    None => Out::normal(st),
                };
                let mut jumps = cons.jumps;
                jumps.extend(alt.jumps);
                Out { normal: self.join(cons.normal, alt.normal), jumps }
            }
            K::Return | K::Throw => {
                let mut st = self.opt_expr(s.get(node, F::Arg), st);
                if let Some(x) = st.as_mut() {
                    self.d.step(s, node, x);
                }
                let jump = if kind == K::Return { Jump::Return } else { Jump::Throw };
                Out { normal: None, jumps: st.map(|x| (jump, x)).into_iter().collect() }
            }
            K::Break | K::Continue => {
                let label = s.get(node, F::Label).map(|l| s.text(l));
                let jump = if kind == K::Break { Jump::Break(label) } else { Jump::Continue(label) };
                Out { normal: None, jumps: st.map(|x| (jump, x)).into_iter().collect() }
            }
            K::Labeled => {
                let label = s.get(node, F::Label).map(|l| s.text(l));
                let out = match s.get(node, F::Body) {
                    Some(b) => self.stmt(b, st),
                    None => Out::normal(st),
                };
                self.close(out, |j| j == Jump::Break(label))
            }
            K::With => {
                let st = self.opt_expr(s.get(node, F::Object), st);
                match s.get(node, F::Body) {
                    Some(b) => self.stmt(b, st),
                    None => Out::normal(st),
                }
            }
            K::While => {
                let test = s.get(node, F::Test);
                self.lp(node, st, None, test, s.get(node, F::Body), None, false)
            }
            K::DoWhile => {
                let test = s.get(node, F::Test);
                self.lp(node, st, None, test, s.get(node, F::Body), None, true)
            }
            K::For => {
                let init = s.get(node, F::Init);
                let st = match init {
                    Some(i) if !is_expression(s, i) => self.stmt(i, st).normal,
                    i => self.opt_expr(i, st),
                };
                self.lp(node, st, None, s.get(node, F::Test), s.get(node, F::Body), s.get(node, F::Update), false)
            }
            K::ForIn | K::ForOf => {
                let st = self.opt_expr(s.get(node, F::Right), st);
                // The left side is written each round.
                let left = s.get(node, F::Left);
                self.lp(node, st, left, None, s.get(node, F::Body), None, false)
            }
            K::Switch => self.switch(node, st),
            K::Try => self.try_(node, st),
            _ => Out::normal(self.opt_expr(Some(node), st)),
        };
        out
    }

    /// Jumps matching `ends` resume after the statement.
    fn close(&self, out: Out<'t, D::S>, ends: impl Fn(Jump<'t>) -> bool) -> Out<'t, D::S> {
        let mut normal = out.normal;
        let mut jumps = Vec::new();
        for (j, x) in out.jumps {
            if ends(j) {
                normal = self.join(normal, Some(x));
            } else {
                jumps.push((j, x));
            }
        }
        Out { normal, jumps }
    }

    /// The labels a loop answers to: its own (a labelled loop), or none.
    fn loop_label(&self, node: u32) -> Option<&'t str> {
        let s = self.s;
        s.parent(node).filter(|&p| s.is(p, K::Labeled)).and_then(|p| s.get(p, F::Label)).map(|l| s.text(l))
    }

    /// A loop: `left` written each round (for-in/of), `test` before each
    /// round (after the first for do-while), `update` after each.
    #[allow(clippy::too_many_arguments)]
    fn lp(&mut self, node: u32, st: Option<D::S>, left: Option<u32>, test: Option<u32>, body: Option<u32>, update: Option<u32>, body_first: bool) -> Out<'t, D::S> {
        let s = self.s;
        let label = self.loop_label(node);
        let is_for = s.is(node, K::For);
        let constant = match test {
            Some(t) => constant_test(s, t),
            // `for (;;)`
            None if is_for => Some(true),
            None => None,
        };
        let mine = |j: Jump<'t>, brk: bool| match j {
            Jump::Break(None) if brk => true,
            Jump::Continue(None) if !brk => true,
            Jump::Break(Some(l)) if brk => Some(l) == label,
            Jump::Continue(Some(l)) if !brk => Some(l) == label,
            _ => false,
        };
        let mut head = st.clone();
        let mut exits: Option<D::S> = None;
        let mut jumps_out = Vec::new();
        // Until the state at the head settles (a few rounds at most).
        for round in 0..4 {
            exits = None;
            jumps_out.clear();
            let mut before_body = head.clone();
            if !body_first {
                before_body = self.opt_expr(test, before_body);
                // The test fails: leave (unless it never can).
                if constant != Some(true) && (test.is_some() || is_for) {
                    exits = self.join(exits, before_body.clone());
                }
                if constant == Some(false) {
                    before_body = None;
                }
            }
            let mut entry = before_body;
            if let (Some(l), Some(x)) = (left, entry.as_mut()) {
                self.write_target(l, x);
            }
            if left.is_some() || test.is_none() && !is_for {
                // for-in/of: may run zero times.
                exits = self.join(exits, head.clone());
            }
            let out = match body {
                Some(b) => self.stmt(b, entry.take()),
                None => Out::normal(entry.take()),
            };
            let mut next = out.normal;
            for (j, x) in out.jumps {
                if mine(j, true) {
                    exits = self.join(exits, Some(x));
                } else if mine(j, false) {
                    next = self.join(next, Some(x));
                } else {
                    jumps_out.push((j, x));
                }
            }
            next = self.opt_expr(update, next);
            if body_first {
                next = self.opt_expr(test, next);
                if constant != Some(true) {
                    exits = self.join(exits, next.clone());
                }
                if constant == Some(false) {
                    next = None;
                }
            }
            let new_head = self.join(st.clone(), next);
            if new_head == head && round > 0 {
                break;
            }
            head = new_head;
        }
        Out { normal: exits, jumps: jumps_out }
    }

    fn switch(&mut self, node: u32, st: Option<D::S>) -> Out<'t, D::S> {
        let s = self.s;
        let mut test_state = self.opt_expr(s.get(node, F::Discriminant), st);
        let mut fall: Option<D::S> = None;
        let mut jumps = Vec::new();
        let mut has_default = false;
        for c in s.all(node, F::Case).collect::<Vec<_>>() {
            let entry = match s.get(c, F::Test) {
                Some(t) => {
                    test_state = self.opt_expr(Some(t), test_state);
                    self.join(test_state.clone(), fall.take())
                }
                None => {
                    has_default = true;
                    self.join(test_state.clone(), fall.take())
                }
            };
            let out = self.list(s.all(c, F::Body).collect(), entry);
            fall = out.normal;
            jumps.extend(out.jumps);
        }
        let mut normal = fall;
        if !has_default {
            normal = self.join(normal, test_state);
        }
        self.close(Out { normal, jumps }, |j| j == Jump::Break(None))
    }

    fn try_(&mut self, node: u32, st: Option<D::S>) -> Out<'t, D::S> {
        let s = self.s;
        let handler = s.get(node, F::Handler);
        let tracked = handler.is_some() || s.get(node, F::Finalizer).is_some();
        if tracked {
            self.first_throw.push(None);
        }
        let block = match s.get(node, F::Block) {
            Some(b) => self.stmt(b, st),
            None => Out::normal(st),
        };
        let (mut normal, mut jumps) = (block.normal, block.jumps);
        // As eslint models it: the first thing in the block that can throw
        // may have thrown (with the state from before it).
        let first = if tracked { self.first_throw.pop().flatten().flatten() } else { None };
        if handler.is_none() {
            if let Some(x) = first.clone() {
                jumps.push((Jump::Throw, x));
            }
        }
        if let Some(h) = handler {
            // The handler is entered from `throw`s, from that first throwing
            // node, and from wherever the block ends.
            let mut thrown = self.join(normal.clone(), first);
            jumps.retain(|(j, x)| {
                if *j == Jump::Throw {
                    thrown = join_opt(&*self.d, thrown.take(), Some(x.clone()));
                    false
                } else {
                    true
                }
            });
            let mut entry = thrown;
            if let (Some(p), Some(x)) = (s.get(node, F::Param), entry.as_mut()) {
                self.write_target(p, x);
            }
            let out = self.stmt(h, entry);
            normal = self.join(normal, out.normal);
            jumps.extend(out.jumps);
        }
        let Some(fin) = s.get(node, F::Finalizer) else { return Out { normal, jumps } };
        // The finally block runs on every way out, and continues each only
        // if it completes itself. The jumps' paths and the normal one are
        // kept apart (walked separately), as eslint keeps their segments.
        let mut out_jumps: Vec<(Jump<'t>, D::S)> = Vec::new();
        if !jumps.is_empty() {
            let mut entry = None;
            for (_, x) in &jumps {
                entry = self.join(entry, Some(x.clone()));
            }
            let fin_out = self.stmt(fin, entry);
            if let Some(after) = fin_out.normal {
                out_jumps.extend(jumps.into_iter().map(|(j, _)| (j, after.clone())));
            }
            out_jumps.extend(fin_out.jumps);
        }
        if normal.is_some() || out_jumps.is_empty() {
            let fin_out = self.stmt(fin, normal);
            out_jumps.extend(fin_out.jumps);
            return Out { normal: fin_out.normal, jumps: out_jumps };
        }
        Out { normal: None, jumps: out_jumps }
    }

    /// A destructuring target or loop variable being written: its defaults
    /// and computed keys run.
    fn write_target(&mut self, node: u32, st: &mut D::S) {
        let s = self.s;
        if s.is(node, K::Var) {
            for d in s.all(node, F::Decl).collect::<Vec<_>>() {
                self.d.step(s, d, st);
            }
            return;
        }
        let next = self.expr(node, st.clone());
        *st = next;
    }

    fn class_parts(&mut self, node: u32, st: &mut D::S) {
        let s = self.s;
        for f in [F::Decorator, F::Extends] {
            for c in s.all(node, f).collect::<Vec<_>>() {
                *st = self.expr(c, st.clone());
            }
        }
        for m in s.all(node, F::Member).collect::<Vec<_>>() {
            if let Some(k) = s.get(m, F::Key).filter(|&k| s.is(k, K::Computed)) {
                *st = self.expr(k, st.clone());
            }
        }
    }

    /// An expression, in evaluation order, forking at `&&`, `||`, `??`,
    /// `?:`, `?.` and default values.
    fn expr(&mut self, node: u32, st: D::S) -> D::S {
        let s = self.s;
        let Some(kind) = s.kind(node) else { return st };
        // The first node of a `try` block that can throw: the state as it
        // starts.
        if matches!(kind, K::Call | K::New | K::Member | K::Index | K::Yield | K::ImportCall) {
            if let Some(slot @ None) = self.first_throw.last_mut() {
                *slot = Some(Some(st.clone()));
            }
        }
        let mut st = st;
        self.d.before_expr(s, node, &mut st);
        match kind {
            // Separate code paths (or no code at all).
            K::Function | K::Arrow => {}
            K::Class => self.class_parts(node, &mut st),
            K::Logical | K::Conditional => {
                let (first, rest): (Option<u32>, Vec<u32>) = if kind == K::Logical {
                    (s.get(node, F::Left), s.get(node, F::Right).into_iter().collect())
                } else {
                    (s.get(node, F::Test), [F::Consequent, F::Alternate].iter().filter_map(|&f| s.get(node, f)).collect())
                };
                if let Some(f) = first {
                    st = self.expr(f, st);
                }
                st = match (kind, rest.as_slice()) {
                    (K::Logical, [r]) => {
                        let taken = self.expr(*r, st.clone());
                        self.d.join(&st, &taken)
                    }
                    (_, [a, b]) => {
                        let x = self.expr(*a, st.clone());
                        let y = self.expr(*b, st);
                        self.d.join(&x, &y)
                    }
                    _ => st,
                };
            }
            K::Assign if s.word(node, F::Op).is_some_and(|op| matches!(op, "&&=" | "||=" | "??=")) => {
                if let Some(l) = s.get(node, F::Left) {
                    st = self.expr(l, st);
                }
                if let Some(r) = s.get(node, F::Right) {
                    let taken = self.expr(r, st.clone());
                    st = self.d.join(&st, &taken);
                }
            }
            // `a = b`: the value first, then the target is written.
            K::Assign => {
                if let Some(r) = s.get(node, F::Right) {
                    st = self.expr(r, st);
                }
                if let Some(l) = s.get(node, F::Left) {
                    st = self.expr(l, st);
                }
            }
            // `x = 1` inside a pattern: only taken when the value is missing.
            K::AssignPattern => {
                if let Some(l) = s.get(node, F::Left) {
                    st = self.expr(l, st);
                }
                if let Some(r) = s.get(node, F::Right) {
                    let taken = self.expr(r, st.clone());
                    st = self.d.join(&st, &taken);
                }
            }
            // `a?.b`: everything after the `?.` may be skipped.
            K::Member | K::Index | K::Call if s.get(node, F::Optional).is_some() => {
                let head = match kind {
                    K::Call => s.get(node, F::Callee),
                    _ => s.get(node, F::Object),
                };
                if let Some(h) = head {
                    st = self.expr(h, st);
                }
                let mut taken = st.clone();
                for (f, c) in s.kids(node).collect::<Vec<_>>() {
                    if Some(c) != head && matches!(f, Some(F::Arg | F::Index | F::Property)) {
                        taken = self.expr(c, taken);
                    }
                }
                st = self.d.join(&st, &taken);
            }
            _ => {
                for (f, c) in s.kids(node).collect::<Vec<_>>() {
                    // Types and names do not run.
                    if matches!(f, Some(F::Type | F::TypeArg | F::TypeParam | F::ReturnType | F::Op | F::Optional | F::Kind | F::Modifier)) {
                        continue;
                    }
                    if f == Some(F::Property) && kind == K::Member {
                        continue;
                    }
                    if f == Some(F::Key) && !s.is(c, K::Computed) {
                        continue;
                    }
                    st = self.expr(c, st);
                }
            }
        }
        self.d.step(s, node, &mut st);
        st
    }
}

// ---- reachability ----------------------------------------------------------------

#[derive(Default)]
pub struct Reach {
    /// Function-like node (or the program) -> its end is reachable (it can
    /// finish without `return` / `throw`).
    pub falls_off: HashMap<u32, bool>,
}

struct ReachDomain;

impl Domain for ReachDomain {
    type S = ();
    fn join(&self, _: &(), _: &()) {}
}

/// Code bodies of a file: the program and every function, method, arrow,
/// static block and class field initializer.
pub fn bodies(s: &Syntax) -> Vec<(u32, u32)> {
    let mut out = vec![(s.root(), s.root())];
    for id in 0..s.tree.nodes.len() as u32 {
        match s.kind(id) {
            Some(K::Function | K::Arrow | K::Method) => {
                if let Some(b) = s.get(id, F::Body) {
                    out.push((id, b));
                }
            }
            Some(K::StaticBlock) => {
                if let Some(b) = s.get(id, F::Body) {
                    out.push((id, b));
                }
            }
            Some(K::ClassProp) => {
                if let Some(i) = s.get(id, F::Init) {
                    out.push((id, i));
                }
            }
            _ => {}
        }
    }
    out
}

pub fn reach(s: &Syntax) -> Reach {
    let mut reach = Reach::default();
    let mut d = ReachDomain;
    for (owner, body) in bodies(s) {
        reach.falls_off.insert(owner, walk(s, &mut d, body, ()).is_some());
    }
    reach
}
