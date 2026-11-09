//! Variables every rule can use (`text`, `line`, `parent`, ...): their
//! names, docs and values.

use super::indent::{AnchorBase, IndentHit};
use super::{value, Ctx, Value};

pub use dx_ir::builtins::Builtin;

impl<'a> Ctx<'a> {
    pub(crate) fn builtin(&self, b: Builtin) -> Result<Value<'a>, String> {
        let start = self.tree.node(self.node).start as usize;
        Ok(match b {
            Builtin::Environment => self.language_options.map_or_else(
                || Value::record(vec![("settings".into(), Value::list(Vec::new()))]),
                |options| options.environment(self.lang, std::path::Path::new(self.file)),
            ),
            Builtin::Text => Value::Str(value::Text::Read(self.node_text(self.node))),
            Builtin::Indent => Value::from(self.indent_at(start)),
            Builtin::ParentIndent => match self.parent_layer() {
                Some(id) if !self.tree.is_root(id) => self.line_indent(self.lines.line_of(self.tree.node(id).start as usize)),
                _ => Value::from(""),
            },
            Builtin::Indented => Value::Bool(self.indent_hit()?.is_some()),
            Builtin::IndentLevel => match self.indent_hit()? {
                Some(hit) if self.indent_closes(&hit) => Value::Num(hit.extra),
                Some(IndentHit { rule: Some(i), node, extra, .. }) => {
                    let rule = &self.indents[i];
                    let levels = match &rule.by {
                        Some(_) => Ctx::indent_eval(&Ctx { node, ..*self }, self.indents.lowered_by(i))?.num()?,
                        None => 1.0,
                    };
                    Value::Num(extra + if rule.dedent { -levels } else { levels })
                }
                _ => Value::Num(0.0),
            },
            Builtin::IndentAnchor => match self.indent_hit()? {
                Some(IndentHit { base: AnchorBase::Line(line), .. }) => self.line_indent(line),
                Some(IndentHit { base: AnchorBase::Text(text), .. }) => Value::from(text),
                None => Value::from(""),
            },
            Builtin::IndentAnchorLine => match self.indent_hit()? {
                Some(IndentHit { base: AnchorBase::Line(line), .. }) => self.whole_line(line),
                Some(IndentHit { base: AnchorBase::Text(text), .. }) => Value::from(text),
                None => Value::from(""),
            },
            Builtin::IndentCloses => Value::Bool(self.indent_hit()?.is_some_and(|hit| self.indent_closes(&hit))),
            Builtin::IndentHangs => Value::Bool(self.indent_hit()?.is_some_and(|hit| {
                hit.inside.is_some_and(|(inside, _)| {
                    let line_end = self.lines.line_end(self.source, self.lines.line_of(inside));
                    !self.source_between(inside, line_end.max(inside)).trim().is_empty()
                })
            })),
            Builtin::ParentLine => match self.parent_layer() {
                Some(id) if !self.tree.is_root(id) => self.whole_line(self.lines.line_of(self.tree.node(id).start as usize)),
                _ => Value::from(""),
            },
            Builtin::Index => self.tree.field_index(self.node).map_or(Value::Null, Value::from),
            Builtin::GapBefore => {
                let prev = self.tree.parent(self.node).and_then(|p| {
                    self.tree.children(p).iter().map(|e| e.node).filter(|&c| self.tree.node(c).end as usize <= start && c != self.node).max_by_key(|&c| self.tree.node(c).end)
                });
                match prev {
                    Some(c) => Value::from(self.source_between(self.tree.node(c).end as usize, start)),
                    None => Value::Null,
                }
            }
            Builtin::Line => Value::from(self.lines.line_of(start) + 1),
            Builtin::Column => Value::from(self.lines.line_col(self.source, start).1 + 1),
            // Offsets of kept nodes keep their order across edits; what lies
            // between two is read as text (and recorded) when it matters.
            Builtin::Offset => Value::from(start),
            Builtin::EndOffset => Value::from(self.tree.node(self.node).end as usize),
            Builtin::File => Value::from(self.file),
            Builtin::Before => {
                let line_start = self.lines.line_start(self.lines.line_of(start));
                Value::from(self.source_between(line_start, start))
            }
            Builtin::After => {
                let end = self.tree.node(self.node).end as usize;
                // A node ending in (or inside) a line break has nothing
                // after it on its line.
                let line_end = self.lines.line_end(self.source, self.lines.line_of(end));
                if end > start && self.source.as_bytes()[end - 1] == b'\n' || end > line_end {
                    Value::from("")
                } else {
                    Value::from(self.source_between(end, line_end))
                }
            }
            Builtin::PreviousLine => match self.lines.line_of(start) {
                0 => Value::Null,
                line => self.whole_line(line - 1),
            },
            Builtin::NextLine => {
                let end = self.tree.node(self.node).end as usize;
                let last = self.lines.line_of(end.saturating_sub(1).max(start));
                if last + 1 < self.lines.line_count() {
                    Value::from(self.lines.line_text(self.source, last + 1))
                } else {
                    Value::Null
                }
            }
            Builtin::Parent => match self.parent_layer() {
                Some(id) => self.node_value(id),
                None => Value::Null,
            },
            Builtin::TextBefore => Value::from(self.source_between(0, start)),
            Builtin::TextAfter => Value::from(self.source_between(self.tree.node(self.node).end as usize, self.source.len())),
            Builtin::PreviousSibling => match self.prev_sibling() {
                Some(id) => Value::Str(value::Text::Read(self.node_text(id))),
                None => Value::Null,
            },
            Builtin::PreviousGap => match self.prev_sibling() {
                Some(id) => Value::from(self.source_between(self.tree.node(id).end as usize, start)),
                None => Value::Null,
            },
            Builtin::ParentText => match self.parent_layer() {
                Some(id) => Value::Str(value::Text::Read(self.node_text(id))),
                None => Value::Null,
            },
            Builtin::ParentBefore => match self.parent_layer() {
                Some(id) => Value::from(self.source_between(self.tree.node(id).start as usize, start)),
                None => Value::Null,
            },
            // Read where used: a slice of it reads only that slice.
            Builtin::Source => Value::from(self.source),
            Builtin::Node => self.node_value(self.node),
            Builtin::ScopeDeclaration => match self.names().declaration.get(&self.node) {
                Some(&id) => self.node_value(id),
                None => Value::Null,
            },
            Builtin::ScopeReferences => Value::list(
                self.names()
                    .references
                    .get(&self.node)
                    .map(|refs| refs.iter().map(|&id| self.node_value(id)).collect())
                    .unwrap_or_default(),
            ),
            Builtin::ScopeEffect => match self.names().effects.get(&self.node) {
                Some(&(effect, _)) => Value::from(self.grammar.names.vocabulary.effects[effect as usize].as_str()),
                None => Value::Null,
            },
            Builtin::ScopeEffectSite => match self.names().effects.get(&self.node) {
                Some(&(_, site)) => self.node_value(site),
                None => Value::Null,
            },
        })
    }

    /// Node `id` as a value: its text is read only when used as text.
    pub(crate) fn node_value(&self, id: u32) -> Value<'a> {
        let n = self.tree.node(id);
        Value::Node(id, value::Text::Borrowed(&self.source[n.start as usize..n.end as usize]))
    }

    /// Whether this tree's names are being resolved right now (their
    /// conditions are running), so they cannot be read yet.
    pub(crate) fn names_resolving(&self) -> bool {
        self.memo.resolving_names.get()
    }

    /// This tree's names, resolved on first use.
    pub(crate) fn names(&self) -> &'a crate::grammar::names::Resolved {
        assert!(!self.memo.resolving_names.get(), "name conditions cannot read resolved names");
        let cached = self.memo.names.get_or_init(|| {
            let resolve = || {
                self.memo.resolving_names.set(true);
                #[cfg(feature = "work")]
                let _owner = crate::work::owner("names.resolve");
                let incomplete = std::cell::Cell::new(false);
                let names = &self.grammar.names;
                let session = crate::vm::Session::new(self, self.node, &[]);
                // An editor's file keeps each answer with what it read, for
                // the next lint's names ([`AttrMemo::name_conditions`]).
                let tracked = self.tree.regions().is_some();
                let holds = |node: u32, condition: u32, e: &crate::vm::VExpr| {
                    if tracked {
                        if let Some(&(held, _)) = self.memo.name_conditions.borrow().get(&(node, condition)) { return held }
                    }
                    let outer = tracked.then(crate::footprint::begin);
                    let held = session.holds_on(node, e);
                    let read = outer.map(crate::footprint::end);
                    match held {
                        Some(held) => {
                            if let Some(read) = read { self.memo.name_conditions.borrow_mut().insert((node, condition), (held, read)); }
                            held
                        }
                        None => { incomplete.set(true); self.memo.vm.clear_error(); false }
                    }
                };
                let when = |node: u32, condition: u32| {
                    let Some(e) = names.conditions[condition as usize].lowered.get() else { incomplete.set(true); return false };
                    match self.profile {
                        Some(profile) => profile.time_name_condition(|| holds(node, condition, e)),
                        None => holds(node, condition, e),
                    }
                };
                let key = |text: &'a str| match self.native {
                    Some(native) => native.name_key(text),
                    None => std::borrow::Cow::Borrowed(text),
                };
                let operand = |node: u32, id: u32, item: Option<u32>| {
                    let Some(e) = names.operands.get(id as usize).and_then(|operand| operand.lowered.get()) else {
                        incomplete.set(true);
                        return Err(Box::<str>::from("item expression is not compiled"));
                    };
                    session.value_on(node, e, item).ok_or_else(|| {
                        incomplete.set(true);
                        session.error().into()
                    })
                };
                let mut resolved = None;
                let mut run = || {
                    let resolve = || names.resolve(self.tree, self.source, &key, &when, &operand);
                    resolved = Some(match self.profile {
                        Some(profile) => profile.time_names(resolve),
                        None => resolve(),
                    });
                };
                // Readers record the entries they read, not the whole build.
                let complete = crate::footprint::quiet(|| match self.world {
                    Some(world) => world.complete_attribute(run).known().is_some(),
                    None => { run(); true }
                });
                let mut resolved = resolved.expect("name resolution ran");
                resolved.complete &= complete && !incomplete.get();
                if let Some(owner) = &self.tree.owner { resolved.own(owner.clone()) }
                if tracked {
                    if let Some(previous) = self.memo.previous_names.borrow().as_deref() { resolved.renumber_scopes(previous) }
                    resolved.track();
                }
                self.memo.resolving_names.set(false);
                std::sync::Arc::new(resolved)
            };
            match self.world.and_then(|world| world.name_cache(self.file_id)) {
                Some(cache) => match cache.get() {
                    Some(cached) => cached.clone(),
                    None => {
                        let answer = crate::engine::reads::Cached::compute(resolve);
                        if answer.value.complete {
                            let _ = cache.set(answer.clone());
                            cache.get().expect("completed names").clone()
                        } else { answer }
                    }
                },
                None => crate::engine::reads::Cached::compute(resolve),
            }
        });
        cached.as_ref()
    }

    /// The tree's native facts.
    pub(crate) fn native_facts(&self) -> Option<&'a (dyn crate::engine::frontend::Native + 'a)> {
        self.native
    }
}
