//! The files behind type questions (docs/design/type-layers.md "World:
//! files behind questions"): the one engine owner of trees, texts, names
//! and VM runs that type-layer engines see through [`World`].
//!
//! File 0 is the linted file. Other files load on module lookup, with
//! shared parse data and per-lint facts, names and VM state.

use std::borrow::Cow;
use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};

use crate::expr::{Ctx, Value};
use crate::grammar::names::question::{Known, Leaf, ScopeRef, Target, World};
use crate::grammar::names::semantic::Semantic;
use crate::grammar::names::modules::compile::Action;
use crate::grammar::names::modules::resolution::{DiscoveryRequest, Host, Resolver};
use crate::grammar::names::question::QueryContext;
use crate::grammar::names::{Resolved, ScopeIndex};
use crate::grammar::{Grammar, Tree};
use crate::vm::{Session, VExpr};

/// One lint's world. Engines hold it as `&dyn World` through [`Files`].
pub struct LintWorld<'a> {
    linter: &'a super::Linter,
    path: &'a Path,
    views: super::arena::Arena<'a>,
    files: RefCell<crate::fx_hash::FxHashMap<PathBuf, u32>>,
    /// File 0 outside any rule (no locals, no indents).
    ctx: Ctx<'a>,
    /// The question whose conditions are running ([`LintWorld::asking`]);
    /// null outside any.
    question: Cell<*const ()>,
}

impl<'a> LintWorld<'a> {
    pub fn new(linter: &'a super::Linter, path: &'a Path, ctx: Ctx<'a>) -> Self {
        LintWorld { linter, path, views: super::arena::Arena::new(linter), files: Default::default(), ctx: Ctx { node: ctx.tree.root, ..ctx }, question: Cell::new(std::ptr::null()) }
    }

    fn file(&'a self, path: &Path) -> Option<u32> {
        if path == self.path { return Some(0); }
        if let Some(&file) = self.files.borrow().get(path) { return Some(file); }
        let unit = self.linter.unit(path)?;
        let file = self.views.insert(unit) + 1;
        self.files.borrow_mut().insert(path.to_owned(), file);
        Some(file)
    }
    /// File 0's context on `node`, able to reach this world.
    fn on(&'a self, file: u32, node: u32) -> Option<Ctx<'a>> {
        if file == 0 {
            self.ctx.tree.nodes.get(node as usize)?;
            return Some(Ctx { node, world: Some(self), ..self.ctx });
        }
        let view = self.views.get(file.checked_sub(1)?)?;
        let unit = &view.unit;
        unit.parsed.tree.nodes.get(node as usize)?;
        let ctx = self.linter.tree_ctx(unit.lang, &unit.parsed.tree, (&unit.text, &view.lines, &unit.rel), &view.memo, &view.reindented, view.native());
        Some(Ctx { file_id: file, node, world: Some(self), ..ctx })
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

    fn here(&'a self, at: Leaf) -> Option<Ctx<'a>> {
        match at.target {
            Target::Node(node) => self.on(node.file, node.node),
            _ => None,
        }
    }

    fn key<'s>(&'a self, file: u32, text: &'s str) -> Cow<'s, str> {
        match self.on(file, 0).and_then(|ctx| ctx.native) {
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
        if file == 0 { return Some(self.0.ctx.tree); }
        Some(&self.0.views.get(file.checked_sub(1)?)?.unit.parsed.tree)
    }

    fn text(&self, file: u32) -> Option<&str> {
        if file == 0 { return Some(self.0.ctx.source); }
        Some(&self.0.views.get(file.checked_sub(1)?)?.unit.text)
    }

    fn names(&self, file: u32) -> Option<&Resolved> {
        Some(self.0.on(file, self.tree(file)?.root)?.names())
    }

    fn index(&self, file: u32) -> Option<&ScopeIndex> {
        let ctx = self.0.on(file, self.tree(file)?.root)?;
        Some(ctx.names().index(ctx.tree, ctx.source, &|text| self.0.key(file, text)))
    }

    fn name_key<'s>(&self, file: u32, text: &'s str) -> Cow<'s, str> {
        self.0.key(file, text)
    }

    fn module(&self, at: Leaf, specifier: &str, semantic: &Semantic<'_>) -> Known<Option<ScopeRef>> {
        let Some(ctx) = self.0.here(at) else { return Known::Unknown };
        let mut node = Some(ctx.node);
        while let Some(id) = node {
            if !semantic.question.budget.step() { return Known::Unknown; }
            let kind = ctx.tree.node(id).matcher;
            if let Some(items) = ctx.grammar.names.kinds.get(kind as usize) {
                for item in &items.modules.items {
                    let Action::Uses(name) = &item.action else { continue };
                    let leaf = Leaf { target: Target::Node(crate::grammar::names::question::NodeRef { file: ctx.file_id, node: id }), ..at };
                    if let Some(condition) = item.when {
                        match self.condition(leaf, condition, semantic) {
                            Known::Yes(true) => {}
                            Known::Yes(false) => continue,
                            Known::Unknown => return Known::Unknown,
                        }
                    }
                    let Some(profile) = ctx.grammar.names.module_resolutions.iter().find(|profile| profile.name == *name) else { return Known::Unknown };
                    // Mode expressions need the resolver context binding.
                    if !profile.modes.is_empty() { return Known::Unknown; }
                    return self.0.asking(semantic, || Resolver { host: self, settings: &profile.settings }.resolve(ctx.file_id, specifier, &[], semantic.question));
                }
            }
            node = ctx.tree.parent(id);
        }
        Known::Unknown
    }

    fn condition(&self, at: Leaf, condition: u32, semantic: &Semantic<'_>) -> Known<bool> {
        let world = self.0;
        let lowered = world.ctx.grammar.names.conditions.get(condition as usize).and_then(|c| c.lowered.get());
        let (Some(ctx), Some(e)) = (world.here(at), lowered) else { return Known::Unknown };
        world.asking(semantic, || {
            let held = Session::new(&ctx, ctx.node, &[]).holds_on(ctx.node, e);
            held.map_or(Known::Unknown, Known::Yes)
        })
    }

    fn operand(&self, at: Leaf, operand: u32, semantic: &Semantic<'_>) -> Known<Value<'static>> {
        let world = self.0;
        let lowered = world.ctx.grammar.names.operands.get(operand as usize).and_then(|o| o.lowered.get());
        let (Some(ctx), Some(e)) = (world.here(at), lowered) else { return Known::Unknown };
        world.asking(semantic, || run(&ctx, e))
    }

    fn attribute(&self, at: Leaf, attr: u16, semantic: &Semantic<'_>) -> Known<Value<'static>> {
        let world = self.0;
        let Some(ctx) = world.here(at) else { return Known::Unknown };
        world.asking(semantic, || run(&ctx, &VExpr::Attr(attr)))
    }
}

impl Host for Files<'_> {
    fn path(&self, file: u32) -> Known<PathBuf> {
        if file == 0 { return Known::Yes(self.0.path.to_owned()); }
        self.0.views.get(file.saturating_sub(1)).map_or(Known::Unknown, |view| Known::Yes(view.unit.path.clone()))
    }

    fn text(&self, path: &Path, _: &QueryContext) -> Known<Option<String>> {
        if path == self.0.path { return Known::Yes(Some(self.0.ctx.source.to_owned())); }
        Known::Yes(super::reads::read(path).map(|text| text.to_string()))
    }

    fn is_file(&self, path: &Path, _: &QueryContext) -> Known<bool> {
        Known::Yes(path == self.0.path || super::reads::is_file(path))
    }

    fn scope(&self, path: &Path, _: &QueryContext) -> Known<Option<ScopeRef>> {
        if path != self.0.path && !super::reads::is_file(path) { return Known::Yes(None); }
        self.0.file(path).map_or(Known::Unknown, |file| Known::Yes(Some(ScopeRef { file, scope: 0 })))
    }

    fn ambient(&self, _: u32, _: &str, _: u64, _: &QueryContext) -> Known<Option<ScopeRef>> {
        Known::Unknown
    }

    fn discover(&self, request: &DiscoveryRequest, _: &QueryContext) -> Known<Vec<PathBuf>> {
        let mut discoveries = self.0.linter.discoveries.lock().expect("unit discoveries");
        let paths = discoveries.entry(request.clone()).or_insert_with(|| super::reads::Cached::compute(|| super::reads::requested(request))).get();
        paths.map_or(Known::Unknown, Known::Yes)
    }
}

/// `e`'s value on the context's node; unknown when the VM gives up.
fn run(ctx: &Ctx<'_>, e: &VExpr) -> Known<Value<'static>> {
    let session = Session::new(ctx, ctx.node, &[]);
    let value = session.value(e, None).map(Value::into_static);
    value.map_or(Known::Unknown, Known::Yes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grammar::names::question::{FrameId, NodeRef, QueryContext};
    use crate::engine::{reads, tests::linter};

    fn with_world<T>(linter: &super::super::Linter, f: impl FnOnce(Files<'_>) -> T) -> T {
        let source = "()";
        let tree = Tree::parse_with(&linter.grammar, 0, source, None, None);
        let lines = crate::line_index::LineIndex::new(source);
        let memo = Default::default();
        let reindented = Default::default();
        let ctx = linter.tree_ctx(0, &tree, (source, &lines, "main.t"), &memo, &reindented, None);
        let world = LintWorld::new(linter, Path::new("/root/main.t"), ctx);
        f(Files(&world))
    }

    #[test]
    fn unit_vm_uses_the_target_file_and_keeps_stable_file_ids() {
        let linter = linter(r#"
            in paren {
                let identity text
                fits marked when text == "(leaf)"
                scope module named text
            }
        "#);
        let path = Path::new("/root/unit-vm.t");
        reads::set_buffer(path, "(leaf)");
        with_world(&linter, |files| {
            let file = files.0.file(path).expect("unit loads");
            assert_ne!(file, 0);
            assert_eq!(files.0.file(path), Some(file));
            let tree = files.tree(file).expect("unit tree");
            let node = tree.order.iter().copied().find(|&node| tree.node(node).matcher == linter.grammar.matcher("paren").unwrap()).unwrap();
            let at = Leaf { target: Target::Node(NodeRef { file, node }), frame: FrameId::ROOT };
            let q = QueryContext::default();
            let semantic = Semantic::new(&files, &linter.grammar.names, &q);
            assert_eq!(files.condition(at, 0, &semantic), Known::Yes(true));
            let Known::Yes(operand) = files.operand(at, 0, &semantic) else { panic!("operand answers") };
            assert_eq!(operand.text(), "(leaf)");
            let Known::Yes(value) = files.attribute(at, 0, &semantic) else { panic!("attribute answers") };
            assert_eq!(value.text(), "(leaf)");
            assert_eq!(World::text(&files, 0), Some("()"));
        });
        reads::drop_buffer(path);
    }

    #[test]
    fn every_unit_consumer_records_text_and_changed_units_are_replaced() {
        let linter = linter("in paren fits marked");
        let path = Path::new("/root/unit-cache.t");
        reads::set_buffer(path, "(before)");
        let first = linter.unit(path).unwrap();
        let second = linter.unit(path).unwrap();
        assert!(std::sync::Arc::ptr_eq(&first, &second));
        for _ in 0..2 {
            let (_, deps) = reads::track(|| with_world(&linter, |files| {
                let file = files.0.file(path).expect("unit loads");
                assert_eq!(World::text(&files, file), Some("(before)"));
            }));
            assert!(deps.any(&mut |dep| matches!(dep, reads::Dep::Text(p, _) if p == path)));
            assert!(!deps.any(&mut |dep| !linter.holds(dep)));
            reads::set_buffer(path, "(after)");
            assert!(deps.any(&mut |dep| !linter.holds(dep)));
            let stale = linter.invalidate(&[path.to_owned()]);
            assert!(linter.is_stale(&stale, &deps));
            reads::set_buffer(path, "(before)");
        }
        reads::set_buffer(path, "(after)");
        linter.invalidate(&[path.to_owned()]);
        with_world(&linter, |files| {
            let file = files.0.file(path).expect("unit reloads");
            assert_eq!(World::text(&files, file), Some("(after)"));
        });
        reads::drop_buffer(path);
    }

    #[test]
    fn module_lookup_loads_a_unit_and_missing_candidates_invalidate_on_creation() {
        use crate::grammar::names::modules::compile::{Conditional, Profile};
        use crate::grammar::names::modules::resolution::{Search, Settings};
        let mut linter = linter("in paren fits marked");
        let matcher = linter.grammar.matcher("paren").unwrap();
        linter.grammar.names.kinds[matcher as usize].modules.items.push(Conditional { action: Action::Uses("unit-test".into()), when: None });
        linter.grammar.names.module_resolutions.push(Profile { name: "unit-test".into(), modes: Vec::new(), settings: Settings {
            search: vec![Search::Relative], extensions: vec![".t".into()], candidates: vec!["{dir}/{name}{ext}".into()], ..Settings::default()
        } });
        let path = Path::new("/root/module-unit.t");
        reads::set_buffer(path, "(remote)");
        with_world(&linter, |files| {
            let tree = files.tree(0).unwrap();
            let node = tree.order.iter().copied().find(|&node| tree.node(node).matcher == matcher).unwrap();
            let at = Leaf { target: Target::Node(NodeRef::here(node)), frame: FrameId::ROOT };
            let q = QueryContext::default();
            let semantic = Semantic::new(&files, &linter.grammar.names, &q);
            let Known::Yes(Some(scope)) = files.module(at, "./module-unit", &semantic) else { panic!("module reaches unit") };
            assert_ne!(scope.file, 0);
            assert_eq!(World::text(&files, scope.file), Some("(remote)"));
            let (answer, deps) = reads::track(|| files.module(at, "./created-unit", &semantic));
            assert_eq!(answer, Known::Yes(None));
            let created = Path::new("/root/created-unit.t");
            reads::set_buffer(created, "()");
            assert!(deps.any(&mut |dep| !linter.holds(dep)));
            assert!(linter.is_stale(&linter.invalidate(&[created.to_owned()]), &deps));
            let Known::Yes(Some(created)) = files.module(at, "./created-unit", &semantic) else { panic!("created module reaches unit") };
            assert_eq!(World::text(&files, created.file), Some("()"));
            reads::drop_buffer(Path::new("/root/created-unit.t"));
        });
        reads::drop_buffer(path);
    }

    #[test]
    fn native_unit_names_use_the_remote_frontend_after_more_units_arrive() {
        let linter = crate::engine::tests::native_linter("");
        let path = Path::new("/root/native-unit.ts");
        let text = r"const \u0061 = 1; a;";
        reads::set_buffer(path, text);
        with_world(&linter, |files| {
            let file = files.0.file(path).unwrap();
            let tree = files.tree(file).unwrap();
            let names = files.names(file).unwrap();
            assert_eq!(files.name_key(file, r"\u0061"), "a");
            let reference = tree.order.iter().copied().find(|&node| {
                let n = tree.node(node);
                &text[n.start as usize..n.end as usize] == "a"
            }).unwrap();
            let declared = names.declaration[&reference];
            let n = tree.node(declared);
            assert_eq!(&text[n.start as usize..n.end as usize], r"\u0061");
            let other = Path::new("/root/native-other-unit.ts");
            reads::set_buffer(other, "const other = 2;");
            let second = files.0.file(other).unwrap();
            assert_ne!(file, second);
            assert_eq!(files.names(file).unwrap().declaration[&reference], declared);
            assert!(files.index(file).unwrap().names(0).any(|(name, _)| name == "a"));
            reads::drop_buffer(other);
        });
        reads::drop_buffer(path);
    }
}
