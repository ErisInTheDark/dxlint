# zoomie-lint

An extremely fast linter and formatter whose rules are written in a small
language, `.zlint`. Think tree-sitter queries, but you build the tree
yourself out of regexes, and only describe the parts you care about.

```zlint
import "std/rust"

# A layer: function names, found anywhere in Rust code (never in strings
# or comments, because those are layers of their own).
in rs_code match rs_fn /\bfn[ \t]+/ name:ident

# A rule: what the text should look like. Different => diagnostic + fix.
in rs_fn.name rule fn-snake-case warn "`{text}` should be snake_case" fix snake_case(text)
```

```
$ zlint
warn: `doThing` should be snake_case [fn-snake-case] (fixable)
  --> src/lib.rs:1:4
  |
1 | fn doThing() {
  |    ^^^^^^^
```

## Why

- **Two ideas, small language.** *Matchers* build layers out of regexes, and
  *rules* say what a layer should look like. Everything else is
  configuration (`set`), reuse (`import`, `macro`) and file selection.
- **Brackets, quotes and escapes come for free.** A string is a layer whose
  escapes are child layers. A bracket group is a layer whose nested groups and
  strings are child layers. `until` skips over children whole, so
  `f(")", g(x))` closes at the right paren without any special rules.
- **Fixes can do anything.** A rule's fix is an expression: rename
  identifiers, sort lists, reorder fields, delete code, add headers. If the
  fix differs from the text, that is the diagnostic, and the fix.
- **Fast.** Pure-regex layers compile to lazy DFAs. Everything else runs on a
  memoised linear-time backtracker with prefilters, two-byte start filters and
  skip fast-forwarding, and files are linted in parallel. See
  [Performance](#performance).

## Getting started

```sh
cargo build --release          # binary: target/release/zlint
```

Put a `zoomie.zlint` at your project root. zlint finds the nearest one above
the current directory, and paths in it are relative to its directory.

```zlint
import "std/rust"
import "std/whitespace"

set max-line-length.max 100
ignore "vendor" or "src/generated/**"
```

| command | |
|---|---|
| `zlint [paths]` | lint (default: the whole project, respecting `.gitignore`) |
| `zlint --fix` | apply fixes in place |
| `zlint --format jsonl` | one JSON diagnostic per line, streamed as files finish |
| `zlint --stats` | timing and throughput |
| `zlint check` | validate the config only |
| `zlint rules` | list rules with severities, options and messages |
| `zlint tree <file>` | print the layer tree (handy while writing matchers) |
| `zlint functions` | list expression functions and variables |
| `zlint lsp` | language server on stdio |

The exit code is 1 when there are errors, and 2 when the config is broken.

## The language

The full reference is [docs/language.md](docs/language.md). The short
version:

```zlint
lang rust "*.rs" {                                  # .rust: the root layer of Rust files
  match paren                                       # lines in a block share its context
  match rs_string /"/ until /"/                     # a layer: `"`, skip, `"`
}
match paren /\(/ until /\)/
in paren {
  match paren                                       # parens nest
  match rs_string                                   # strings live in parens too
  indent                                            # lines inside parens are indented
}
in rs_string match rs_escape /\\./                  # escapes live in strings

in .rust match call name:/\w+/ args:paren           # fields: call.name, call.args
over .* match trailing_ws /[ \t]+$/ {               # overlays see all text
  rule no-trailing-ws warn "no trailing whitespace" fix ""
}

in call.args rule long-calls {
  option max 80 "longest allowed argument list"
  when len(text) > max
  warn "`{call.name}` has {len(text)} characters of arguments"
}

set long-calls.max 120                              # configure existing rules
set long-calls error
```

Layers can be specialised (`match js_object like brace ...` wins ties against
plain braces and is still a `brace` to rules), collected into open groups
(`group js_code js_stmt` adds to `js_code`), and declared as indenting, so an
indentation rule only decides units and continuation lines. Regexes support
lookahead and lookbehind, and rule expressions have line context
(`prev_line`, `parent_before`, `indent_anchor`, ...) and `re_*` functions.

A config with mistakes still lints with every part that built: the errors
are reported, and the run fails, but one typo never switches linting off.

The standard library ships languages (`std/rust`, `std/js`, `std/python`,
`std/c`, `std/toml`, `std/markdown`, `std/zlint`), bracket nesting
(`std/core`), and hygiene rules (`std/whitespace`). See
[examples/](examples) for complete configs, and [zoomie.zlint](zoomie.zlint)
for the rules this repository lints itself with.

Suppress with a comment anywhere: `zlint-ignore`, `zlint-ignore rule-name`,
`zlint-ignore-start` ... `zlint-ignore-end`, or `zlint-ignore-file`.

## Editors

`zlint lsp` speaks the Language Server Protocol:

- open files are linted on every change, from the unsaved buffer
- the rest of the workspace is scanned in the background, and each file's
  results are published the moment it finishes, with progress reporting
- quick fixes per diagnostic, "fix all" as `source.fixAll.zlint`, and document
  formatting that applies every fix
- `.zlint` files reload live, and config errors appear in the `.zlint` file
  they point at

A VS Code extension (client + `.zlint` syntax highlighting) is in
[editors/vscode](editors/vscode).

## Performance

Release build, 16 threads, files from this repository's own sources:

| workload | time | throughput |
|---|---|---|
| 1,521 files (8.7 MiB), full `zoomie.zlint` | ~72 ms | ~120 MiB/s |
| one 27 MiB file, Rust structure (strings, comments, brackets) + a rule | ~640 ms | ~42 MiB/s |
| one 27 MiB file, `std/whitespace` | ~490 ms | ~55 MiB/s |
| one 27 MiB file, full `zoomie.zlint` | ~1.2 s | ~23 MiB/s |

A single file is parsed on one thread. Files are linted in parallel.

Where the speed comes from:

1. **DFAs for pure regex.** Matchers without references or `*` compile to a
   `regex-automata` lazy DFA, overlay searches use its SIMD prefilters, and
   impure matchers get a DFA *guard* for their leading regex part.
2. **Memoised, linear backtracking.** The interpreter explores each
   (instruction, position) state at most once per run. States a matcher
   failed from are remembered for the whole file, so an unclosed `(` cannot
   cause rescans. Calls are memoised per (matcher, position).
3. **Cheap rejection.** First-byte sets, a 64K-bit table of possible first
   two bytes, and `*` fast-forwarding to the next byte where anything could
   start.
4. **Only what is needed.** Overlays no rule looks at are never searched,
   expression values borrow from the source instead of copying, and warm DFA
   caches are pooled across files.

## Layout

| path | |
|---|---|
| `src/config` | lexer, macro/import token stream, parser, embedded std |
| `src/pattern` | pattern AST, regex parser, compiler, DFA bridge, prefilters |
| `src/grammar` | layers per language, the parser that builds trees |
| `src/expr` | rule expressions, values, functions |
| `src/engine` | the linter: rules, fixes, suppressions |
| `src/cli`, `src/lsp` | the binary |
| `std/` | the standard library (`import "std/..."`) |
| `editors/vscode` | VS Code extension |
