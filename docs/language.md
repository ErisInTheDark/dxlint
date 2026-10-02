# The `.zlint` language

A `.zlint` file describes **layers** (what the text is made of) and
**formatters** (what the text should look like). zlint parses every file into
a tree of layers using only fast regex-style matching, then runs formatters
on the nodes they target.

```zlint
import "std/rust"

in rs_code match rs_fn = /\bfn[ \t]+/ name:ident
format fn-snake-case on rs_fn.name "function names are snake_case" = snake_case(text)
```

- [Items](#items)
- [Layers](#layers)
- [Patterns](#patterns)
- [Formatters](#formatters)
- [Expressions](#expressions)
- [Configuring rules: `set`](#configuring-rules-set)
- [Imports and macros](#imports-and-macros)
- [Suppressing diagnostics](#suppressing-diagnostics)

## Items

| item | meaning |
|---|---|
| `lang rust = "*.rs" \| "*.rs.in"` | a language: the root layer `.rust` for files matching the globs |
| `match name = pattern` | a matcher (layer) that is only used where something references it |
| `in A \| B match name = pattern` | a matcher searched inside layers `A` and `B` |
| `in A match name` | attach an existing matcher to more parents |
| `over A match name = pattern` | an overlay: searched over `A`'s text without consuming it |
| `format name on target ...` | a formatter, see [Formatters](#formatters) |
| `set name.key = value` | configure an existing formatter |
| `ignore "glob" \| "glob"` | never lint matching files |
| `import "path"` | paste another `.zlint` file here |
| `macro name(args) { tokens }` | define a macro |

`#` starts a comment. Items end at the end of the line (or `;`).

Globs without a `/` match any single path component (`*.rs`, `target`), and
globs with a `/` match the path relative to the config file (`src/**/*.rs`,
`tests/fixtures/**`).

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

**Parents.** `in A | B match x` makes `x` a child of `A` and `B`. A parent is
a matcher name, `.lang` (a language root) or `.*` (every language root).

**Searching.** A layer looks for its children across its text. Anything that
no child matches is skipped, so you never have to describe a whole language,
only the parts you care about. A language root does this over the whole file.

**`*` (skip).** Inside a pattern, `*` lazily skips text until the rest of the
pattern can match, and while skipping, the layer's children eat whole chunks:

```zlint
match paren = /\(/ * /\)/
in paren match paren                   # nested parens are eaten whole
in .rust | paren match rs_string = /"/ * /"/
in rs_string match rs_escape = /\\./   # so `\"` never ends a string
```

`f(")", g(x))` is one `paren`: the `")"` is eaten by `rs_string`, and `(x)` by
the nested `paren`, so the outer `\)` only matches the real closing bracket.
This is how brackets, quotes and escapes work without special support. They
are just layers.

A matcher without `*` searches for its children across its own text after it
matches, skipping its fields, the same way a language root does.

**Ties.** When several children match at the same position, the longest match
wins, and among equal lengths the one attached first wins.

**Language gating.** A matcher attached to a `.lang` only runs inside files
of the languages it is attached to (or attached through `.*`). That lets
shared layers like `paren` carry Rust strings in Rust files and JavaScript
strings in JavaScript files.

**Overlays.** `over A match x = ...` searches `A`'s text for `x` without
taking that text away from `A`'s other children. Use overlays for
line-oriented checks:

```zlint
over .* match trailing_ws = /[ \t]+$/
```

**Fields.** Labelled pieces of a match become fields:

```zlint
in .rust match call = name:ident args:paren   # call.name, call.args
in .rust match call2 = ident :paren           # `:paren` is short for `paren:paren`
```

A label on a matcher reference makes that child node the field. A label on
regex text makes a plain text node. A label that matches several times
(inside a repetition) is a list.

**Restrictions.** A matcher must always consume at least one character, and
it may only reach itself again after consuming text (no left recursion).
Both are reported when the config loads.

## Patterns

A pattern is a sequence of items on one line. Newlines are allowed inside
`( ... )`.

| item | meaning |
|---|---|
| `/regex/` | a regex (the subset below) |
| `"text"` | literal text |
| `name` | another matcher, as a child node |
| `label:item` | label an item as a field |
| `:name` | `name:name` |
| `*` | skip, see [Layers](#layers) |
| `( a b \| c )` | grouping and alternation |
| `item?` `item*` `item+` | repetition, glued to the item; add `?` for lazy (`item*?`) |

### The regex subset

Only constructs that run in linear time are supported, so every matcher runs
in time proportional to the text no matter how it is written.

| syntax | meaning |
|---|---|
| `abc`, `\.`, `\/` | literals (escape `/` inside a regex) |
| `.` | any character except `\n` |
| `[a-z_]`, `[^"\\]` | classes, with ranges, negation and escapes |
| `\d \D \w \W \s \S` | digit, word, space classes (unicode-aware) |
| `^` `$` | start and end of a line (`$` also sits before `\r\n`) |
| `\A` `\z` | start and end of the text |
| `\b` `\B` | word boundary and non-boundary |
| `a\|b`, `(...)`, `(?:...)` | alternation and groups |
| `(?<name>...)` | a labelled field, like `name:` |
| `* + ? {n} {n,} {n,m}` | repetition, add `?` for lazy |
| `(?i)` at the start | ASCII case-insensitive |
| `\n \t \r \xHH \u{HHHH}` | escapes |

Backreferences and lookaround are not supported.

## Formatters

```zlint
format name on target "description" = shape
format name on target "description" {
  option max = 100 "what max means"
  severity error
  files "src/**"
  ignore "src/generated/**"
  when len(text) > max
  message "line is {len(text)} long"
  shape-expression
}
```

**Targets.** `on m` targets every node of matcher `m`. `on m.field` targets a
field of `m`. `on .lang` targets whole files of a language, and `on .*`
targets whole files of every language.

**Shape.** The bare expression in the body (or after `=`) says what the node
*should* be. If its text differs from the node's text, the formatter reports
the node and offers the shape as the fix. If they are equal, nothing happens.
Shapes can be anything: reorder, rename, delete (`""`), add text.

A formatter without a shape reports every node that passes its `when`
conditions.

**Body clauses.**

| clause | meaning |
|---|---|
| `option name = value "description"` | a configurable value, see [`set`](#configuring-rules-set) |
| `severity off\|hint\|info\|warn\|error` | default severity (`warn`) |
| `when expr` | only nodes where `expr` is truthy; several `when`s must all hold |
| `message expr` | the diagnostic message |
| `files "glob" \| ...` | only these files |
| `ignore "glob" \| ...` | not these files |

The description is the default message, and can use everything a message can:
`"{text} left in a comment"`. `zlint rules` shows descriptions with option
values filled in and other holes left as written.

Fixes that overlap are applied over several passes (re-linting in between),
so nested formatters compose.

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
| `if c then a else b` | conditional |

**Values** are `null`, booleans, numbers, text and lists. A field that did not
match is `null`, and a field that matched several times is a list. `null`,
`false`, `0`, `""` and `[]` are falsy.

**Variables.**

| name | value |
|---|---|
| `text` | the node's text |
| `indent` | leading whitespace of the node's first line |
| `line`, `col` | 1-based position of the node |
| `file` | path relative to the config |
| `lang` | the file's language |

**Functions.** `zlint functions` lists them all. Highlights: `len`, `lower`,
`upper`, `snake_case`, `camel_case`, `pascal_case`, `kebab_case`,
`screaming_case`, `title_case`, `trim`, `trim_start`, `trim_end`, `replace`,
`repeat`, `pad_start`, `pad_end`, `starts_with`, `ends_with`, `contains`,
`split`, `lines`, `join`, `sort`, `unique`, `reverse`, `first`, `last`,
`count`, `slice`, `str`, `num`, `default`, `min`, `max`, `abs`, `round`.

## Configuring rules: `set`

```zlint
set max-line-length.max = 100
set max-line-length.severity = error
set style-*.severity = off           # `*` globs over formatter names
```

Option values must be constants of the same type as the option's default.
`severity` can be set on every formatter.

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
macro rs_code = .rust | paren | brack | brace       # one line, no arguments

macro forbid(name, pattern, why) {                  # block with arguments
  in rs_code match name = pattern
  format name on name why { severity error }
}
forbid(no_dbg, /\bdbg!/, "remove dbg! before committing")
```

An expansion behaves like a parenthesised group wherever it lands, so
`macro digits = /\d/+` then `digits?` means `(/\d/+)?`. Macros cannot
recurse.

## Suppressing diagnostics

Write these inside any comment:

| directive | effect |
|---|---|
| `zlint-ignore` alone on a line | next line, every rule |
| `zlint-ignore` after code | this line |
| `zlint-ignore rule-a rule-b` | only these rules (`*` globs allowed) |
| `zlint-ignore-file [rules]` | the whole file |
