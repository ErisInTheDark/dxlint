# zoomie-lint

An extremely fast linter and formatter whose rules are written in a small
language, `.zlint`. Think tree-sitter queries, but you build the tree
yourself out of regexes, and only describe the parts you care about.

```zlint
import .rs

# A layer: function names, found anywhere in Rust code (never in strings
# or comments, because those are layers of their own).
in rs_code match rs_function /\bfn[ \t]+/ name:identifier

# A rule: what the text should look like. Different => diagnostic + fix.
in rs_function.name rule fn-snake-case warn "`{text}` should be snake_case" fix as snake_case(text)
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
  configuration (`rule name ...`), reuse (`import`, `macro`) and file
  selection.
- **Built-in rules, all yours to pick.** Importing a language (`import .ts`)
  brings every rule written for it, off: turn on exactly the ones you want
  (`rule no-cond-assign`), or a whole ruleset (`import rules:recommended`)
  and take some back. Options take records and lists, so eslint's richer
  options carry over. `override` or `drop` anything an import defines;
  anything left depending on a dropped name is reported at the `drop`.
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

Put a `.zlint` at your project root. zlint finds the nearest one above the
current directory, and paths in it are relative to its directory.

```zlint
import .ts
import rules:recommended

rule eqeqeq error
rule max-line-length {
  set max 100
}
ignore "vendor" "src/generated/**"
```

| command | |
|---|---|
| `zlint [paths]` | lint (default: the whole project, respecting `.gitignore`) |
| `zlint --fix` | apply fixes in place |
| `zlint --format jsonl` | one JSON diagnostic per line, streamed as files finish |
| `zlint --stats` | timing and throughput (and how many files came from `.zlintcache`) |
| `zlint --no-cache` | lint every file, ignoring `.zlintcache` |
| `zlint check` | validate the config only |
| `zlint rules` | list every defined rule (off ones too) with severities, options and messages |
| `zlint describe <name>` | explain any name in the config (rule, layer, group, attribute, macro, fact...) and where it is used |
| `zlint tree <file>` | print the layer tree (handy while writing matchers) |
| `zlint functions` | list expression functions and variables |
| `zlint lsp` | language server on stdio |

The exit code is 1 when there are errors, and 2 when the config is broken.
`-c <config>` picks a config explicitly, before or after a subcommand.

Results are cached in `.zlintcache` beside the config (add it to
`.gitignore`): a file is linted again only when its text, the rules, or
something its lint read from another file (an imported declaration, module
resolution, a tsconfig) changed.

## The language

The full reference is [docs/language.md](docs/language.md). The short
version:

```zlint
lang rust "*.rs" {                                  # .rust: the root layer of Rust files
  match parentheses                                 # lines in a block share its context
  match rs_string /"/ until /"/                     # a layer: `"`, skip, `"`
}
match parentheses /\(/ until /\)/
in parentheses {
  match parentheses                                 # parentheses nest
  match rs_string                                   # strings live in them too
}
in rs_string match rs_escape /\\./                  # escapes live in strings

in .rust match call name:/\w+/ arguments:parentheses   # fields: call.name, call.arguments
in .* layer text match trailing_whitespace /[ \t]+$/ {   # named layers see all text
  rule no-trailing-whitespace warn "no trailing whitespace" fix as ""
}
in call.name rule loud-calls warn "shout it" fix as upper(text) "!"   # values side by side join

in call.arguments rule long-calls {
  option max 80 "longest allowed argument list"
  when len(text) > max
  warn "`{call.name}` has {len(text)} characters of arguments"
}

rule long-calls error {                             # configure existing rules
  set max 120
}
```

Layers can be specialised (`match js_object like braces ...` wins ties against
plain braces and is still `braces` to rules), collected into open groups
(`group js_code js_statement` adds to `js_code`, even for rules written
earlier). An indentation rule declares what it indents (`indent in braces`,
`indent ts_switch.case by SwitchCase`) and only decides units and
continuation lines (a mis-indented block opener is one report: the lines
inside are measured from where it will be once fixed). Regexes support
lookahead and lookbehind, and rule expressions have line context
(`previous_line`, `parent_before`, `indent_anchor`, ...), neighbours
(`children.x`, `previous.x`, `next.x`), layer tests (`parent is [ts_block,
ts_program]`), rule-local `let`s and `re_*` functions.

For TypeScript and JavaScript, `import .ts` adds zlint's own parser:
the real syntax tree as `ts_*` layers, scopes (`uses`, `value_uses`),
cross-file facts (`declaration_doc`) and declared types (`type`,
`type_members`) read from your tsconfig, `lib` and `@types` files, without
running TypeScript itself. Computed attributes (`in ts_logical let operators
1 + sum(children.operators)`, with `in ts_parenthesized forward operators
from expression` for wrappers) fold trees for rules like "conditions with
more than two `&&`/`||`".

A config with mistakes still lints with every part that built: the errors
are reported, and the run fails, but one typo never switches linting off.

The built-in library ships languages (`.rs`, `.ts` for JS and TS, `.py`, `.c`,
`.toml`, `.md`, `.zlint`) with their rules (ports of eslint, @stylistic and
typescript-eslint rules for JS/TS, topiary-style layout for Rust, and
language-neutral hygiene), and rulesets (`rules:correctness`,
`rules:suspicious`, `rules:conventions`, `rules:format`, `rules:types`,
`rules:recommended`). See [examples/](examples) for complete configs, and
[.zlint](.zlint) for the rules this repository lints itself with.

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
  they point at, with links to related places
- in `.zlint` files: hover on any name, go to definition (into std too),
  find all references, completion and an outline; diagnostic codes link to
  their rule
- an edit re-lints the other files it affects (an importer seeing a changed
  export), open or not, and the scan reuses `.zlintcache`

A VS Code extension (client + `.zlint` syntax highlighting) is in
[editors/vscode](editors/vscode).

## Performance

Release build, 16 threads, files from this repository's own sources:

| workload | time | throughput |
|---|---|---|
| 1,521 files (8.7 MiB), full self-lint config | ~72 ms | ~120 MiB/s |
| one 27 MiB file, Rust structure (strings, comments, brackets) + a rule | ~640 ms | ~42 MiB/s |
| one 27 MiB file, the whitespace rules | ~490 ms | ~55 MiB/s |
| one 27 MiB file, full self-lint config | ~1.2 s | ~23 MiB/s |

A single file is parsed on one thread. Files are linted in parallel; large native files can also run independent rule groups in parallel. `--jobs N` limits the shared file/rule worker budget and directory traversal threads. It defaults to available CPUs; use `--jobs 1` for serial linting.

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
4. **Only what is needed.** Named layers no rule looks at are never searched,
   built-in layers only rules that are off would need are not built at all,
   expression values borrow from the source instead of copying, and warm DFA
   caches are pooled across files.

## Layout

| path | |
|---|---|
| `src/config` | lexer, macro/import token stream, parser, embedded std |
| `src/pattern` | pattern AST, regex parser, compiler, DFA bridge, prefilters |
| `src/grammar` | layers per language, the parser that builds trees |
| `src/expr` | rule expressions, values, functions |
| `src/engine` | the linter: rules, configuring them, pruning unused library layers, fixes, suppressions, `.zlintcache`, the symbol index behind `describe` and hover |
| `src/native` | the TypeScript front end: parser, scopes, types, the project's files |
| `src/cli`, `src/lsp` | the binary |
| `std/` | built-in languages and their rule definitions (`import .ts`) |
| `rules/` | built-in rulesets (`import rules:recommended`) |
| `editors/vscode` | VS Code extension |
