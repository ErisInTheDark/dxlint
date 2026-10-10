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
    Index = "index": "the node's position among its parent's children in the same field, from 0 (`ts_call.arg` 2nd argument: 1); null for the root",
    GapBefore = "gap_before": "the text between the previous child of the node's parent (of any layer or field) and the node: blank lines, comments, separators; null for the first child",
    Node = "node": "the node itself as a node value (reads as its text): for `walk`, `report at`, and `.x` reads later (`right.node`, `parent.node`)",
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
                        Some(by) => Ctx::indent_eval(&Ctx { node, ..*self }, by, self.indents.lowered_by(i))?.num()?,
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
        })
    }
}
