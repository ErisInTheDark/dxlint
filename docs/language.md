# The `.zlint` language

A `.zlint` file describes **layers** (what the text is made of) and **rules**
(what the text should look like). zlint parses every file into a tree of
layers using only fast regex-style matching, then runs rules on the nodes
they check. A project's config is a file named `.zlint`; zlint uses the
nearest one above the files it lints.

```zlint
import .rs                       # Rust's layers, and its built-in rules (all off)

rule rs-indent                   # turn a built-in rule on

# ...or write your own:
in rs_code match rs_function /\bfn[ \t]+/ name:identifier
in rs_function.name rule fn-snake-case warn "function names are snake_case" fix as snake_case(text)
```

- [Items](#items)
- [Context blocks](#context-blocks)
- [Layers](#layers)
- [Patterns](#patterns)
- [Rules](#rules)
- [Expressions](#expressions)
- [Debugging rules](#debugging-rules)
- [Attributes](#attributes)
- [Native layers (TypeScript)](#native-layers-typescript)
- [Turning rules on and configuring them](#turning-rules-on-and-configuring-them)
- [Changing imported definitions: `override`, `drop`](#changing-imported-definitions-override-drop)
- [Imports and macros](#imports-and-macros)
- [Suppressing diagnostics](#suppressing-diagnostics)
- [Editor and tooling](#editor-and-tooling)
- [Performance and caching](#performance-and-caching)

## Items

| item | meaning |
|---|---|
| `lang rust "*.rs" "*.rs.in"` | a language: the root layer `.rust` for files matching the globs |
| `match name pattern` | a matcher (layer) that is only used where something references it |
| `in A, B match name pattern` | a matcher searched inside layers `A` and `B` |
| `in A match name` | attach an existing matcher to more parents |
| `match name like base pattern` | a specialisation of `base`, see [Layers](#layers) |
| `in A layer L match name pattern` | `name` goes in the named layer `L`: a tree of its own over the same text, searched across `A`'s text without consuming it, see [Layers](#layers) |
| `group name A, B` | add layers to a named group, usable wherever a layer list is |
| `indent [in] A, B.field [by n]` | rule clause: the indentation structure that rule measures, see [Layers](#layers) |
| `in A rule name ...` | define a rule checking `A`'s nodes (and turn it on), see [Rules](#rules) |
| `rule name [severity ["message"]] [set value] [{ ... }]` | turn an existing rule on, off, or configure it, see [Turning rules on](#turning-rules-on-and-configuring-them) |
| `define { items }` | items inside define; their rules stay off until turned on |
| `in A let name expr` | an attribute computed for `A`'s nodes, see [Attributes](#attributes) |
| `in A forward x, y from field` | `A`'s nodes pass attributes `x` and `y` up from a field, see [Attributes](#attributes) |
| `native ts` | TypeScript/JavaScript syntax as `ts_*` layers, see [Native layers](#native-layers-typescript) |
| `override <definition>`, `drop ...` | replace or remove what an import defined, see [Changing imported definitions](#changing-imported-definitions-override-drop) |
| `ignore "glob" "glob"` | never lint matching files |
| `import .ts`, `import rules:recommended`, `import some/path` | a built-in language, a built-in ruleset, or another `.zlint` file, see [Imports](#imports-and-macros) |
| `@macro name tokens`, `@macro name(args) { lines }` | define a macro; call it with `@name` |

`#` starts a comment, and `#` lines right above a definition are its
documentation (shown on hover). Items end at the end of the line, or at a
`}` that closes their block.

Layer lists are separated by `,` (`in a, b`); a list may continue on the next
line after a `,`. Glob lists are strings side by side (`"*.rs" "*.rs.in"`).
Globs without a `/` match any single path component (`*.rs`, `target`), and
globs with a `/` match the path relative to the config file (`src/**/*.rs`,
`tests/fixtures/**`).

Names are whole words (`ts_expression_statement`, `previous_line`), except
for language prefixes (`ts_`, `rs_`, `js_`, `py_`) and the ESTree field names
native layers keep (`init`, `quasi`, `consequent`).

## Context blocks

`in A`, `in A layer L`, `match x ...` and `lang l ...` set a **context**: the layers
the rest of the line is about. A `{` at the end of such a line opens a block,
and every line inside it is in that context:

| head | lines inside mean |
|---|---|
| `in A, B {` | `in A, B <line>` |
| `in A layer L {` | `in A layer L <line>`: matchers go in layer `L` |
| `match x ... {` | `in x <line>` |
| `lang l ... {` | `in .l <line>` |
| `rule r ... {` | rule clauses, see [Rules](#rules) |

In a context, `match` defines or attaches children and `rule` checks the
context's layers themselves. Blocks nest, so a layer, its children and its
rules can live together:

```zlint
lang rust "*.rs" {
  match braces /\{/ until /\}/ {
    match braces                                 # braces nest
    match rs_string /"/ until /"/ {
      match rs_escape /\\./
      rule no-tabs warn "tab in a string" { when contains(text, "\t") }
    }
  }
}
```

An explicit `in ...` line inside a block replaces the block's context.

## Layers

Every file is parsed into a tree:

```
.rust                      the language root: the whole file
├── rs_line_comment        children found in the root
├── braces
│   ├── rs_string          children found inside braces
│   │   └── rs_escape
│   └── braces
└── rs_function
    └── name: identifier   fields: labelled parts of a match
```

**Parents.** `in A, B match x` makes `x` a child of `A` and `B`. A parent is
a matcher name, a group, `.lang` (a language root) or `.*` (every language
root).

**Searching.** A layer looks for its children across its text. Anything that
no child matches is skipped, so you never have to describe a whole language,
only the parts you care about. A language root does this over the whole file.

**`until` (skip).** Inside a pattern, `until` lazily skips text until the rest
of the pattern can match, and while skipping, the layer's children eat whole
chunks:

```zlint
match parentheses /\(/ until /\)/
in parentheses match parentheses             # nested parentheses are eaten whole
in .rust, parentheses match rs_string /"/ until /"/
in rs_string match rs_escape /\\./           # so `\"` never ends a string
```

`f(")", g(x))` is one `parentheses` node: the `")"` is eaten by `rs_string`,
and `(x)` by the nested `parentheses`, so the outer `\)` only matches the real
closing bracket. This is how brackets, quotes and escapes work without
special support. They are just layers.

A matcher without `until` searches for its children across its own text
after it matches, skipping its fields, the same way a language root does.

**Ties.** When several children match at the same position, the longest match
wins, and among equal lengths a specialisation beats its base, then the one
attached first wins.

**Specialisation (`like`).** `match js_object like braces ...` is a kind of
`braces`: it keeps every child and named-layer child of `braces`, wins ties against it,
and counts as `braces` for rules (`in braces rule` sees it; `is obj` tells
the specialisation), for `is` tests and for ancestor lookups (`braces`
in an expression finds the closest one). Use it to tell kinds of one shape
apart without re-describing them:

```zlint
match js_object like braces /(?<=[=(,]\s*)\{/ until /\}/
in js_code match js_object {
  rule object-spacing warn "..." fix as ...      # plain braces are untouched
}
```

**Groups.** `group js_code .js, parentheses, braces` names a set of layers.
Groups are **open**: every further `group js_code ...` line adds to it, from
anywhere (another file, an importer), and they are resolved after everything
is loaded. So anything aimed at a group, even written earlier, reaches layers
added later. That makes groups the extension point of a library:

```zlint
# .ts (built in)
group js_code .js, parentheses, brackets, braces
in js_code match js_string /"/ until /"/        # strings inside every code layer
# your config, after importing it
group js_code js_type_arguments                 # ...and now inside these too
```

A macro listing the same layers would be pasted where it is used, so a later
addition would not reach earlier uses.

`group name` declares an empty extension point. Language adapters can add members later. A rule declared in `define { ... }` over an empty group stays dormant, even with `rule *`; a user-defined rule with no targets is an error.

**Language gating.** A matcher attached to a `.lang` only runs inside files
of the languages it is attached to (or attached through `.*`). That lets
shared layers like `parentheses` carry Rust strings in Rust files and
JavaScript strings in JavaScript files.

**Named layers.** A file's nodes form trees over its text: the default
layer, where every plain `match` goes (and a native front end's nodes), and
any named layers. Inside one layer nodes tile the text; different layers
overlap freely. `in A layer L match x ...` puts `x` in layer `L`: it is
searched across `A`'s text without taking it from `A`'s children, and sits
in `L`'s tree, whose root is the file. Use named layers for what cuts across
syntax, like lines:

```zlint
in .* layer text match trailing_whitespace /[ \t]+$/
in .rust layer lines match line /^.*$/
```

`parent`, `previous`, `children` and `descendants` stay within a node's own
layer; other layers are read by position: `containing(kind)` is the
innermost node of that kind (any layer) holding the node, and
`overlapping(kind)` the nodes sharing its span. All three also take
`kind.field`, selecting what fills that field (text too):
`containing(ts_template.quasi)` is inside template text, not merely inside a
template.

**Indentation.** A rule that measures indentation declares its structure
with `indent` lines; other rules never see it.

- `indent in X [by n]`: X's inside (the text its `until` skips; for a node
  without one, its text after its first line) sits `n` levels (default 1)
  past the line X starts on. The line where the inside ends (`}`) sits at
  that line.
- `indent X [by n]`: X itself, when it starts its own line below the line its
  owner starts on, sits `n` levels past that line (`indent ts_switch.case by
  1`: cases one level past `switch`).
- `from <offset>` on either form measures from the line of that byte offset (or node), read on X: `indent in ts_function.body from parent.offset`. A text measures from that text itself (`from ""`: column 0); null keeps the default.
- `class name`: levels opened on one line collapse to one within a class (all unnamed lines share one); lines in different classes add up. An anchor in the middle of a line sits where the indent lines opened earlier on that line put it: `indent in ts_variable_declaration class declarators` indents a `var foo = {` object two levels past `var`.
- `indent reset <expr>`: one predicate per rule. A matching node starting at a line's first token keeps that line's physical indentation with zero levels, before enclosing candidates are considered. Otherwise, the predicate reads the winning indentation candidate's node after ordinary priority and anchor applicability. Virtual anchors also keep their own line's physical indentation when reset. An unmarked winning inner candidate still applies normally; reset never searches beyond it for a marked ancestor. The predicate may use attributes and options, but not rule locals or implicit target fields; indentation queries are disabled while it runs.

An overridable attribute makes reset a config extension point:

```zlint
in ts_node let indent_reset false
in ts_line rule indentation {
  indent reset indent_reset
  indent in ts_block
  # The rule can now read indent_anchor and indent_level as usual.
}
override in ts_node let indent_reset is ts_block
```

Errors in reset, `from` or `by` expressions fail the rule instead of silently using a default anchor or level.

`dedent X` and `dedent in X` count the levels back from the anchor instead
(`indent_level` goes negative). `X` is a layer, a group or `layer.field`;
`n` is read on X and may use the rule's options (`by SwitchCase`; 0 keeps
the line at its anchor). For a line, the innermost node, walking out from
its first character, that an `indent` line applies to decides; on one node
an applying `in` form beats the node form, then a `layer.field` line beats a
plain one, then the later line. Measuring from lines means `foo(Bar {` adds
one level, not two. Rules read the result through `indented`,
`indent_level`, `indent_anchor`, `indent_closes`, `indent_hangs` and
`indent_anchor_line` (see [Expressions](#expressions)) and decide the unit
and any language-specific continuation lines:

```zlint
in rs_code layer lines match line /^[ \t]*\S/   # each code line, first char included

in line rule indent {
  indent in parentheses, brackets, braces
  let level indent_anchor repeat("    ", indent_level)
  when slice(text, 0, -1) != level
  warn "Expected {len(level)} spaces of indentation."
  fix as level slice(text, -1)
}
```

`fix if cond as expr` (or `fix space_before if cond as ...`) attaches the fix
only when `cond` holds; otherwise the diagnostic is reported without one.

Lines above the node are read as their fixes leave them: when a rule's fix
only changes a line's leading whitespace, `indent_anchor`,
`indent_anchor_line`, `previous_line`, `parent_indent` and `parent_line` see
that line re-indented. So a mis-indented `if x {` is one report, not one per
line inside it, and its block's lines are checked against where it will be.

**Fields.** Labelled pieces of a match become fields:

```zlint
in .rust match call name:identifier arguments:parentheses   # call.name, call.arguments
in .rust match call2 identifier :parentheses                # `:parentheses` is short for `parentheses:parentheses`
```

A label on a matcher reference makes that child node the field. A label on
regex text makes a plain text node. A label that matches several times
(inside a repetition) is a list.

**Restrictions.** A matcher never produces an empty node: a pattern that
*may* match empty (`/[ \t]*/`) simply skips those spots, and one that can
*only* match empty is an error. A matcher may only reach itself again after
consuming text (no left recursion). Both are reported when the config loads.

## Patterns

A pattern is a sequence of items after the matcher's name, running to the end
of the line or to a `{` block. Newlines are allowed inside `( ... )`.

| item | meaning |
|---|---|
| `/regex/` | a regex (the subset below) |
| `"text"` | literal text |
| `name` | another matcher, as a child node |
| `label:item` | label an item as a field |
| `:name` | `name:name` |
| `until` | skip, see [Layers](#layers) |
| `( a b or c )` | grouping and alternation (`or` means "one of", unlike the `,` of layer lists) |
| `item?` `item*` `item+` | repetition, glued to the item; add `?` for lazy (`item*?`) |

### The regex subset

Only constructs that run in linear time are supported, so every matcher runs
in time proportional to the text no matter how it is written.

| syntax | meaning |
|---|---|
| `abc`, `\.`, `\/` | literals (escape `/` inside a regex) |
| `.` | any character except `\n` |
| `[a-z_]`, `[^"\\]`, `[\x00-\x1f]` | classes, with ranges (either end may be escaped), negation and escapes |
| `\d \D \w \W \s \S` | digit, word, space classes (unicode-aware, also inside `[...]`) |
| `^` `$` | start and end of a line (`$` also sits before `\r\n`) |
| `\A` `\z` | start and end of the text |
| `\b` `\B` | word boundary and non-boundary |
| `a\|b`, `(...)`, `(?:...)` | alternation and groups |
| `(?<name>...)` | a labelled field, like `name:` |
| `(?=...)` `(?!...)` `(?<=...)` `(?<!...)` | lookahead and lookbehind |
| `* + ? {n} {n,} {n,m}` | repetition, add `?` for lazy |
| `(?i)` at the start | ASCII case-insensitive |
| `\n \t \r \xHH \u{HHHH}` | escapes |

Lookarounds may hold any regex of this subset (lookbehinds can be unbounded,
`(?<=\bclass\b[^{;]*)\{`), but cannot nest or hold fields. Leading
lookbehinds and lookaheads also speed matching up: they tell zlint which
positions can start a match. Backreferences are not supported.

## Rules

```zlint
in todo_note rule todo-comment info "{text} left in a comment"
in rs_function.name rule fn-snake-case warn "function names are snake_case" fix as snake_case(text)

in line rule line-length {
  option max int(1..) = 100 "what max means"
  error "line is {len(text)} long"
  files "src/**"
  ignore "src/generated/**" "vendor/**"
  when len(text) > max
  fix as slice(text, 0, max - 3) "..."
}
```

A rule is `rule name [severity [message]] [fix as expr] [{ clauses }]` in a
context (`in ...` or a block). That defines it; in your own config it is on
right away, at its severity. Without a context, `rule name ...` turns an
existing rule on or configures it instead (see
[Turning rules on](#turning-rules-on-and-configuring-them)).

**Targets.** A rule checks every node of its context's layers: a matcher (and
its specialisations), a field (`in m.field rule`), a group (every member),
`.lang` (whole files of a language) or `.*` (whole files of every language).
With several targets (`in a, b.field rule`), bare fields used in the body must
exist on all of them. Dotted paths through a known field (`name.offset`,
`name.unparen_offset`) instead read that field on the current node; a missing
field yields `null`.

`in K.link rule` also targets nodes reached through a registered node link
from nodes of matcher `K`. Links can return one node or many:

```zlint
in ts_program.binding_declarations rule declarations info "{text}"
in ts_identifier.declaration rule bindings info "{text}"
```

The body runs on the reached node, not `K`: `text`, attributes, facts,
rule locals, report ranges and fixes belong to that node. Nodes are checked
in document order, once per rule even when several owners or links reach
them. An unresolved link, or a file without the native front end, selects
nothing. Links do not depend on rule options. A real grammar field takes
precedence over a link of the same name; unlike a known field's target,
a link infers no child fields for the body (use explicit node paths).
`binding_declarations` includes merged declarations, named expressions and
identifier enum members, not referencing identifiers.

**Severity and message.** The severity keyword (`off`, `hint`, `info`, `warn`,
`error`) comes first and the message follows it, on the rule's line or as a
body line (`warn "..."`). The message can use everything an expression can
(`"{text} left in a comment"`), or be a macro. Without one the rule is a
`warn` and its message is its name. `zlint rules` shows messages with option
values filled in and other holes left as written.

**Names.** A rule name may be `family:variant` (`semi:never`,
`no-multiple-empty-lines:eof`): several rules that make up one. `rule
semi` turns on and configures `semi` and every `semi:*` variant, `rule
semi:* off` reaches every variant, and suppressing `semi` suppresses the
whole family. A second definition with a name already in use is an error;
use [`override`](#changing-imported-definitions-override-drop) to replace one.

**Fix.** `fix as expr` says what the node *should* be. If its text differs from
the node's text, the rule reports the node and offers the fix. If they are
equal, nothing happens. Fixes can be anything: reorder, rename, delete
(`""`), add text. A rule without a fix reports every node that passes its
`when` conditions.

`fix at span as expr` rewrites explicit UTF-8 bytes: a number inserts at that offset, a `{start, end}` record replaces that half-open range, and a node replaces its span. Offsets must be finite integers within the file, on UTF-8 character boundaries, with `start <= end`; invalid spans report a rule error without a fix. `fix at span if condition as expr` attaches a fix only when the condition holds. Unchanged text still suppresses the diagnostic; the diagnostic's location remains controlled by `report`, not the fix span.

```zlint
fix at {start:comma.offset, end:comma.end_offset} as ""
fix at end_offset as ";"
```

`--format jsonl` retains `fixable` and adds `fix.edits` on fixable diagnostics. Each edit is `{start, end, text}`, using zero-based, half-open UTF-8 byte offsets into the reported file text (not character or UTF-16 offsets). All edits of one diagnostic form one atomic fix; replacements apply to the same original text, not sequentially shifted offsets. Unfixable diagnostics omit `fix`.

`--fix-preview --format jsonl` emits one `{file, kind: "fix-preview", text}` record after each wanted file's original diagnostics, including clean files. The text applies one engine fix pass without relinting, source writes or cache reads/writes. Works with `--sources`; incompatible with `--fix`.

**Body clauses.**

| clause | meaning |
|---|---|
| `off\|hint\|info\|warn\|error [message]` | severity (default `warn`) and message |
| `when expr` | only nodes where `expr` is truthy; several `when`s must all hold, in order |
| `let name expr` | a value computed once per node and usable by later clauses, the message and the fix |
| `fix as expr` | what the node should be |
| `fix at span [if condition] as expr` | replace explicit UTF-8 bytes; a numeric span inserts text |
| `option name <type> = <default> "description"` | a configurable value of that type, see [Option types](#option-types) |
| `files "glob" ...` | only these files |
| `ignore "glob" ...` | not these files |
| `report node\|change\|end` | where diagnostics point: the whole node (default), only the part the fix changes, or the node's end |
| `report at expr` | a byte offset selects a zero-width point; a `{start, end}` record or node selects its span. Without a report clause, diagnostics cover the matched node. Report positions do not change fix regions |
| `@each name in list` | one diagnostic per item of a list from the rule's options (`@each entry in paths`): `when`, the message, `fix` and `report at` see the item as `name` (`let`s stay per node). The list is known when the config loads; rules never loop over code |

A macro at the start of a body line is pasted as clauses.

**Rule `let`s** name an intermediate value, so a long condition reads as
steps. Each is computed at most once per node, only when something reads it,
and can read the `let`s above it:

```zlint
in ts_statement rule padding-line-between-statements {
  when index > 0 && parent is (ts_block, ts_program)
  let wanted is_conditional || previous.wants_blank_after == true
  when wanted && !{ value (gap_before.nonempty ?? ""); re_test /\n[ \t]*\r?\n/ }
  warn "Expected blank line before this statement."
}
```

A rule's `let` can also be read on another node the rule targets:
`previous.rank`, `parent.depth`. That carries state along siblings or down
the tree without loops, and the state sees the rule's options. A `let` may
read itself on other nodes only:

```zlint
in ts_class_member rule member-order warn "out of order" {
  option order string[] = ["field", "method"]
  let rank index_of(order, kind)
  let highest if previous.highest == null then rank else max(previous.highest, rank)
  when previous.highest != null && rank < previous.highest
}
```

**Links** lead to other nodes: `declaration` (of a referencing identifier)
and `references` (of a declaring one). A node reads as its text, `is` and
`not` test its layer, and `.x` reads anything on it: `declaration.line`,
`references.line`.

Fixes are minimal: the common start and end of the node and its fix are left
alone, so a rule on a whole statement that only adds `;` edits one character.
With `report change` the diagnostic points at that edit too.

Fixes that overlap are applied over several passes (re-linting in between),
so nested rules compose.

## Expressions

### Pipelines

Start with `value`, then apply each stage to the preceding value:

```zlint
let names {
  value declarations.nodes
  filter (item, i) i % 2 == 0
  map (item) item.name
  unique
  join ", "
}
```

A stage names any function, with the preceding value as its first argument: `trim`, `slice 0 8`, `replace "old" "new"`. Remaining arguments are whitespace-separated operands; parenthesise arithmetic or other compound expressions. Newlines or `;` separate steps: `{ value before; trim; nonempty } ?? indent`.

`then (value) expression` transforms the **whole** preceding value, rather than each item. `label: stage` saves a step's result for later steps. Label the seed with `value label: expression`. Labels are unique within a pipeline, shadow surrounding names only after their step, and never escape the block. Another `value expression` restarts the chain without discarding labels:

```zlint
let selected {
  value original: declarations.nodes
  filter (item) item.used
  used: count
  value original
  then (items) { items: items, used: used }
}
```

Pipelines are expressions: use them in functions, attributes, rule lets, `when`, conditional arms, lambda bodies, messages and template holes. `{ value: expression }` is still a record; the `value` marker without a following `:` identifies a pipeline.

| stage | result |
|---|---|
| `map (item, i) expr`, `flat_map (item, i) expr` | transformed items; `flat_map` flattens one level |
| `filter (item, i) test` | matching items, in input order |
| `find (item, i) test`, `find_last (item, i) test` | first or last matching item, or `null` |
| `any (item, i) test`, `all (item, i) test` | whether any or all items match; stop once decided |
| `take_while (item, i) test`, `skip_while (item, i) test` | matching prefix, or items after that prefix |
| `min_by (item, i) key`, `max_by (item, i) key` | item with the smallest or largest key; first wins ties; `null` when empty |
| `sort_by (item, i) key` | stable ascending order |
| `unique_by (item, i) key` | first item for each distinct key |
| `group_by (item, i) key` | first-seen groups as `[{ key, items }]` |
| `partition (item, i) test` | `{ yes, no }` lists, each in input order |
| `reduce (acc, item) expr` | first item seeds the accumulator; `null` when empty |
| `fold initial (acc, item) expr` | explicit initial accumulator; evaluates the input before the initial value |
| `zip other`, `zip other (a, b) expr` | pairs or paired transformation, stopping at the shorter input |
| `take n`, `drop n` | first `n` items, or items after them |
| `flat`, `concat other` | flatten one list level, or append another list |
| `reverse`, `unique` | reversed items, or first occurrences |
| `chunk n`, `windows n` | consecutive chunks (last may be shorter), or overlapping full windows; `n` must be positive |
| `count`, `sum`, `join separator` | item count, numeric total, or joined text |
| `nonempty` | `null` for `""`, `[]` or `{}`; otherwise unchanged |

Item stages may omit the second parameter. `i` is the original input index, including in `find_last`; no intermediate tuple or enumeration step is needed. Lambdas see surrounding values and pipeline labels.

**Fallbacks.** `a ?? b` evaluates `a` once and evaluates `b` only when `a` is `null`. Unlike truthiness, it preserves `false`, `0`, `""`, `[]` and `{}`. Use `a.nonempty ?? b` when empty text, lists or records should also fall back. `.nonempty` and `.nodes` are reserved suffixes; read record keys with those names using `get(record, "nonempty")` or `get(record, "nodes")`. Grammar fields cannot use these names.

**Calls.** A call's arguments must be simple: names, literals, dot reads, operators over those, or a stage lambda. Do not nest calls within arguments; name an intermediate value with `let` or use a pipeline. `count(params) > max` and `"{count(all)}"` remain valid. Calls inside a lambda body follow the same rule independently.

| syntax | meaning |
|---|---|
| `"text {expr} more"` | template string; `{{` and `}}` for literal braces |
| `'text'` | single-quoted string (handy inside `{...}`) |
| `"""` ... `"""` | multi-line string, see below |
| `42`, `1.5`, `7e-8`, `true`, `false`, `null` | literals |
| `[a, b]` | list |
| `{ key: value, ... }` | record (keys are names or strings) |
| `name` | a field of the node, an option, a `let`, or a variable |
| `option.key`, `x.key`, `f(...).key` | a record's value for `key` (`null` when missing) |
| `map(xs, x => ...)` | a lambda, for `map`, `filter`, `find`, `find_last`, `all` and `any` |
| `fold(xs, init, (state, x) => ...)`, `reduce(xs, (state, x) => ...)` | accumulation with an explicit seed, or the first item as seed |
| `matcher` / `matcher.x` | the closest enclosing node of `matcher`, or anything read on it (`ts_function.name`, `ts_class.offset`, `ts_function.my_attr`) |
| `parent.x` | anything read on the layer the node sits in (`parent.offset`, `parent.depth`) |
| `field.node`, `field.offset`, `field.attr`, `field.fact`, `field.link` | a builtin, attribute, fact or link read on the node in one of this node's fields (a list for several) |
| `callee.object.x`, `parent.left.x`, `previous.body.x` | longer paths walk: each field label steps into its node(s), the last part is read there |
| `first(references).line` | `.x` on a node value (from a link, `node`, or a lambda parameter) reads on that node; longer paths walk (`n.callee.object.offset`) |
| `node`, `right.node`, `parent.node` | the node itself as a node value (it reads as its text) |
| `right.nodes`, `n.callee.nodes` | the field's node values, always a list, including for zero or one match |
| `overlapping(layer)`, `overlapping(layer, from, to)` | the nodes of a layer or group anywhere in the file whose span overlaps the node's (or bytes `from..to`), in source order: on a `ts_line`, `overlapping(ts_comment)` |
| `descendants(layer)` | the nodes of a layer or group (specialisations too) inside the node, in source order: `descendants(ts_export)` |
| `walk(roots, n => next)` | the nodes reachable from `roots` by `next` (nodes or lists of them), each once, roots first: `walk(references.nodes, n => n.writer.right.node)` |
| `f(a, b)` | function call |
| `a b` | join: values side by side join as text (`text "="`), or as lists if either is one (`[a] [b]`) |
| `!a` `-a` `a * b` `a % b` `a + b` `a - b` | arithmetic (`text * n` repeats text) |
| `== != < <= > >=` | comparisons (numbers compare numerically, even as text) |
| `a && b`, `a \|\| b` | logic, short-circuiting |
| `a ?? b` | lazy fallback for `null` only |
| `value ~ pattern-item` | does the whole value match? (`text ~ /[a-z_]+/`, `text ~ snake`) |
| `is a`, `x is a`, `x is (a, b)`, `x not (a, b)` | is the node (`x` alone: this node; or a field, `parent`, `previous`, a link, a node value) of one of these kinds: itself, a specialisation, or a group member (`not`: none of them) |
| `is a.field` | does the node sit in that field of a node of kind `a` (`is ts_if.alternate`) |
| `is layer name`, `is .lang` | is the node in that named layer; is the file in that language |
| `/regex/` elsewhere | the regex source as text, for the `re_*` functions |
| `if c then a else b` | conditional |

**Spacing.** Binary operators stand apart from both operands: `a - 1`,
`x == 2`, `text ~ /re/`; `a -1` is an error. Unary `-` and `!` stay tight
(`-1`, `!x`). A `-` between name characters belongs to the name
(`no-dbg`, `rule-id`), so `offset-1` is one (unknown) name.

**Joins** bind tighter than comparisons and looser than arithmetic:
`len(x) + 1 "px"` is `"4px"`, and `a == "x" "y"` compares with `"xy"`. A join
runs to the end of the line (inside brackets, across lines) or to `then`,
`else`, `fix` or a `{`. Numbers join as their text, `null` as nothing. `+`
only adds numbers.

**Long conditions.** A line that starts with `&&`, `||`, a comparison or `is`
continues the expression above it, as does a line after a trailing operator
or inside brackets:

```zlint
let wants_blank_after is_conditional
  || is (ts_break, ts_continue, ts_throw)
  || spans_lines && declares != null
```

**`is`** tests what a node is; kinds are never compared as text. `is
ts_loop` (no subject: this node) holds for every member of the `ts_loop`
group, and `parent is braces` for any specialisation of `braces`. Several
go in parentheses, a group in place: `parent is (ts_block, ts_program)`;
`is ts_if.test` means "in the `test` field of a `ts_if`"; `is layer lines`
tests the layer and `is .rust` the file's language. `not` is the inverse:
`next_token not (ts_closer, ts_comma)`. A value that is not a node is never
any kind. Unknown names are config errors. `kind_name(n)` gives a node's
kind as text, for messages.

**Multi-line strings** start with `"""` at the end of a line and end with
`"""` on a line of its own. That closing line's indentation is removed from
every line, and the line break before it is not part of the string. A line
ending in `\` continues on the next without a line break. Escapes and
`{holes}` work as in `"..."`, and `"` needs no escaping:

```zlint
rule header:* error {
  set license """
    /*!
     * Copyright 2025 Someone. Modification and/or \
    distribution is prohibited.
     */
    """
}
```

**Values** are `null`, booleans, numbers, text, lists and records. A field
that did not match is `null`, and a field that matched several times is a
list. `null`, `false`, `0`, `""`, `[]` and `{}` are falsy.

**Lambdas** walk lists, usually lists of records from an option: `map(xs, x
=> value)`, `filter(xs, x => test)`, `find(xs, x => test)` (first match or
`null`), `find_last`, `all(xs, x => test)` and `any(xs, x => test)`.
`fold(xs, init, (state, x) => next)` carries a value through the list, for
decisions that depend on earlier ones. `keys`, `values`, `get(record, key)`
and `len` read records. A lambda sees everything the expression around it
sees, plus its parameters:

```zlint
let wanted find_last(config, c => matches(c.prev, previous) && matches(c.next, node)).blankLine
```

**Variables.**

| name | value |
|---|---|
| `text` | the node's text |
| `node` | the node itself, as a node value |
| `indent` | leading whitespace of the node's first line |
| `line`, `column` | 1-based position of the node |
| `file` | path relative to the config |
| `before`, `after` | the rest of the node's first line before it, and of its last line after it |
| `previous_line`, `next_line` | the whole lines around the node (`null` at the file's edges); `previous_line` as its indentation fix leaves it |
| `text_before`, `text_after`, `source` | everything before the node, after it, and the whole file |
| `parent`, `parent_text` | the node this one sits in (in its layer; the file for top nodes), and its text |
| `parent_before` | the parent's text up to the node ("what came earlier in this statement") |
| `parent_indent`, `parent_line` | leading whitespace and whole text of the line the parent starts on |
| `previous_sibling`, `previous_gap` | the previous node of the same layer in the same parent, and the text between |
| `gap_before` | the text between the previous child of the parent (of any layer) and the node: blank lines, comments, separators; `null` for the first child |
| `index` | the node's position among the parent's children in the same field, from 0 (the second `ts_call.argument` is `1`) |
| `indented` | one of the rule's `indent` lines applies to the node's line |
| `indent_level` | levels that line gives it past its anchor (`by`, default 1; 0 on a closing line or when not indented) |
| `indent_anchor`, `indent_anchor_line` | leading whitespace and text of the line it is measured from (`indent in X`: X's line; `indent X`: X's owner's line) |
| `indent_closes` | this line starts with the end of an `indent in X` inside, so it sits at the anchor |
| `indent_hangs` | that inside starts on its opener line (`f(a,⏎ b)`) |

**Neighbours.** `children`, `previous` and `next` reach other nodes:

| name | value |
|---|---|
| `children.x` | `x` of every direct child, in order, whatever its field (`null`s kept, so `last(children.x)` is the last child's) |
| `previous.x`, `next.x` | `x` of the previous or next node in the same field of the same parent (`null` at the edges) |
| `children`, `previous`, `next` alone | their text |

`x` can be a field, an attribute, a fact or a builtin (`previous.offset`); test
the neighbour itself with `is` (`previous is ts_comma`).

**Functions.** `zlint functions` lists them all. Highlights: `len`, `lower`,
`upper`, `snake_case`, `camel_case`, `pascal_case`, `kebab_case`,
`screaming_case`, `title_case`, `trim`, `trim_start`, `trim_end`, `replace`,
`repeat`, `pad_start`, `pad_end`, `starts_with`, `ends_with`, `contains`,
`split`, `lines`, `join`, `sort`, `unique`, `reverse`, `first`, `last`,
`compact`, `sum`, `any`,
`count`, `slice`, `str`, `num`, `nonempty`, `min`, `max`, `abs`, `round`,
`compare_text`, `js_compare`, `js_number`, `js_re_test`.

| function | value |
|---|---|
| `js_number(value)` | JavaScript Number conversion of primitives or `literal_value` records; NaN for nonnumeric strings, null for unsupported values (including symbols) |
| `js_re_test(text, pattern, flags?)` | stateless JavaScript regex matching (`flags` defaults to empty); unsupported syntax and work exhaustion fail the rule. `re_test` keeps its existing regex dialect |

`js_re_test` uses a bounded JavaScript subset with `i`, `m`, `s`, `u` flags and flat positive/negative lookaheads. Nested assertions, lookbehind, backreferences, named groups, control escapes and Unicode case-insensitive word boundaries fail explicitly. Multiline anchors recognise LF, CR, U+2028 and U+2029, including positions inside CRLF. Legacy `i` supports ordinary Unicode folding; characters in fold groups that differ from JavaScript's non-`u` uppercase canonicalisation (such as Kelvin sign, long s and astral case pairs) fail explicitly in patterns or values. Use `u` for those groups. Strings cannot preserve lone surrogates, and non-`u` astral matching uses Unicode scalars rather than UTF-16 units.

Regex sources are limited to 64 KiB and 128 nested groups; assertion patterns allow 64 assertions. Assertion/multiline execution allows 50,000 states per compiled program and 10 million state visits per match, shared with its assertion checks. Exhaustion produces a rule error, never a silent non-match.

```zlint
group restricted_syntax ts_with
rule no-restricted-syntax
in ts_call rule no-eval-call {
  when callee.text == "eval"
  warn "Avoid eval."
}
```

`restricted_syntax` is an empty, open group: add syntax to prohibit it. Contextual exclusions and custom messages belong in ordinary rules, not selector options.

`no-extra-parens` checks expressions in the open `no_extra_parens_inner` group (default: `ts_node`). The `no_extra_parens_checked` attribute on `structure_parens` defaults to true; override it to select reported parentheses by their inner expression or owner. `no_extra_parens_spread_allowed` starts empty and exempts its members only as spread operands:

```zlint
drop group no_extra_parens_inner ts_node
group no_extra_parens_inner ts_function, ts_arrow, structure_assertion
group no_extra_parens_spread_allowed ts_conditional
```

`slice` counts characters; offsets (`offset`, `end_offset`, native spans)
count bytes, so cut source text at them with `byte_slice(text, from, to?)`.

`byte_char(text, offset, before?)` reads one character at a UTF-8 byte boundary, or the preceding character when `before` is true. Returns `""` at file edges. Offsets must be finite non-negative integers within text, on character boundaries; invalid offsets are errors.

`position(offset)` gives `{line, column}` of an offset, like the `line` and
`column` builtins (`position(end_offset).line`), and `{js_line, js_column}`
as JavaScript tools count them: U+2028 and U+2029 break lines, columns
count UTF-16 units; `js_line_start` is the byte offset that line starts at.

Regex functions take the full syntax of Rust's `regex` crate (no lookaround
or backreferences there): `re_test(text, re)`, `re_find(text, re)` (first
match or `null`), `re_count(text, re)`, `re_replace(text, re, with)` (`$1`
for groups) and `re_escape(text)` to build patterns from text.

## Debugging rules

- **`zlint describe <name>`** explains any name in the config: a rule (final
  severity or that it is off, options with their values, targets, where it
  is defined and configured), a layer (pattern, parents, fields, rules and attributes on it), a
  group (its final members), an attribute, macro, builtin, fact, function or
  keyword, with every place it is used. Misspelt names get a suggestion.
- **`dbg(value, "label")`** returns `value` unchanged and prints
  `file:line:col layer label = value` to stderr for every node it is
  evaluated on. Wrap any part of a `when`, `let` or message to see what it
  computes.
- **`zlint eval <target> <expr> files...`** prints what an expression gives on
  every node of a target, with the project's config: attributes, facts,
  macros and options resolve as in a rule.

  ```text
  $ zlint eval ts_call.callee 'resolved' src/a.ts
  src/a.ts:3:1 setTimeout => global
  src/a.ts:4:5 this.load => null
  ```
- **`zlint tree file --at LINE:COL`** explains one position: the layers
  covering it, then what every child matcher of the innermost one does there
  (rules out the character, is ruled out by its prefilter, does not match,
  matches to where, or runs past its layer), marking the one in the tree.
  When a span went to the wrong layer (or to none), this names the matcher
  that took it.

`-c config` works before or after any subcommand.

## Attributes

`in A let name expr` computes a value for every node of `A`, on demand and
once per node. Attributes read their own node like a rule does, and other
nodes' attributes through fields and neighbours, so they can fold a tree
bottom-up:

```zlint
import .ts
in ts_logical let operators 1 + sum(children.operators)
in ts_parenthesized forward operators from expression
in ts_call let operators sum(argument.operators)
in ts_if.test rule too-complex warn "{operators} operators" { when operators > 2 }
```

- `name` is the node's attribute; `field.name` is the attribute of that
  field's node (a list when the field repeats). Layers without a definition
  give `null`.
- `parent.field` and `parent.name` read the parent layer's field or
  attribute (`in ts_call.argument rule ... { when parent.callee == "f" }`).
- `children.name`, `previous.name` and `next.name` read neighbours (see
  [Expressions](#expressions)).
- Native facts read the same way: `callee.resolved` is the fact of the
  `callee` field's node, `parent.type_flags` the parent's.
- `[] field.name` is always a list, whether the field matched none, one or
  several nodes.
- An attribute that depends on itself fails like any expression error.
- Attributes also lift builtins onto fields: `in ts_identifier let used uses`
  makes `local.used` readable from an import specifier.

**Forwarding.** `in ts_parenthesized forward operators, call_depth from
expression` makes wrapper nodes transparent: when a `ts_parenthesized` node
has no `operators` of its own, it gives its `expression`'s. Only the listed
attributes pass through, so other rulesets' attributes on the same layer are
unaffected. A field that repeats gives the list of its nodes' values.

A value one rule needs is better as a rule [`let`](#rules) than as an
attribute: it stays next to its rule.

## Native layers (TypeScript)

`import .ts` (or `native ts` with your own JS language) parses
`.ts`/`.tsx`/`.js`/`.mjs`/`.cjs` files with zlint's own TypeScript parser and
turns its syntax tree into `ts_*` layers: `ts_if` with fields `test`,
`consequent`, `alternate`; `ts_call` with `callee`, `type_argument`,
`argument`; `ts_binary`/`ts_logical` with `left`, `operator`, `right`, and so
on (`zlint tree file.ts` shows any file). Rules on them work like any rule:

```zlint
import .ts
in ts_call rule my-no-eval warn "no eval" { when callee == "eval" }
in ts_if.test, ts_while.test rule assign-in-condition warn "assignment in a condition" {
  when is ts_assignment
}
```

A file the parser cannot read gets one warning instead of rules on `ts_*`
layers.

**One tree.** A file the parser reads gets one tree: the parser's nodes, and
the pattern layers inside them, by the rules pattern trees follow: a pattern
layer inside a native layer fills the gaps between that node's other
children and sits under it, a named layer's matcher searches its whole text; the language's own
pattern layers (`in .js match ...`) are matched across the file and sit
under the program. Rules see no difference:

```zlint
in ts_comment match todo /\bTODO\b/
in ts_string, ts_template match escape /\\./
in todo rule no-todo warn "Unexpected TODO."
```

**Tokens.** Punctuation and keywords that no child node covers are layers
too, under the node that owns them (field `token`): `ts_comma`,
`ts_semicolon`, `ts_colon`, `ts_dot`, `ts_question`, `ts_ellipsis`,
`ts_equals`, the brackets `ts_open_paren` ... `ts_close_angle` (groups
`opener` and `closer`), `ts_keyword` and `ts_punctuator`; group `ts_token`
has them all. `opener.x` and `closer.x` read a node's own first opening and
last closing bracket; `opener.ts_open_brace.x` only one of that layer (a
class's first opener is the `<` of `class A<T>`). Every native node knows `space_before` and
`space_after` (the gap to the neighbouring token) and the layers of
`previous_token` and `next_token`; `fix space_after as " "` rewrites just
that whitespace. Gap `blank_lines` counts physical blank lines; `blank_gaps` counts blank lines between tokens/comments, excluding comment interiors. Spacing rules are one decision per token:

```zlint
in ts_comma rule comma-spacing:after warn "A space is required after ','." {
  when next_token not (closer, ts_comma) && !space_after.line_break && !space_after.spaced
  fix space_after as " "
}
in ts_block rule padded-blocks:top warn "Block must not be padded by blank lines." {
  when opener.space_after.blank_lines > 0
}
```

Files get token nodes only when an active rule needs them. A family name
turns on and configures every variant (`comma-spacing:before`,
`comma-spacing:after`): `rule comma-spacing`.

Comments and lines are pattern layers in `std/ts.zlint` (see One tree):
`ts_line_comment` and `ts_block_comment` (group `ts_comment`) are children of
the code node whose text, between its other children, holds them, so never
inside a string, regex or template text; `ts_line` is in the named layer `lines`, over the
file, each JavaScript line with its line break (the last may have none).

Group `ts_node` holds every syntax layer (not tokens): `in ts_node let ...`
gives an attribute to all of them, for walks that may cross any kind.
`ts_code` is the same without literal text (`ts_string`, `ts_regex`,
`ts_jsx_text`).

**JSX tags.** `ts_jsx_element` and `ts_jsx_fragment` own `opening`, `body`
and `closing`. Their `ts_jsx_opening` and `ts_jsx_closing` nodes cover the
whole tags, including punctuation. Opening tags own `name`,
`type_argument`, `property` (attributes) and `token`; closing tags own
`name` and `token`. Fragment tags have no name or attributes; self-closing
elements have no closing tag. Children stay in the element or fragment's
`body`, separate from both tag owners.

Project rules can style opening-tag lines without changing child indentation:

```zlint
import .ts
rule indent {
  set mode 2
  indent in ts_jsx_opening by 3
}
```

```tsx
<Box
      prop="value"
>
  Text
</Box>
```

`override in ts_jsx_opening let indent_reset true` instead keeps opening-tag
lines at their physical indentation. Child text, closing tags and nested
attribute expressions remain checked. This selects every opening tag,
including nested elements; use a predicate such as `name == "Box"` to keep
other opening tags checked. Read tag bounds from `opening.offset`,
`opening.end_offset`, `closing.offset` and `closing.end_offset`, and tag
fields from their tag owner rather than the enclosing element.

**Facts.** Native layers add builtins (null elsewhere): properties of the
code any rule may ask about. What a rule makes of them (its options, what
is wrong, messages) is written in the rule.

| name | value |
|---|---|
| `uses`, `value_uses` | on a declaring identifier: how often its name is used, and how many of those uses need the runtime value (type positions, and `typeof x` or `[x]` keys inside types, do not) |
| `self_uses`, `writes` | uses inside its own function, and assignments to it |
| `declaration_kind` | `const`, `let`, `var`, `parameter`, `type_parameter`, `function`, `class`, `import`, `type`, `interface`, `enum`, `namespace`, `catch`, ... |
| `exported`, `ambient` | declared by an `export`ed statement; inside `declare` |
| `imported_from` | on an identifier that is or refers to an import: the module it comes from |
| `declaration_doc` | the JSDoc of the declaration an identifier refers to, followed through imports, re-exports and `export *` |
| `symbol_metadata` | `{name, own_doc, declarations, aliases, selected_signature}`; raw declaration and alias docs, overload `signature_docs`, declaration kinds and file/lib/package provenance gates. Alias rows retain lookup order, path and module; `selected_signature` is null or `{doc, kind}` (`call`/`construct`). Ambient package gates retain ordinary file provenance. Rust exposes docs and origins; `.zlint` decides deprecation |
| `resolved`, `reference_kind` | on a referencing identifier: `local`, `import` or `global` (declared nowhere in the file), and the `declaration_kind` of what it refers to |
| `type` | the expression's type as text (`Promise<void>`, `never`, `NodeJS.Timeout`), null when unknown |
| `type_flags` | what the type is made of: `any`, `never`, `string`, `number`, `boolean`, `bigint`, `symbol`, `literal`, `enum`, `array`, `tuple`, `promise`, `function`, `object`, `null`, `undefined`, `nullable`, `union`, and `unknown` for parts zlint cannot read |
| `type_literal_values` | literal union/intersection constituents as type/value records; excludes nonliteral constituents; bigint values retain literal text |
| `type_domains` | constrained truthy, falsy, nullish and non-nullish possibilities, flags, opacity and scalar literal; `kind` matches `type_structure`, `parts` retains union/intersection constituent domains |
| `evaluated_type_domains` | the same domains before optional-chain short-circuiting, preserving intrinsic member and return nullability |
| `access_resolution` | native member/index lookup: `from_index` signature origin, `optional` declaration and `evaluated` value domains before optional-chain short-circuiting |
| `callable_return_domains` | domains of every callable return, including local inferred bodies and in-scope parameter bounds |
| `predicate_call` | asserted argument index and `asserts`, argument/target domains, `to_target`/`from_target` assignability and `target_union`; null without a resolved argument |
| `type_has_void` | a union or intersection constituent is void, distinct from undefined |
| `type_iterable` | constrained type supports synchronous iteration; null when unresolved |
| `could_be_error` | expression syntax permits an Error-valued result through a possible value branch; does not require type analysis |
| `error_value` | constrained value category (`error`, `any`, `unknown`, `undefined`, `other`), caught-binding provenance (`rethrown`), and unparenthesised byte `offset` |
| `argument_transfers` | calls, constructions and tagged templates: any/generic argument and spread transfer rows `{offset, mode, error, sender, receiver}`; modes `argument`, `spread`, `array`, `tuple`; `error` marks unresolved provenance, sender/receiver are raw printed types or null |
| `assignment_transfers` | assignments, initialisers, properties and destructuring: any/generic transfer rows `{offset, mode, error, sender, receiver}`; modes `any`, `generic`, `array`, `tuple`, `object`, `spread`; `error` marks unresolved provenance, sender/receiver are raw printed types or null |
| `return_transfer` | returns and expression arrow bodies: transfer record `{mode, error, sender, receiver}` or null; modes `value`, `array`, `promise`, `generic`; `error` marks unresolved provenance, sender/receiver are raw printed types or null |
| `builtin_type(name)`, `builtin_constructor(name)` | standard-library type identity through instance heritage, or constructor value identity through class heritage; names alone do not identify user-defined types |
| `type_members` | member names of that type, inherited ones included (`then`, `[Symbol.dispose]`) |
| `property_info` | on static member or index access: `{declared, visibility}` (`public`, `private`, `protected` or null), preserving declaration presence even when its value type is unresolved |
| `index_key_types` | constrained index-signature key domains as `{text, string_like}` records, inherited domains included |
| `static_number` | statically known numeric value of an expression (numeric, boolean and bigint literals, arithmetic, unmodified local bindings); null when unresolved |
| `type_relations` | operand, annotation, original and context identity, bounded assignability, nested property mutability and generic shape; null when unresolved |
| `type_flow(site, source)` | source/expected type relation records at declared, contextual, argument, return, indexed-key and heritage sites: union domains and enum identities, tri-state assignability, aligned generic/index/property children, typed operands and receiver enum names; call receivers retain declared generic bounds or explicit type arguments, not inference from the checked value; `complete: false` marks traversal limits |
| `type_awaitability` | structural thenable union facts `{any, all, union_equivalent, complete}`; `all` requires every non-never arm to be known thenable, `any` needs one; `union_equivalent` is mutual assignability between awaited and ordinary arms of a known mixed union, or null |
| `callable_contracts(site, source, member_path?)` | callable transfer rows `{origin, source, target, heritage, member_path, complete}`; source/target contain return arms `{kind, thenable}`. Origins are `argument`, `return`, `declared`, `contextual`, `heritage` or `resource`; heritage is a direct base node or null. The optional string-list path selects exactly one member chain; omitted or `[]` selects the root. Argument contracts include every visible overload, not only the selected one; unresolved signatures or work limits mark rows incomplete |
| `type_structure(names, fields?)` | constrained recursive `{kind, parts, names, generic_names, aliases, members}`: array/tuple elements or union/intersection parts; optional tuple slots include undefined. Explicit `unknown`, open `parameter` and `unresolved` are distinct. `names` includes printed base types; `generic_names` contains root generic declaration names. `aliases` contains retained root-annotation alias names, not aliases propagated through inferred values or calls. Queried members contain winning `{owner, kind, signature, key_symbol}` declarations; owner is `{name, kind, from}` or null, and `key_symbol` verifies standard-library Symbol computed-key identity. The second argument selects the recursive fields returned (default: the six above); opt-in fields: `declarations` (`{name, kind, from, inherited}`), `callable`, `constructor` (declared construct signatures or class values), `symbol_constructor` (a named type's constructible value side), `class_value`, `property_names` (without signature markers or apparent function prototype members), `iterable` (null when unresolved) and, on the root, `property_writes` (property names assigned through the binding) |
| `type_parameter_defaults`, `type_parameters_inferred` | on a call, `new`, type reference or class with type arguments: each type parameter's default, and what the arguments infer, printed |
| `type_argument_redundant` | on the last type argument: `default` when it is its parameter's default, `inferred` when the arguments infer it anyway (types assignable both ways count as the same, as for typescript-eslint) |
| `member_reads`, `member_writes`, `member_constructor_writes` | on a class property, method or parameter property: reads through `this` (or `x.#name`, or another instance's `private` member) inside the class, writes outside the constructor (`=`, `++`, `delete`, destructuring, `this.x.y = v`), and writes in it |
| `uses_this`, `uses_arguments`, `has_yield` | on a function or method: what its own body does (arrows inside count, nested functions do not) |
| `super_calls` | on a constructor: `all`, `some` or `none` of the paths that leave it normally call `super()` |
| `regex_problems` | on a regex literal or the pattern string of `RegExp(...)`: one problem per line: `empty_class`, `spaces:N`, `backref:<kind>:<backref>:<group>`, `misleading:<kind>` |
| `regex_error` | on the pattern string of `RegExp(...)`: the syntax error (or bad flags) as ESLint words it |
| `precision_lost` | on a number literal: not exactly representable as a double |
| `identifier_name` | on an identifier or private name: the name as JavaScript reads it (`foo` is `foo`) |
| `first_token`, `last_token` | the node's first / last code token: `{text, kind, start, end}`; test it with `is` (`first_token is ts_identifier`) |
| `paired_token` | on a node starting with `(`, `[` or `{` (a bracket token node, or `(a)`), or else ending with `)`, `]` or `}`: the matching bracket, as `first_token` |
| `token_texts` | the node's code tokens, in order (comments and spacing dropped): equal lists mean the same tokens |

`callable_contracts(parent, node, ["[Symbol.dispose]"])` asks only about that member, without traversing unrelated fields. `[Symbol.<name>]` path selectors require real Symbol identity on both sides, never printed-string properties. Symbol keys require the actual unshadowed library `Symbol` constructor and a declared symbol-valued member; aliases and arbitrary computed keys remain unknown. Ordinary object-property contexts still match string keys to string keys.

Explicit instantiation (`fn<T>` or `typeof fn<T>`) binds each declared function signature independently; later calls do not infer those parameters again. Constructor instantiation, callable-object instantiation and functions with inferred body-return overrides remain unsupported and produce unknown types.

Mapped property types retain their key domain, dependent value type, optionality and readonly flag. Indexed-key transfers preserve computed-key declaration identity and prefer ordinary declared properties. Key remapping (`[P in K as ...]`) remains unsupported; key domains that cannot be enumerated do not imply an empty property bag.

Values and types are separate namespaces (`const X` does not hide an
imported type `X`), and merged declarations (`interface X` + `const X`,
overloads) are one binding.

**Types** come from declarations: annotations, return types, initializers
and member chains, generic arguments fixed by an argument (`map(x => ...)`),
`await`, `as`, `!`, object literals, `keyof`. Types are read from the files
they are declared in (other project files, `node_modules` packages and
`@types`, `declare module "x"`), with the nearest `tsconfig.json`'s `paths`,
`lib`, `types` and `references` deciding what is visible. Other files are
read once per run and kept only as declaration summaries; unannotated
functions of the linted file itself are typed from their `return`s.

- **Overloads** are chosen by argument types (a bounded assignability check:
  primitives, literals, enums, unions, arrays, tuples, the same generic, and
  small object types property by property). When the arguments cannot pick
  between overloads that return different types, the call is unknown.
- **Narrowing** is local and lazy: a reference's type is refined by the
  conditions guarding it in the same function (`if`/`else`, `?:`, `&&`,
  `||`, early `return`/`throw`) when they test it with `typeof`,
  `=== null`/`undefined`, truthiness, `instanceof` or `Array.isArray`. An
  assignment to it in between cancels the guards; anything else leaves the
  declared type.

Where a type cannot be told, `type` is null and rules stay quiet; there is
no contextual inference beyond callback parameters.

## Turning rules on and configuring them

Importing a language brings every rule written for it, **off**. You turn on
exactly the ones you want; a built-in ruleset (`import rules:recommended`)
is a shortcut that turns on a whole category, and anything it turned on can
be turned off again.

```zlint
import .ts

define {
  in ts_array rule demo:arrays {
    default option mode enum(never, always-multiline) = never
  }
}

rule no-cond-assign                  # on, at its default severity
rule eqeqeq error                    # on, as an error
rule demo:arrays error set always-multiline
rule semi:always info "no ;"         # a severity and a new message
rule style-* off                     # `*` globs over rule names
rule brace-style:* {                 # options are set in a block
  set style "stroustrup"
}
rule padding-line-between-statements {
  set config [
    { blankLine: "always", prev: "*", next: "return" },
  ]
  ignore "scripts/**"                # clauses add to the rule
}
```

Promise misuse checks are independent variants; the family enables all nine:

```zlint
rule no-misused-promises
rule no-misused-promises:spreads off
rule no-misused-promises:predicates off
rule no-misused-promises:conditionals { set flagUnions "strict" }
```

`flagUnions` defaults to `"none"` (only provably all-thenable values). `"strict"` also flags known mixed unions whose awaited and ordinary arms are mutually assignable; `"all"` flags any known thenable arm.

- `rule name` without `in` configures an existing rule: it turns it on at
  its default severity (unless it is on already), `rule name <severity>`
  sets the severity, a message after the severity replaces the rule's own,
  and `rule name off` turns it off.
- In the block, `set option value` sets an option; `when`, `let`, `files`
  and `ignore` add to the rule; `fix as` and `report` replace its own.
- Inline `set value` sets the rule's default option; severity and `set` may appear in either order. A family name sets each variant's default option. Without a declared default option, inline `set` is an error; block `set name value` still works for any option.
- These lines may come before or after the definition; they apply in order.
  A name nothing defines is an error with a suggestion, and says which
  language import defines it (`rule rs-indent` → "comes with `import .rs`").
  Rulesets skip rules of languages you did not import.
- **Option values** must fit the option's [type](#option-types). A mismatch
  says where: ``option `config` of `padding-line-between-statements`:
  `[1].blankLine` must be "any" | "never" | "always", got "sometimes"``.

- `define { ... }` defines without turning on: rules inside stay off until a
  `rule name` line turns them on. The built-in languages are written this
  way. Layers and attributes defined inside `define` take part in parsing
  only while something needs them (a turned-on rule, or your own items), so
  importing a language never changes what your own matchers match.
- `zlint rules` lists every defined rule, the off ones with the severity
  they turn on at.

Default options are also the intended shape for per-context families such as comma-dangle variants.

## Language settings

Language settings describe the source environment, not a rule's policy. `import .ts` declares the JavaScript/TypeScript environment on `.js`:

```zlint
import .ts
language .js {
  files "scripts/**"
  ignore "scripts/generated/**"
  set globals { app: "readonly", cache: "writable", removed: "off" }
  set ecmaVersion 5
  set sourceType "commonjs"
  set impliedStrict false
  set globalReturn true
}
```

- `files` and `ignore` select `set` clauses using config-relative globs, as rule filters do. Without `files`, settings apply to every file of that language. Option schemas/defaults belong to the language, independent of filters. Later matching blocks override scalar settings; global maps merge by name, including explicit `"off"`. Real source global comments override configured names without becoming configuration.
- `ecmaVersion` accepts `3`, `5`, editions `6` through `17`, years `2015` through `2026`, `"latest"` or `null`. Unset/`null` means latest. The edition selects built-in globals and directive semantics; it does not reject every newer syntax feature.
- `sourceType` accepts `"script"`, `"module"`, `"commonjs"` or `null`. Unset/`null` preserves filename/import inference. Modules are strict; CommonJS adds a root function scope and CommonJS globals.
- `impliedStrict` and `globalReturn` accept booleans or `null` (false). `impliedStrict` supplies strictness without fabricating a directive. `globalReturn` adds a script root function scope, not CommonJS globals; modules do not acquire that wrapper.
- A language declares supported settings with `language .name { option name <type> = <default> }`. Unknown settings and values outside the declared type are errors. Changing language settings invalidates cached lint results.

## Option types

Every option declares its type, in TypeScript-like syntax, and a default:
`option name <type> = <default> "description"`. Named types (`schema`) are
shared between rules.

`enum(never, always)` declares string values written as bare names, including hyphenated rule-name characters. Defaults and `set` values need no quotes: `option mode enum(never, always) = never`. A rule may mark at most one option as default: `default option mode enum(never, always) = never "mode"`. Its enum cannot contain severity words (`off`, `hint`, `info`, `warn`, `error`).

```zlint
schema statement_kind "*" | "block-like" | "return" "a statement kind"
schema statement_pair {
  blankLine: "any" | "never" | "always"  "what goes between them"
  prev: statement_kind | statement_kind[]
  next: statement_kind | statement_kind[]
  note?: string                          # optional: null when absent
  max: int(0..) = 2                      # optional, with a default
}

in ts_program rule padding-line-between-statements {
  option config statement_pair[] = [] "blank-line requirements"
  option style "1tbs" | "stroustrup" | "allman" = "1tbs"
  option ignore regex | null = null
  ...
}
```

| type | takes |
|---|---|
| `string`, `number`, `int`, `boolean`, `any`, `null` | values of that kind (`int`: whole numbers) |
| `regex` | a string that is a valid regex in the Rust `re_test` dialect |
| `js_regex` | a string that is a valid JavaScript regex in Unicode (`u`) mode, checked when config loads |
| `"x"`, `3`, `true` | exactly that value |
| `enum(a, b-c)` | one of the bare names, stored as a string |
| `a \| b` | either; a union may continue on a line starting with `\|` |
| `T[]`, `T[1..]`, `T[..3]` | a list of `T`, with an optional length range |
| `int(0..)`, `number(0..1)`, `string(1..)` | a number in a range, or a string of a length |
| `{ key: T; ... }` | a record. `key: T` is required, `key?: T` is optional (null when absent), `key: T = value` is optional with that default. Keys are separated by lines, `,` or `;`, and a `"description"` can follow each |
| `{ [key: string]: T }` | a record with any keys, each taking `T` (it may also list named keys) |
| `(T)` | grouping: `("a" \| "b")[]` |
| `name` | a `schema` |

Checking a value fills in record defaults, so a rule can read every key
(`c.word`) without a fallback expression. Defaults are literal values, and an
option's default must fit its own type. `override schema name ...` replaces
a named type.

Use `option pattern js_regex = "^(?!internal_).+$"` with `js_re_test(text, pattern, "u")`, or `"iu"` to ignore case.

## Changing imported definitions: `override`, `drop`

An imported ruleset is a starting point. Its importer can replace or remove
anything in it, in order: each of these sees everything above it (imports
included), and everything below sees the result.

```zlint
import ../rules/ts-problems

override in ts_call rule no-eval error "eval is banned here" { when callee == "eval" }
override @macro newline "\n"
drop rule no-console                     # the name is free again
drop rule semi:*                         # rule names take globs
drop in ts_call let operators            # one layer's definition
drop let operators                       # every layer's
drop group ts_statement ts_with          # some members, or `drop group g` for all
drop in braces match rs_string           # detach from one parent
drop match js_jsdoc_line
drop macro is_identifier
drop ignore "static/**"
```

- **`override`** goes in front of a definition (`rule`, `match`, `let`,
  `@macro` or `group`) and replaces the existing one, which must exist.
  A plain second definition is an error that points at the first and
  suggests `override`. An overridden rule keeps its place, and `rule` lines
  above the override are dropped with the old rule. `override match x
  pattern` replaces the whole definition, parents included, so write
  `override in A match x ...` to keep it in `A`. Overriding a built-in rule
  (`override in my_layer rule no-console ...`) replaces it with your own.
- **`drop`** removes a definition. Whatever still uses it (rules, `let`s,
  patterns, `like` bases, group members, children, expressions, `rule` lines
  below) is an error **at the `drop`**, with a link to each use, so the fix
  sits in your file: drop or override those too.

## Imports and macros

`import` pastes something's items in place, once:

| import | loads |
|---|---|
| `import .ts`, `.rs`, `.py`, `.c`, `.toml`, `.md`, `.css`, `.json`, `.zlint` | a built-in language: its layers, and every rule written for it, off |
| `import rules:recommended` | a built-in ruleset: `rule` lines turning on a category of rules |
| `import some/path` | another `.zlint` file, relative to this one (`.zlint` is added when there is no extension; quotes are optional) |

Every language also brings the shared building blocks: `parentheses`,
`brackets`, `braces` (nesting), `whitespace`, `space`, `identifier`, `number`,
the `newline` macro (the file's own line break), and the language-neutral
rules `trailing-whitespace`, `max-line-length`, `tab-indent`, `blank-lines`
and `final-newline`.

| language | contents |
|---|---|
| `.rs`, `.py`, `.c`, `.toml`, `.zlint` | brackets, strings, escapes, comments (`rs_code` etc. name all code layers) |
| `.css` | pattern-owned textual delimiters, strings/escapes and block comments; not full CSS validation |
| `.json` | pattern-owned JSON/JSONC objects/arrays, strings/escapes and comments; not full JSON validation |
| `.core/lines` | shared line structure/hygiene with syntax-owner comment/raw adapters |
| `.core/lists` | shared value-only list spacing/wrapping policy over syntax-owner records |
| `.ts` | `native ts` for every JS and TS file: `ts_*` layers, comments (`ts_comment`) and lines (`ts_line`), the groups `ts_node`, `ts_code`, `ts_function_like`, `ts_loop`, `ts_statement`, and the eslint, @stylistic and typescript-eslint ports |
| `.ts/import-attributes` | shared pattern-owned import/export attributes for key spacing and trailing commas; loaded by the rule modules that need it |
| `.ts/conditions` | type-aware condition policy, imported by `.ts`; optional-chain suggestions remain diagnostic-only |
| `.ts/arithmetic` | type-aware addition policy and options, imported by `.ts/typed` |
| `.ts/expressions` | boolean-cast, exponentiation and logical-assignment policy with precedence-safe fixes, imported by `.ts/suggestions` |
| `.ts/expression-fixes` | shared precedence, token-boundary and ASI helpers for expression autofixes |
| `.ts/template-fixes` | shared raw-escape-preserving template fix helpers |
| `.ts/suggestions/object-shorthand` | object member short/long syntax policy and source-preserving fixes |
| `.ts/suggestions/curly` | control-body brace policy and statement-safe fixes |
| `.ts/suggestions/renames` | redundant import, export and destructuring aliases; comment-safe fixes |
| `.ts/suggestions/object-spread` | Object.assign eligibility and source-preserving object spread fixes, imported by `.ts/suggestions` |
| `.ts/suggestions/templates` | string concatenation policies and source-preserving template fixes, imported by `.ts/suggestions` |
| `.ts/suggestions/comparisons` | literal-side comparison policy, range exceptions and source-preserving operand swaps |
| `.ts/typed/optional-chains` | optional-chain guard selection, return-type safety and atomic fixes, imported by `.ts/typed` |
| `.ts/typed/readonly-members` | readonly private members and parameter properties, with type-preserving annotation fixes, imported by `.ts/typed` |
| `.ts/typed/template-expressions` | unnecessary template interpolation checks and raw-escape-preserving, precedence-safe fixes, imported by `.ts/typed` |
| `.ts/typed/floating-promises` | floating promises, including discarded arrays and tuples with immediate promise elements, imported by `.ts/typed` |
| `.ts/parentheses` | expression/type parenthesis policy and IIFE wrapping, imported by `.ts/structure` |
| `.ts/structure/padding` | statement padding policies and comment-aware blank-line fixes, imported by `.ts/structure` |
| `.ts/spreads` | type-aware spread policy and allowed-type options, imported by `.ts/typed`; suggestions remain diagnostic-only |
| `.ts/typed/promises` | `no-misused-promises` variants: `conditionals`, `predicates` (array callbacks), `spreads`, `void-arguments`, `void-attributes`, `void-inherited-methods`, `void-properties`, `void-returns`, `void-variables`; imported by `.ts/typed`, diagnostic-only |
| `.ts/enums` | enum transfer policy (`no-unsafe-enum-assignment`) over the `type_flow` fact, imported by `.ts` |
| `.ts/assertions` | type-aware assertion and non-null policy, imported by `.ts` |
| `.ts/naming` | name-format policy (`naming-convention` variants), open context groups and shared `naming_failure` helper; imported by `.ts/scoping` |
| `.ts/deprecation` | declaration documentation policy (`no-deprecated`), imported by `.ts`; diagnostic-only |
| `.ts/type-specifiers` | shared type/value origin matching helpers (`matches_type_specifier`, `type_is_allowed`) |
| `.ts/methods` | type-aware method policy (`prefer-find`), imported by `.ts/typed`; filter result zero-index/bracket/at access, computed constant methods, sequences and both conditional branches; diagnostic-only |
| `.ts/selectors` | open `restricted_syntax` group and prohibition rule (`no-restricted-syntax`), imported by `.ts` |
| `.rs` | also the `rs-*` layout rules |
| `.md` | code fences, inline code, HTML comments |

A language's rules live in topic files it imports itself (`.ts/format`,
`.ts/layout`, `.ts/indentation`, `.ts/style` with its parts `.ts/style/ordering`,
`.ts/style/signatures`, ...; `.rs/format`): internal building blocks, not
separate bundles.

| ruleset | turns on |
|---|---|
| `rules:correctness` | code that is almost certainly wrong (`no-cond-assign`, `use-isnan`, `valid-typeof`, ...) |
| `rules:suspicious` | likely mistakes, or code that does nothing (`no-fallthrough`, `no-self-assign`, ...) |
| `rules:conventions` | one consistent way to write things (`prefer-const`, `eqeqeq`, ...) |
| `rules:format` | layout and spacing, as eslint's and @stylistic's defaults lay code out, and the `rs-*` rules |
| `rules:types` | rules that read declared types (`no-floating-promises`, ...) |
| `rules:recommended` | what eslint and typescript-eslint recommend without type information |

A `.zlint` file is a config file: `import ../.zlint` builds on the config
of the directory above.

**Functions** compute a value from their arguments, for logic several rules
share. A body sees its parameters and nothing of the node: pass in what it
needs. Functions may call each other and themselves.

```zlint
fn indent_width(line) = {
  value line
  trim_start
  then (trimmed) len(line) - len(trimmed)
}
fn rank(order, group) = index_of(order, group)
in ts_class_body rule late-member warn "out of order" {
  when rank(["field", "method"], kind) < rank(["field", "method"], previous.kind)
}
```

Macros paste tokens: they generate zlint code, not text. Everything that
happens while the config loads starts with `@`: defining a macro, calling
one, `@each`.

```zlint
@macro rs_code_layers .rust, parentheses, brackets, braces  # one line

@macro forbid(name, pattern, why) {                         # lines, with arguments
  in rs_code match name pattern {
    rule name error why
  }
}
@forbid(no_dbg, /\bdbg!/, "remove dbg! before committing")
```

Arguments are glued to the name (`@forbid(`); `@macro nl (...)`, with a
space, is a one-line macro whose body starts with `(`. An expansion behaves
like a parenthesised group wherever it lands, so `@macro digits /\d/+` then
`@digits?` means `(/\d/+)?`. Macros cannot recurse, and redefining one needs
`override @macro`. Plain `macro` definitions are errors; bare names never expand.

## Suppressing diagnostics

Write these inside any comment:

| directive | effect |
|---|---|
| `zlint-ignore` alone on a line | next line, every rule |
| `zlint-ignore` after code | this line |
| `zlint-ignore rule-a rule-b` | only these rules (`*` globs allowed, a family name covers its variants) |
| `zlint-ignore-start [rules]` ... `zlint-ignore-end` | every line in between |
| `zlint-ignore-file [rules]` | the whole file |

## Editor and tooling

`zlint lsp` (used by the VS Code extension) understands `.zlint` files as
well as the files they lint:

- **hover** on any name: rules with their final severity and option values,
  layers with their fields, rules and attributes, groups with their members,
  attributes, macros, builtins, facts, functions and keywords
- **go to definition** (std modules open as read-only copies), **find all
  references** across imports and macro bodies, **completion** by context
  (layers after `in`, fields after `layer.`, every defined rule after `rule `,
  severities and `set` after a rule name, enum words after `set `, the rule's options after block `set `, languages and rulesets after
  `import `) and an **outline**
- config errors in place, with links to related spots ("first defined
  here", "still used here")
- every diagnostic's rule code links to where the rule is defined
- with `zlint.inspectHover`, hovering in a linted file shows the layers at
  the cursor and their facts

Normal CLI linting compiles enabled rules and reachable helpers only. Errors in off-rule bodies and unreachable function bodies are deferred until activation. Parse errors and configured option errors still report. `zlint check`, tooling commands and the language server keep full compilation and validation.

### Packed sources

`zlint --sources <file|->` reads a JSONL stream of `{"path":"main.ts","text":"..."}` records; `-` reads stdin. Paths resolve relative to the current directory. Sources are registered before config discovery, so the stream may include `.zlint` and its imports. Buffered files and implied directories participate in TypeScript module and global resolution; dependencies not supplied still read from disk.

Only supplied files matching the config are linted: no disk discovery. Use `--sources - --format jsonl --no-cache --jobs 1` to lint an in-memory project without writing source files or a cache. Positional paths and `--fix` cannot be combined with `--sources`. Malformed records and duplicate normalised paths report their input line and exit with a configuration/input error. Buffers last only for that invocation.

Tooling commands also accept packed configs and files: for example, `zlint rules --sources - --config project/.zlint`. `tree` and `eval` still take their target file arguments. `lsp` rejects `--sources` because it owns stdin as a protocol stream.

## Performance and caching

`zlint --stats` prints throughput, and `zlint --profile` lists the matchers
that cost the most interpreter steps and the rules that cost the most time.
Matchers that compile to a DFA never show up there; slow ones are usually
nullable, use fields, or have no leading literal or lookaround to tell zlint
where they can start.

CLI linting skips compilation of off-rule bodies, unreachable functions and library attributes, and does not build tooling symbols. Layers only off rules need are not parsed.

**`.zlintcache`.** zlint keeps each file's results beside the config and
reuses them while the file's text is unchanged and so is everything its lint
read from other files: the declaration summaries of imported files (an edit
inside a function body changes nothing for importers, an exported type
does), module resolution (a new `a.ts` beside `a/index.ts`), tsconfigs and
`package.json`s, and which files declare globals. Results with problems are
kept too, so a warm run only lints what changed. Any change to the rules
(comments and blank lines aside) or to the zlint binary starts afresh.

`--no-cache` lints everything without reading or writing the cache, and
`--profile` implies it. `--stats` shows how many files came from the cache.
A run over the whole project forgets files that no longer exist. The editor
uses the same cache for its workspace scan, and re-lints the files an edit
affects as you type, open or not. Add `.zlintcache` to `.gitignore`.
