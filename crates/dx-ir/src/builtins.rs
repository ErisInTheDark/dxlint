//! Variables every rule can use (`text`, `line`, `parent`, ...): their names and docs.

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
    IndentHangs = "indent_hangs": "that `indent in X` inside starts on its opener line (`f(a,? b)`)",
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
