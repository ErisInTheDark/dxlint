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
| `over A match name pattern` | an overlay: searched over `A`'s text without consuming it |
| `group name A, B` | add layers to a named group, usable wherever a layer list is |
| `in A, B.field indent` | lines inside these layers (or fields) are indented, see [Layers](#layers) |
| `in A rule name ...` | define a rule checking `A`'s nodes (and turn it on), see [Rules](#rules) |
| `rule name [severity] [{ ... }]` | turn an existing rule on, off, or configure it, see [Turning rules on](#turning-rules-on-and-configuring-them) |
| `define { items }` | items inside define; their rules stay off until turned on |
| `in A let name expr` | an attribute computed for `A`'s nodes, see [Attributes](#attributes) |
| `in A forward x, y from field` | `A`'s nodes pass attributes `x` and `y` up from a field, see [Attributes](#attributes) |
| `native ts` | TypeScript/JavaScript syntax as `ts_*` layers, see [Native layers](#native-layers-typescript) |
| `override <definition>`, `drop ...` | replace or remove what an import defined, see [Changing imported definitions](#changing-imported-definitions-override-drop) |
| `ignore "glob" "glob"` | never lint matching files |
| `import .ts`, `import rules:recommended`, `import some/path` | a built-in language, a built-in ruleset, or another `.zlint` file, see [Imports](#imports-and-macros) |
| `macro name tokens`, `macro name(args) { lines }` | define a macro |

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

`in A`, `over A`, `match x ...` and `lang l ...` set a **context**: the layers
the rest of the line is about. A `{` at the end of such a line opens a block,
and every line inside it is in that context:

| head | lines inside mean |
|---|---|
| `in A, B {` | `in A, B <line>` |
| `over A {` | `over A <line>`: matchers are overlays |
| `match x ... {` | `in x <line>` |
| `lang l ... {` | `in .l <line>` |
| `rule r ... {` | rule clauses, see [Rules](#rules) |

In a context, `match` defines or attaches children, `rule` checks the
context's layers themselves, and `indent` indents them. Blocks nest, so a
layer, its children and its rules can live together:

```zlint
lang rust "*.rs" {
  match braces /\{/ until /\}/ {
    indent
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
`braces`: it keeps every child and overlay of `braces`, wins ties against it,
and counts as `braces` for rules (`in braces rule` sees it, with `layer`
naming the specialisation), for `is` tests and for ancestor lookups (`braces`
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

**Language gating.** A matcher attached to a `.lang` only runs inside files
of the languages it is attached to (or attached through `.*`). That lets
shared layers like `parentheses` carry Rust strings in Rust files and
JavaScript strings in JavaScript files.

**Overlays.** `over A match x ...` searches `A`'s text for `x` without
taking that text away from `A`'s other children. Use overlays for
line-oriented checks:

```zlint
over .* match trailing_whitespace /[ \t]+$/
```

**Indentation.** `in braces, parentheses, brackets indent` declares that a
layer's inside, the text its `until` skips, is indented: a line starting
there belongs one level deeper than the line the layer opened on, and the
line where its end pattern starts (`}`) sits at the opener's level. `in
function.body indent` does the same for one field, and `indent` alone inside a
matcher's block indents that matcher. Measuring from the opener *line* means
`foo(Bar {` adds one level, not two. The engine works out which layer a line
is in; rules read it through `indented`, `indent_anchor`, `indent_closes`,
`indent_hangs` and `indent_anchor_line` (see [Expressions](#expressions)) and
decide the unit and any language-specific continuation lines:

```zlint
in parentheses, brackets, braces indent
over rs_code match line /^[ \t]*\S/          # each code line, first char included

in line rule indent {
  let level if !indented then "" else if indent_closes then indent_anchor else indent_anchor "    "
  when slice(text, 0, -1) != level
  warn "Expected {len(level)} spaces of indentation."
  fix as level slice(text, -1)
}
```

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
With several targets (`in a, b.field rule`), fields used in the body must
exist on all of them.

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

**Body clauses.**

| clause | meaning |
|---|---|
| `off\|hint\|info\|warn\|error [message]` | severity (default `warn`) and message |
| `when expr` | only nodes where `expr` is truthy; several `when`s must all hold, in order |
| `let name expr` | a value computed once per node and usable by later clauses, the message and the fix |
| `fix as expr` | what the node should be |
| `option name <type> = <default> "description"` | a configurable value of that type, see [Option types](#option-types) |
| `files "glob" ...` | only these files |
| `ignore "glob" ...` | not these files |
| `report node\|change\|end` | where diagnostics point: the whole node (default), only the part the fix changes, or the node's end |
| `report at expr` | where diagnostics point: a byte offset starts them there (`offset` and `end_offset` are a node's own; native facts give others, such as `write_site`); a `{start, end}` record or a node (`declaration`) gives the whole span |
| `@each name in list` | one diagnostic per item of a list from the rule's options (`@each entry in paths`): `when`, the message, `fix` and `report at` see the item as `name` (`let`s stay per node). The list is known when the config loads; rules never loop over code |

A macro at the start of a body line is pasted as clauses.

**Rule `let`s** name an intermediate value, so a long condition reads as
steps. Each is computed at most once per node, only when something reads it,
and can read the `let`s above it:

```zlint
in ts_statement rule padding-line-between-statements {
  when index > 0 && parent is (ts_block, ts_program)
  let wanted is_conditional || previous.wants_blank_after == true
  when wanted && !re_test(default(gap_before, ""), /\n[ \t]*\r?\n/)
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
| `reduce(xs, init, (state, x) => ...)` | folds a list in order: each step sees the state the previous one gave |
| `matcher` / `matcher.x` | the closest enclosing node of `matcher`, or anything read on it (`ts_function.name`, `ts_class.offset`, `ts_function.my_attr`) |
| `parent.x` | anything read on the layer the node sits in (`parent.offset`, `parent.depth`) |
| `field.layer`, `field.offset`, `field.attr`, `field.fact`, `field.link` | a builtin, attribute, fact or link read on the node in one of this node's fields (a list for several) |
| `callee.object.x`, `parent.left.x`, `previous.body.x` | longer paths walk: each field label steps into its node(s), the last part is read there |
| `first(references).line` | `.x` on a node value (from a link, `node`, or a lambda parameter) reads on that node; longer paths walk (`n.callee.object.layer`) |
| `node`, `right.node`, `parent.node` | the node itself as a node value (it reads as its text) |
| `overlapping(layer)` | the nodes of a layer or group anywhere in the file whose span overlaps the node's, in source order: on a `ts_line`, `overlapping(ts_comment)` |
| `descendants(layer)` | the nodes of a layer or group (specialisations too) inside the node, in source order: `descendants(ts_export)` |
| `walk(roots, n => next)` | the nodes reachable from `roots` by `next` (nodes or lists of them), each once, roots first: `walk([] references.node, n => n.writer.right.node)` |
| `f(a, b)` | function call |
| `a b` | join: values side by side join as text (`text "="`), or as lists if either is one (`[a] [b]`) |
| `!a` `-a` `a * b` `a % b` `a + b` `a - b` | arithmetic (`text * n` repeats text) |
| `== != < <= > >=` | comparisons (numbers compare numerically, even as text) |
| `a && b`, `a \|\| b` | logic, short-circuiting |
| `value ~ pattern-item` | does the whole value match? (`text ~ /[a-z_]+/`, `text ~ snake`) |
| `value is layer`, `value is (a, b)`, `value not (a, b)` | does the layer named by `value` (`layer`, `parent`, `previous.layer`...) count as one of these: itself, a specialisation, or a group member (`not`: none of them) |
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
  || layer is (ts_break, ts_continue, ts_throw)
  || spans_lines && declares != null
```

**`is`** compares layer identity, not text: `layer is ts_loop` holds for every
member of the `ts_loop` group, and `parent is braces` for any specialisation
of `braces`. Several names go in parentheses, a group in place:
`parent is (ts_block, ts_program)`. `not` is the inverse: `next_token not
(ts_closer, ts_comma)`. Unknown names are config errors.

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
`reduce(xs, init, (state, x) => next)` carries a value through the list, for
decisions that depend on earlier ones. `keys`, `values`, `get(record, key)`
and `len` read records. A lambda sees everything the expression around it
sees, plus its parameters:

```zlint
let wanted find_last(config, c => matches(c.prev, previous.layer) && matches(c.next, layer)).blankLine
```

**Variables.**

| name | value |
|---|---|
| `text` | the node's text |
| `layer` | the node's own matcher name (a specialisation's own name) |
| `indent` | leading whitespace of the node's first line |
| `line`, `column` | 1-based position of the node |
| `file` | path relative to the config |
| `lang` | the file's language |
| `before`, `after` | the rest of the node's first line before it, and of its last line after it |
| `previous_line`, `next_line` | the whole lines around the node (`null` at the file's edges); `previous_line` as its indentation fix leaves it |
| `text_before`, `text_after`, `source` | everything before the node, after it, and the whole file |
| `parent`, `parent_text` | the layer the node sits in, and its text |
| `parent_before` | the parent's text up to the node ("what came earlier in this statement") |
| `parent_indent`, `parent_line` | leading whitespace and whole text of the line the parent starts on |
| `previous_sibling`, `previous_gap` | the previous node of the same layer in the same parent, and the text between |
| `gap_before` | the text between the previous child of the parent (of any layer) and the node: blank lines, comments, separators; `null` for the first child |
| `index` | the node's position among the parent's children in the same field, from 0 (the second `ts_call.argument` is `1`) |
| `enclosing` | the innermost layer around the node's start, seeing through overlays (`"js_string"` inside a string) |
| `indented` | the node's line starts inside a layer declared with `indent` |
| `indent_anchor`, `indent_anchor_line` | leading whitespace and text of the line that (innermost) layer opened on |
| `indent_closes` | this line starts with that layer's end pattern, so it sits at the anchor |
| `indent_hangs` | that layer's inside starts on its opener line (`f(a,⏎ b)`) |

**Neighbours.** `children`, `previous` and `next` reach other nodes:

| name | value |
|---|---|
| `children.x` | `x` of every direct child, in order, whatever its field (`null`s kept, so `last(children.x)` is the last child's) |
| `previous.x`, `next.x` | `x` of the previous or next node in the same field of the same parent (`null` at the edges) |
| `children`, `previous`, `next` alone | their text |

`x` can be a field, an attribute, a fact or a builtin (`previous.layer`).

**Functions.** `zlint functions` lists them all. Highlights: `len`, `lower`,
`upper`, `snake_case`, `camel_case`, `pascal_case`, `kebab_case`,
`screaming_case`, `title_case`, `trim`, `trim_start`, `trim_end`, `replace`,
`repeat`, `pad_start`, `pad_end`, `starts_with`, `ends_with`, `contains`,
`split`, `lines`, `join`, `sort`, `unique`, `reverse`, `first`, `last`,
`compact`, `sum`, `any`,
`count`, `slice`, `str`, `num`, `default`, `min`, `max`, `abs`, `round`,
`compare_text`, `js_compare`.

`slice` counts characters; offsets (`offset`, `end_offset`, native spans)
count bytes, so cut source text at them with `byte_slice(text, from, to?)`.
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
on (ESTree names, abbreviations spelled out; `zlint tree file.ts` shows any
file). Rules on them work like any rule:

```zlint
import .ts
in ts_call rule my-no-eval warn "no eval" { when callee == "eval" }
in ts_if.test, ts_while.test rule assign-in-condition warn "assignment in a condition" {
  when layer is ts_assignment
}
```

A file the parser cannot read gets one warning instead of rules on `ts_*`
layers.

**One tree.** A file the parser reads gets one tree: the parser's nodes, and
the pattern layers inside them. A pattern layer inside a native layer is
matched across that node's text and sits under it; the language's own
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
that whitespace. Spacing rules are one decision per token:

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

Trivia are layers too, also only when a rule targets them: comments
(`ts_line_comment`, `ts_block_comment`, group `ts_comment`) under the
innermost node holding them, in field `comment`; and `ts_line`, every line
of the file (JavaScript line breaks, the break left out; the last may be
empty) under the program, in field `lines`.

Group `ts_node` holds every syntax layer (not tokens or trivia): `in ts_node let
...` gives an attribute to all of them, for walks that may cross any kind.

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
| `resolved`, `reference_kind` | on a referencing identifier: `local`, `import` or `global` (declared nowhere in the file), and the `declaration_kind` of what it refers to |
| `type` | the expression's type as text (`Promise<void>`, `never`, `NodeJS.Timeout`), null when unknown |
| `type_flags` | what the type is made of: `any`, `never`, `string`, `number`, `boolean`, `bigint`, `symbol`, `literal`, `enum`, `array`, `tuple`, `promise`, `function`, `object`, `null`, `undefined`, `nullable`, `union`, and `unknown` for parts zlint cannot read |
| `type_members` | member names of that type, inherited ones included (`then`, `[Symbol.dispose]`) |
| `type_parameter_defaults`, `type_parameters_inferred` | on a call, `new`, type reference or class with type arguments: each type parameter's default, and what the arguments infer, printed |
| `type_argument_redundant` | on the last type argument: `default` when it is its parameter's default, `inferred` when the arguments infer it anyway (types assignable both ways count as the same, as for typescript-eslint) |
| `member_reads`, `member_writes`, `member_constructor_writes` | on a class property, method or parameter property: reads through `this` (or `x.#name`, or another instance's `private` member) inside the class, writes outside the constructor (`=`, `++`, `delete`, destructuring, `this.x.y = v`), and writes in it |
| `uses_this`, `uses_arguments`, `has_yield` | on a function or method: what its own body does (arrows inside count, nested functions do not) |
| `super_calls` | on a constructor: `all`, `some` or `none` of the paths that leave it normally call `super()` |
| `regex_problems` | on a regex literal or the pattern string of `RegExp(...)`: one problem per line: `empty_class`, `spaces:N`, `backref:<kind>:<backref>:<group>`, `misleading:<kind>` |
| `regex_error` | on the pattern string of `RegExp(...)`: the syntax error (or bad flags) as ESLint words it |
| `precision_lost` | on a number literal: not exactly representable as a double |
| `identifier_name` | on an identifier or private name: the name as JavaScript reads it (`foo` is `foo`) |
| `first_token`, `last_token` | the node's first / last code token: `{text, layer, start, end}` |
| `paired_token` | on a node starting with `(`, `[` or `{` (a bracket token node, or `(a)`), or else ending with `)`, `]` or `}`: the matching bracket, as `first_token` |
| `token_texts` | the node's code tokens, in order (comments and spacing dropped): equal lists mean the same tokens |

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

rule no-cond-assign                  # on, at its default severity
rule eqeqeq error                    # on, as an error
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

- `rule name` without `in` configures an existing rule: it turns it on at
  its default severity (unless it is on already), `rule name <severity>`
  sets the severity, a message after the severity replaces the rule's own,
  and `rule name off` turns it off.
- In the block, `set option value` sets an option; `when`, `let`, `files`
  and `ignore` add to the rule; `fix as` and `report` replace its own.
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

## Option types

Every option declares its type, in TypeScript-like syntax, and a default:
`option name <type> = <default> "description"`. Named types (`schema`) are
shared between rules.

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
| `regex` | a string that is a valid regex |
| `"x"`, `3`, `true` | exactly that value |
| `a \| b` | either; a union may continue on a line starting with `\|` |
| `T[]`, `T[1..]`, `T[..3]` | a list of `T`, with an optional length range |
| `int(0..)`, `number(0..1)`, `string(1..)` | a number in a range, or a string of a length |
| `{ key: T; ... }` | a record. `key: T` is required, `key?: T` is optional (null when absent), `key: T = value` is optional with that default. Keys are separated by lines, `,` or `;`, and a `"description"` can follow each |
| `{ [key: string]: T }` | a record with any keys, each taking `T` (it may also list named keys) |
| `(T)` | grouping: `("a" \| "b")[]` |
| `name` | a `schema` |

Checking a value fills in record defaults, so a rule can read every key
(`c.word`) without `default(...)`. Defaults are literal values, and an
option's default must fit its own type. `override schema name ...` replaces
a named type.

## Changing imported definitions: `override`, `drop`

An imported ruleset is a starting point. Its importer can replace or remove
anything in it, in order: each of these sees everything above it (imports
included), and everything below sees the result.

```zlint
import ../rules/ts-problems

override in ts_call rule no-eval error "eval is banned here" { when callee == "eval" }
override macro newline "\n"
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
  `macro` or `group`) and replaces the existing one, which must exist.
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
| `import .ts`, `.rs`, `.py`, `.c`, `.toml`, `.md`, `.zlint` | a built-in language: its layers, and every rule written for it, off |
| `import rules:recommended` | a built-in ruleset: `rule` lines turning on a category of rules |
| `import some/path` | another `.zlint` file, relative to this one (`.zlint` is added when there is no extension; quotes are optional) |

Every language also brings the shared building blocks: `parentheses`,
`brackets`, `braces` (nesting), `whitespace`, `space`, `identifier`, `number`,
the `newline` macro (the file's own line break), and the language-neutral
rules `trailing-whitespace`, `max-line-length`, `tab-indent`, `blank-lines`
and `final-newline`.

| language | contents |
|---|---|
| `.rs`, `.ts`, `.py`, `.c`, `.toml`, `.zlint` | brackets, strings, escapes, comments (`rs_code`, `js_code` etc. name all code layers) |
| `.ts` | also `native ts` for every JS and TS file (parsed only when a rule needs a `ts_*` layer), the groups `ts_function_like`, `ts_loop`, `ts_statement`, and the eslint, @stylistic and typescript-eslint ports |
| `.rs` | also the `rs-*` layout rules |
| `.md` | code fences, inline code, HTML comments |

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
fn indent_width(line) = len(line) - len(trim_start(line))
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
`override @macro`. (Plain `macro` and calls without `@` still work while
configs move over.)

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
  the rule's options after `set ` in its block, languages and rulesets after
  `import `) and an **outline**
- config errors in place, with links to related spots ("first defined
  here", "still used here")
- every diagnostic's rule code links to where the rule is defined
- with `zlint.inspectHover`, hovering in a linted file shows the layers at
  the cursor and their facts

## Performance and caching

`zlint --stats` prints throughput, and `zlint --profile` lists the matchers
that cost the most interpreter steps and the rules that cost the most time.
Matchers that compile to a DFA never show up there; slow ones are usually
nullable, use fields, or have no leading literal or lookaround to tell zlint
where they can start.

Built-in rules that are off cost nothing: the layers only they need are not
even parsed.

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
