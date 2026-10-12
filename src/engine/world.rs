//! The files behind type questions (docs/design/type-layers.md "World:
//! files behind questions"): the one engine owner of trees, texts, names
//! and VM runs that type-layer engines see through [`World`].
//!
//! File 0 is the linted file. Other files (units) load when module
//! resolution reaches them; until that lands every cross-file question is
//! unknown.

use std::borrow::Cow;
use std::cell::Cell;

use crate::expr::{Ctx, Value};
use crate::grammar::names::question::{Known, Leaf, ScopeRef, Target, World};
use crate::grammar::names::semantic::Semantic;
use crate::grammar::names::{Resolved, ScopeIndex};
use crate::grammar::{Grammar, Tree};
use crate::vm::{Session, VExpr};

/// One lint's world. Engines hold it as `&dyn World` through [`Files`].
pub struct LintWorld<'a> {
    /// File 0 outside any rule (no locals, no indents).
    ctx: Ctx<'a>,
    /// The question whose conditions are running ([`LintWorld::asking`]);
    /// null outside any.
    question: Cell<*const ()>,
}

impl<'a> LintWorld<'a> {
    pub fn new(ctx: Ctx<'a>) -> Self {
        LintWorld { ctx: Ctx { node: ctx.tree.root, ..ctx }, question: Cell::new(std::ptr::null()) }
    }

    /// File 0's context on `node`, able to reach this world.
    fn on(&'a self, node: u32) -> Ctx<'a> {
        Ctx { node, world: Some(self), ..self.ctx }
    }

    /// Runs `f` with `semantic` as the question in progress, so queries
    /// asked from the conditions `f` runs nest in it.
    fn asking<R>(&self, semantic: &Semantic<'_>, f: impl FnOnce() -> R) -> R {
        let before = self.question.replace((semantic as *const Semantic<'_>).cast());
        let _restore = Restore { slot: &self.question, before };
        f()
    }

    /// The question in progress, if a condition is running under one.
    pub fn question(&self) -> Option<&Semantic<'_>> {
        let at = self.question.get();
        // SAFETY: set only by `asking`, which restores it before the
        // semantic it points to can go away; a caller reads it only while
        // running inside that call.
        (!at.is_null()).then(|| unsafe { &*at.cast::<Semantic<'_>>() })
    }

    /// The node `at` stands for in file 0.
    fn here(at: Leaf) -> Option<u32> {
        match at.target {
            Target::Node(node) if node.file == 0 => Some(node.node),
            _ => None,
        }
    }

    fn key<'s>(&self, text: &'s str) -> Cow<'s, str> {
        match self.ctx.native {
            Some(native) => native.name_key(text),
            None => Cow::Borrowed(text),
        }
    }
}

struct Restore<'c> {
    slot: &'c Cell<*const ()>,
    before: *const (),
}

impl Drop for Restore<'_> {
    fn drop(&mut self) {
        self.slot.set(self.before);
    }
}

/// [`World`] over a lint's files, borrowed for the whole lint (conditions
/// rebuild contexts that point back at it).
#[derive(Clone, Copy)]
pub struct Files<'a>(pub &'a LintWorld<'a>);

impl World for Files<'_> {
    fn grammar(&self) -> &Grammar {
        self.0.ctx.grammar
    }

    fn tree(&self, file: u32) -> Option<&Tree> {
        (file == 0).then_some(self.0.ctx.tree)
    }

    fn text(&self, file: u32) -> Option<&str> {
        (file == 0).then_some(self.0.ctx.source)
    }

    fn names(&self, file: u32) -> Option<&Resolved> {
        (file == 0).then(|| self.0.ctx.names())
    }

    fn index(&self, file: u32) -> Option<&ScopeIndex> {
        let ctx = &self.0.ctx;
        (file == 0).then(|| ctx.names().index(ctx.tree, ctx.source, &|text| self.0.key(text)))
    }

    fn name_key<'s>(&self, _file: u32, text: &'s str) -> Cow<'s, str> {
        self.0.key(text)
    }

    fn module(&self, _at: Leaf, _specifier: &str, _semantic: &Semantic<'_>) -> Known<Option<ScopeRef>> {
        Known::Unknown
    }

    fn condition(&self, at: Leaf, condition: u32, semantic: &Semantic<'_>) -> Known<bool> {
        let world = self.0;
        let lowered = world.ctx.grammar.names.conditions.get(condition as usize).and_then(|c| c.lowered.get());
        let (Some(node), Some(e)) = (LintWorld::here(at), lowered) else { return Known::Unknown };
        world.asking(semantic, || {
            let ctx = world.on(node);
            let held = Session::new(&ctx, node, &[]).holds_on(node, e);
            held.map_or(Known::Unknown, Known::Yes)
        })
    }

    fn operand(&self, at: Leaf, operand: u32, semantic: &Semantic<'_>) -> Known<Value<'static>> {
        let world = self.0;
        let lowered = world.ctx.grammar.names.operands.get(operand as usize).and_then(|o| o.lowered.get());
        let (Some(node), Some(e)) = (LintWorld::here(at), lowered) else { return Known::Unknown };
        world.asking(semantic, || run(&world.on(node), e))
    }

    fn attribute(&self, at: Leaf, attr: u16, semantic: &Semantic<'_>) -> Known<Value<'static>> {
        let world = self.0;
        let Some(node) = LintWorld::here(at) else { return Known::Unknown };
        world.asking(semantic, || run(&world.on(node), &VExpr::Attr(attr)))
    }
}

/// `e`'s value on the context's node; unknown when the VM gives up.
fn run(ctx: &Ctx<'_>, e: &VExpr) -> Known<Value<'static>> {
    let session = Session::new(ctx, ctx.node, &[]);
    let value = session.value(e, None).map(Value::into_static);
    value.map_or(Known::Unknown, Known::Yes)
}
