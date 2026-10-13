//! Variables every rule can use (`text`, `line`, `parent`, ...): their
//! names, docs and values.

use super::indent::{AnchorBase, IndentHit};
use super::{value, Ctx, Value};

macro_rules! builtins {
    ($($variant:ident = $name:literal: $doc:literal,)*) => {
        /// A built-in variable.
        #[derive(Clone, Copy, PartialEq, Eq, Debug)]
        pub enum Builtin {
            $($variant,)*
        }

        impl Builtin {
            /// Every builtin, in documentation order; `ALL[b as usize] == b`.
            pub const ALL: &'static [Builtin] = &[$(Builtin::$variant,)*];

            pub fn name(self) -> &'static str {
                match self {
                    $(Builtin::$variant => $name,)*
                }
            }

            pub fn doc(self) -> &'static str {
                match self {
                    $(Builtin::$variant => $doc,)*
                }
            }
        }
    };
}

builtins! {
    Text = "text": "the node's text",
    Indent = "indent": "leading whitespace of the line the node starts on",
    ParentIndent = "parent_indent": "leading whitespace of the line the node's layer starts on (empty for the file)",
    ParentLine = "parent_line": "the whole line the node's layer starts on (empty for the file)",
    Indented = "indented": "one of the rule's `indent` lines applies to the node's line (the innermost node it names, walking out from the line's first character)",
    IndentLevel = "indent_level": "levels that `indent` line gives the line past its anchor (its `by`, default 1; negative for `dedent`; 0 on an `in` form's closing line or when not indented)",
    IndentAnchor = "indent_anchor": "leading whitespace of the line that `indent` line measures from: `indent in X` X's line, `indent X` X's owner's line (empty when not indented); lines above read as their indentation fixes leave them",
    IndentAnchorLine = "indent_anchor_line": "that whole anchor line (empty when not indented)",
    IndentCloses = "indent_closes": "the line starts with the end of an `indent in X` inside (its closer), so it sits at the anchor",
    IndentHangs = "indent_hangs": "that `indent in X` inside starts on its opener line (`f(a,⏎ b)`)",
    Line = "line": "1-based line of the node's start",
    Column = "column": "1-based column of the node's start",
    Offset = "offset": "byte offset of the node's start in the file (for `report at`)",
    EndOffset = "end_offset": "byte offset of the node's end",
    File = "file": "the file's path, relative to the project root",
    Before = "before": "text on the node's first line before the node",
    After = "after": "text on the node's last line after the node",
    PreviousLine = "previous_line": "the whole line before the node's first line (null on the first line)",
    NextLine = "next_line": "the whole line after the node's last line (null on the last line)",
    Parent = "parent": "the node this one sits in, within its layer (the file for top nodes; null for the file); test it with `parent is a`",
    ParentText = "parent_text": "text of the layer the node sits in",
    ParentBefore = "parent_before": "text of the layer the node sits in, up to the node",
    TextBefore = "text_before": "all of the file before the node",
    TextAfter = "text_after": "all of the file after the node",
    PreviousSibling = "previous_sibling": "text of the previous node of the same layer in the same parent (null if none)",
    PreviousGap = "previous_gap": "text between that previous sibling and this node (null if none)",
    Source = "source": "the whole file's text",
    Environment = "environment": "raw file-scoped language settings in config order: {settings: [{name, value}]}",
    Index = "index": "the node's position among its parent's children in the same field, from 0 (`ts_call.arg` 2nd argument: 1); null for the root",
    GapBefore = "gap_before": "the text between the previous child of the node's parent (of any layer or field) and the node: blank lines, comments, separators; null for the first child",
    Node = "node": "the node itself as a node value (reads as its text): for `walk`, `report at`, and `.x` reads later (`right.node`, `parent.node`)",
    ScopeDeclaration = "scope_declaration": "on a name a `resolves` item looks up: the name node that declares it (through `scope` and `declares` items); null when nothing does",
    ScopeReferences = "scope_references": "on a declared name: the names resolving to it, in order",
    ScopeEffect = "scope_effect": "the effect an `unwraps <effect> <field>` item has on this node (`assigned`, `mutated`, ...), reached through `targets`; null when none",
    ScopeEffectSite = "scope_effect_site": "the node whose `unwraps` item has that effect (the assignment, the update); null when none",
}

impl Builtin {
    pub fn named(name: &str) -> Option<Builtin> {
        Builtin::ALL.iter().copied().find(|b| b.name() == name)
    }
}

impl<'a> Ctx<'a> {
    pub(crate) fn builtin(&self, b: Builtin) -> Result<Value<'a>, String> {
        let start = self.tree.node(self.node).start as usize;
        Ok(match b {
            Builtin::Environment => self.language_options.map_or_else(
                || Value::record(vec![("settings".into(), Value::list(Vec::new()))]),
                |options| options.environment(self.lang, std::path::Path::new(self.file)),
            ),
            Builtin::Text => Value::from(self.node_text(self.node)),
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
                    !self.source[inside..line_end.max(inside)].trim().is_empty()
                })
            })),
            Builtin::ParentLine => match self.parent_layer() {
                Some(id) if !self.tree.is_root(id) => self.whole_line(self.lines.line_of(self.tree.node(id).start as usize)),
                _ => Value::from(""),
            },
            Builtin::Index => self.tree.field_index(self.node).map_or(Value::Null, Value::from),
            Builtin::GapBefore => {
                let prev = self.tree.parent(self.node).and_then(|p| {
                    self.tree.node(p).children.iter().map(|e| e.node).filter(|&c| self.tree.node(c).end as usize <= start && c != self.node).max_by_key(|&c| self.tree.node(c).end)
                });
                match prev {
                    Some(c) => Value::from(&self.source[self.tree.node(c).end as usize..start]),
                    None => Value::Null,
                }
            }
            Builtin::Line => Value::from(self.lines.line_of(start) + 1),
            Builtin::Column => Value::from(self.lines.line_col(self.source, start).1 + 1),
            Builtin::Offset => Value::from(start),
            Builtin::EndOffset => Value::from(self.tree.node(self.node).end as usize),
            Builtin::File => Value::from(self.file),
            Builtin::Before => {
                let line_start = self.lines.line_start(self.lines.line_of(start));
                Value::from(&self.source[line_start..start])
            }
            Builtin::After => {
                let end = self.tree.node(self.node).end as usize;
                // A node ending in (or inside) a line break has nothing
                // after it on its line.
                let line_end = self.lines.line_end(self.source, self.lines.line_of(end));
                if end > start && self.source.as_bytes()[end - 1] == b'\n' || end > line_end {
                    Value::from("")
                } else {
                    Value::from(&self.source[end..line_end])
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
                Some(id) => Value::Node(id, value::Text::Borrowed(self.node_text(id))),
                None => Value::Null,
            },
            Builtin::TextBefore => Value::from(&self.source[..start]),
            Builtin::TextAfter => Value::from(&self.source[self.tree.node(self.node).end as usize..]),
            Builtin::PreviousSibling => match self.prev_sibling() {
                Some(id) => Value::from(self.node_text(id)),
                None => Value::Null,
            },
            Builtin::PreviousGap => match self.prev_sibling() {
                Some(id) => Value::from(&self.source[self.tree.node(id).end as usize..start]),
                None => Value::Null,
            },
            Builtin::ParentText => match self.parent_layer() {
                Some(id) => Value::from(self.node_text(id)),
                None => Value::Null,
            },
            Builtin::ParentBefore => match self.parent_layer() {
                Some(id) => Value::from(&self.source[self.tree.node(id).start as usize..start]),
                None => Value::Null,
            },
            Builtin::Source => Value::from(self.source),
            Builtin::Node => Value::Node(self.node, value::Text::Borrowed(self.node_text(self.node))),
            Builtin::ScopeDeclaration => match self.names().declaration.get(&self.node) {
                Some(&id) => Value::Node(id, value::Text::Borrowed(self.node_text(id))),
                None => Value::Null,
            },
            Builtin::ScopeReferences => Value::list(
                self.names()
                    .references
                    .get(&self.node)
                    .map(|refs| refs.iter().map(|&id| Value::Node(id, value::Text::Borrowed(self.node_text(id)))).collect())
                    .unwrap_or_default(),
            ),
            Builtin::ScopeEffect => match self.names().effects.get(&self.node) {
                Some(&(effect, _)) => Value::from(self.grammar.names.vocabulary.effects[effect as usize].as_str()),
                None => Value::Null,
            },
            Builtin::ScopeEffectSite => match self.names().effects.get(&self.node) {
                Some(&(_, site)) => Value::Node(site, value::Text::Borrowed(self.node_text(site))),
                None => Value::Null,
            },
        })
    }

    /// This tree's names, resolved on first use and given to its native
    /// front end.
    pub(crate) fn names(&self) -> &'a crate::grammar::names::Resolved {
        assert!(!self.memo.resolving_names.get(), "name conditions cannot read resolved names");
        let mut fresh = false;
        let cached = self.memo.names.get_or_init(|| {
            fresh = true;
            let resolve = || {
                self.memo.resolving_names.set(true);
                let names = &self.grammar.names;
                let session = crate::vm::Session::new(self, self.node, &[]);
                let holds = |node: u32, e: &crate::vm::VExpr| match session.holds_on(node, e) {
                    Some(held) => held,
                    None => { self.memo.vm.clear_error(); false }
                };
                let when = |node: u32, condition: u32| {
                    let Some(e) = names.conditions[condition as usize].lowered.get() else { return false };
                    match self.profile {
                        Some(profile) => profile.time_name_condition(|| holds(node, e)),
                        None => holds(node, e),
                    }
                };
                let key = |text: &'a str| match self.native {
                    Some(native) => native.name_key(text),
                    None => std::borrow::Cow::Borrowed(text),
                };
                let operand = |node: u32, id: u32, item: Option<u32>| {
                    let e = names.operands.get(id as usize).and_then(|operand| operand.lowered.get())
                        .ok_or_else(|| Box::<str>::from("item expression is not compiled"))?;
                    session.value_on(node, e, item).ok_or_else(|| session.error().into())
                };
                let resolve = || names.resolve(self.tree, self.source, &key, &when, &operand);
                let resolved = std::sync::Arc::new(match self.profile {
                    Some(profile) => profile.time_names(resolve),
                    None => resolve(),
                });
                self.memo.resolving_names.set(false);
                resolved
            };
            match self.world.and_then(|world| world.name_cache(self.file_id)) {
                Some(cache) => cache.get_or_init(|| crate::engine::reads::Cached::compute(resolve)).clone(),
                None => crate::engine::reads::Cached::compute(resolve),
            }
        });
        let resolved = cached.as_ref();
        if fresh {
            if let Some(native) = self.native {
                native.set_names(resolved.clone());
            }
        }
        resolved
    }

    /// The tree's native facts, given the tree's names first (front ends
    /// answer name questions from them). While names are being resolved,
    /// as-is: name conditions read only name-free facts.
    pub(crate) fn native_facts(&self) -> Option<&'a (dyn crate::engine::frontend::Native + 'a)> {
        let native = self.native?;
        if !self.memo.resolving_names.get() {
            self.names();
        }
        Some(native)
    }
}
