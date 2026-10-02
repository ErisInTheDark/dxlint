# The `.zlint` language

A `.zlint` file describes **layers** (what the text is made of) and **rules**
(what the text should look like). zlint parses every file into a tree of
layers using only fast regex-style matching, then runs rules on the nodes
they check.

```zlint
import "std/rust"

in rs_code match rs_fn /\bfn[ \t]+/ name:ident
in rs_fn.name rule fn-snake-case warn "function names are snake_case" fix snake_case(text)
```

- [Items](#items)
- [Context blocks](#context-blocks)
- [Layers](#layers)
- [Patterns](#patterns)
- [Rules](#rules)
- [Expressions](#expressions)
- [Configuring rules: `set`](#configuring-rules-set)
- [Imports and macros](#imports-and-macros)
- [Suppressing diagnostics](#suppressing-diagnostics)
- [Performance](#performance)

## Items

| item | meaning |
|---|---|
| `lang rust "*.rs" or "*.rs.in"` | a language: the root layer `.rust` for files matching the globs |
| `match name pattern` | a matcher (layer) that is only used where something references it |
| `in A or B match name pattern` | a matcher searched inside layers `A` and `B` |
| `in A match name` | attach an existing matcher to more parents |
| `match name like base pattern` | a specialisation of `base`, see [Layers](#layers) |
| `over A match name pattern` | an overlay: searched over `A`'s text without consuming it |
| `group name A or B` | add layers to a named group, usable wherever a layer list is |
| `in A or B.field indent` | lines inside these layers (or fields) are indented, see [Layers](#layers) |
| `in A rule name ...` | a rule checking `A`'s nodes, see [Rules](#rules) |
| `set name.option value`, `set name severity` | configure an existing rule |
| `ignore "glob" or "glob"` | never lint matching files |
| `import "path"` | paste another `.zlint` file here |
| `macro name tokens`, `macro name(args) { lines }` | define a macro |

`#` starts a comment. Items end at the end of the line, or at a `}` that
closes their block.

Globs without a `/` match any single path component (`*.rs`, `target`), and
globs with a `/` match the path relative to the config file (`src/**/*.rs`,
`tests/fixtures/**`).

## Context blocks

`in A`, `over A`, `match x ...` and `lang l ...` set a **context**: the layers
the rest of the line is about. A `{` at the end of such a line opens a block,
and every line inside it is in that context:

| head | lines inside mean |
|---|---|
| `in A or B {` | `in A or B <line>` |
| `over A {` | `over A <line>`: matchers are overlays |
| `match x ... {` | `in x <line>` |
| `lang l ... {` | `in .l <line>` |
| `rule r ... {` | rule clauses, see [Rules](#rules) |

In a context, `match` defines or attaches children, `rule` checks the
context's layers themselves, and `indent` indents them. Blocks nest, so a
layer, its children and its rules can live together:

```zlint
lang rust "*.rs" {
  match brace /\{/ until /\}/ {
    indent
    match brace                                  # braces nest
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
├── brace
│   ├── rs_string          children found inside a brace
│   │   └── rs_escape
│   └── brace
└── rs_fn
    └── name: ident        fields: labelled parts of a match
```

**Parents.** `in A or B match x` makes `x` a child of `A` and `B`. A parent
is a matcher name, a group, `.lang` (a language root) or `.*` (every
language root).

**Searching.** A layer looks for its children across its text. Anything that
no child matches is skipped, so you never have to describe a whole language,
only the parts you care about. A language root does this over the whole file.

**`until` (skip).** Inside a pattern, `until` lazily skips text until the rest
of the pattern can match, and while skipping, the layer's children eat whole
chunks:

```zlint
match paren /\(/ until /\)/
in paren match paren                         # nested parens are eaten whole
in .rust or paren match rs_string /"/ until /"/
in rs_string match rs_escape /\\./           # so `\"` never ends a string
```

`f(")", g(x))` is one `paren`: the `")"` is eaten by `rs_string`, and `(x)` by
the nested `paren`, so the outer `\)` only matches the real closing bracket.
This is how brackets, quotes and escapes work without special support. They
are just layers.

A matcher without `until` searches for its children across its own text
after it matches, skipping its fields, the same way a language root does.

**Ties.** When several children match at the same position, the longest match
wins, and among equal lengths a specialisation beats its base, then the one
attached first wins.

**Specialisation (`like`).** `match js_object like brace ...` is a kind of
`brace`: it keeps every child and overlay of `brace`, wins ties against it,
and counts as a `brace` for rules (`in brace rule` sees it, with `layer`
naming the specialisation) and for ancestor lookups (`brace` in an expression
finds the closest one). Use it to tell kinds of one shape apart without
re-describing them:

```zlint
match js_object like brace /(?<=[=(,]\s*)\{/ until /\}/
in js_code match js_object {
  rule object-spacing warn "..." fix ...         # plain braces are untouched
}
```

**Groups.** `group js_code .js or paren or brace` names a set of layers, and
every further `group js_code ...` line adds to it, from anywhere (another
file, an importer). Groups are resolved after everything is loaded, so
`in js_code match x` reaches layers added later too.

**Language gating.** A matcher attached to a `.lang` only runs inside files
of the languages it is attached to (or attached through `.*`). That lets
shared layers like `paren` carry Rust strings in Rust files and JavaScript
strings in JavaScript files.

**Overlays.** `over A match x ...` searches `A`'s text for `x` without
taking that text away from `A`'s other children. Use overlays for
line-oriented checks:

```zlint
over .* match trailing_ws /[ \t]+$/
```

**Indentation.** `in brace or paren or brack indent` declares that a layer's
inside, the text its `until` skips, is indented: a line starting there
belongs one level deeper than the line the layer opened on, and the line
where its end pattern starts (`}`) sits at the opener's level. `in fn.body
indent` does the same for one field, and `indent` alone inside a matcher's
block indents that matcher. Measuring from the opener *line* means
`foo(Bar {` adds one level, not two. The engine works out which layer a line
is in; rules read it through `indented`, `indent_anchor`, `indent_closes`,
`indent_hangs` and `indent_anchor_line` (see [Expressions](#expressions)) and
decide the unit and any language-specific continuation lines:

```zlint
in paren or brack or brace indent
over rs_code match line /^[ \t]*\S/          # each code line, first char included

macro level (if !indented then "" else if indent_closes then indent_anchor else indent_anchor + "    ")
in line rule indent warn "Expected {len(level)} spaces of indentation." {
  when slice(text, 0, -1) != level
  fix level + slice(text, -1)
}
```

**Fields.** Labelled pieces of a match become fields:

```zlint
in .rust match call name:ident args:paren     # call.name, call.args
in .rust match call2 ident :paren             # `:paren` is short for `paren:paren`
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
| `( a b or c )` | grouping and alternation |
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
in rs_fn.name rule fn-snake-case warn "function names are snake_case" fix snake_case(text)

in line rule line-length {
  option max 100 "what max means"
  error "line is {len(text)} long"
  files "src/**"
  ignore "src/generated/**" or "vendor/**"
  when len(text) > max
  fix slice(text, 0, max)
}
```

A rule is `rule name [severity [message]] [fix expr] [{ clauses }]` in a
context (`in ...` or a block).

**Targets.** A rule checks every node of its context's layers: a matcher (and
its specialisations), a field (`in m.field rule`), a group (every member),
`.lang` (whole files of a language) or `.*` (whole files of every language).
With several targets (`in a or b.field rule`), fields used in the body must
exist on all of them.

**Severity and message.** The severity keyword (`off`, `hint`, `info`, `warn`,
`error`) comes first and the message follows it, on the rule's line or as a
body line (`warn "..."`). The message can use everything an expression can
(`"{text} left in a comment"`), or be a macro. Without one the rule is a
`warn` and its message is its name. `zlint rules` shows messages with option
values filled in and other holes left as written.

**Names.** A rule name may be `family:variant` (`semi:never`,
`no-multiple-empty-lines:eof`): several rules that make up one. `set semi:*
off` reaches every variant, and suppressing `semi` suppresses the whole
family.

**Fix.** `fix expr` says what the node *should* be. If its text differs from
the node's text, the rule reports the node and offers the fix. If they are
equal, nothing happens. Fixes can be anything: reorder, rename, delete
(`""`), add text. A rule without a fix reports every node that passes its
`when` conditions.

**Body clauses.**

| clause | meaning |
|---|---|
| `off\|hint\|info\|warn\|error [message]` | severity (default `warn`) and message |
| `when expr` | only nodes where `expr` is truthy; several `when`s must all hold |
| `fix expr` | what the node should be |
| `option name value "description"` | a configurable value, see [`set`](#configuring-rules-set) |
| `files "glob" or ...` | only these files |
| `ignore "glob" or ...` | not these files |
| `report node\|change\|end` | where diagnostics point: the whole node (default), only the part the fix changes, or the node's end |

A macro at the start of a body line is pasted as clauses.

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
| `42`, `1.5`, `true`, `false`, `null` | literals |
| `[a, b]` | list |
| `name` | a field of the node, an option, or a variable |
| `matcher` / `matcher.field` | the closest enclosing node of `matcher`, or its field |
| `f(a, b)` | function call |
| `!a` `-a` `a * b` `a % b` `a + b` `a - b` | operators (`+` joins text and lists, `text * n` repeats) |
| `== != < <= > >=` | comparisons (numbers compare numerically, even as text) |
| `a && b`, `a \|\| b` | logic, short-circuiting |
| `value ~ pattern-item` | does the whole value match? (`text ~ /[a-z_]+/`, `text ~ snake`) |
| `/regex/` elsewhere | the regex source as text, for the `re_*` functions |
| `if c then a else b` | conditional |

**Values** are `null`, booleans, numbers, text and lists. A field that did not
match is `null`, and a field that matched several times is a list. `null`,
`false`, `0`, `""` and `[]` are falsy.

**Variables.**

| name | value |
|---|---|
| `text` | the node's text |
| `layer` | the node's own matcher name (a specialisation's own name) |
| `indent` | leading whitespace of the node's first line |
| `line`, `col` | 1-based position of the node |
| `file` | path relative to the config |
| `lang` | the file's language |
| `before`, `after` | the rest of the node's first line before it, and of its last line after it |
| `prev_line`, `next_line` | the whole lines around the node (`null` at the file's edges) |
| `text_before`, `text_after`, `source` | everything before the node, after it, and the whole file |
| `parent`, `parent_text` | the layer the node sits in, and its text |
| `parent_before` | the parent's text up to the node ("what came earlier in this statement") |
| `parent_indent`, `parent_line` | leading whitespace and whole text of the line the parent starts on |
| `prev_sibling`, `prev_gap` | the previous node of the same layer in the same parent, and the text between |
| `enclosing` | the innermost layer around the node's start, seeing through overlays (`"js_string"` inside a string) |
| `indented` | the node's line starts inside a layer declared with `indent` |
| `indent_anchor`, `indent_anchor_line` | leading whitespace and text of the line that (innermost) layer opened on |
| `indent_closes` | this line starts with that layer's end pattern, so it sits at the anchor |
| `indent_hangs` | that layer's inside starts on its opener line (`f(a,⏎ b)`) |

**Functions.** `zlint functions` lists them all. Highlights: `len`, `lower`,
`upper`, `snake_case`, `camel_case`, `pascal_case`, `kebab_case`,
`screaming_case`, `title_case`, `trim`, `trim_start`, `trim_end`, `replace`,
`repeat`, `pad_start`, `pad_end`, `starts_with`, `ends_with`, `contains`,
`split`, `lines`, `join`, `sort`, `unique`, `reverse`, `first`, `last`,
`count`, `slice`, `str`, `num`, `default`, `min`, `max`, `abs`, `round`.

Regex functions take the full syntax of Rust's `regex` crate (no lookaround
or backreferences there): `re_test(text, re)`, `re_find(text, re)` (first
match or `null`), `re_count(text, re)`, `re_replace(text, re, with)` (`$1`
for groups) and `re_escape(text)` to build patterns from text.

## Configuring rules: `set`

```zlint
set max-line-length.max 100
set max-line-length error
set style-* off                      # `*` globs over rule names
```

Option values must be constants of the same type as the option's default.
Every rule's severity can be set.

## Imports and macros

`import "path"` pastes another file's items in place. Paths are relative to
the importing file, `.zlint` is added when there is no extension, and every
file is imported at most once. `import "std/rust"` loads the built-in
library:

| module | contents |
|---|---|
| `std/core` | `paren`, `brack`, `brace` (nesting), `ws`, `space`, `ident`, `number` |
| `std/rust`, `std/js`, `std/python`, `std/c`, `std/toml`, `std/zlint` | languages with brackets, strings, escapes, comments (`rs_code` etc. name all code layers) |
| `std/markdown` | code fences, inline code, HTML comments |
| `std/whitespace` | `trailing-whitespace`, `max-line-length`, `tab-indent`, `blank-lines`, `final-newline` |

Macros paste tokens: they generate zlint code, not text.

```zlint
macro rs_code_layers .rust or paren or brack or brace      # one line

macro forbid(name, pattern, why) {                          # lines, with arguments
  in rs_code match name pattern {
    rule name error why
  }
}
forbid(no_dbg, /\bdbg!/, "remove dbg! before committing")
```

Arguments are glued to the name (`forbid(`); `macro nl (...)`, with a space,
is a one-line macro whose body starts with `(`. An expansion behaves like a
parenthesised group wherever it lands, so `macro digits /\d/+` then
`digits?` means `(/\d/+)?`. Macros cannot recurse.

## Suppressing diagnostics

Write these inside any comment:

| directive | effect |
|---|---|
| `zlint-ignore` alone on a line | next line, every rule |
| `zlint-ignore` after code | this line |
| `zlint-ignore rule-a rule-b` | only these rules (`*` globs allowed, a family name covers its variants) |
| `zlint-ignore-start [rules]` ... `zlint-ignore-end` | every line in between |
| `zlint-ignore-file [rules]` | the whole file |

## Performance

`zlint --stats` prints throughput, and `zlint --profile` lists the matchers
that cost the most interpreter steps and the rules that cost the most time.
Matchers that compile to a DFA never show up there; slow ones are usually
nullable, use fields, or have no leading literal or lookaround to tell zlint
where they can start.
