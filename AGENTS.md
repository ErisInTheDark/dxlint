# zoomie-lint

Rust linter; rules in `.zlint` (docs/language.md). Binary `zlint`.

## Commands

- test: `cargo test` (unit tests colocated; `tests/cli.rs`, `tests/lsp.rs` drive the binary)
- self-lint: `cargo run --release --` (uses `.zlint`); must stay clean
- perf check: `zlint --stats --no-cache <big file>`; compare before/after engine changes

## Layout

- `src/config`: lexer, token stream (imports + macros splice tokens), parser
- `src/pattern`: `Pat`, regex subset parser, compiler, DFA bridge, prefilters
- `src/grammar`: layers per language, `parse.rs` builds trees
- `src/expr`: rule expressions; functions registry in `functions.rs`
- `src/engine`: linter, fixes, suppressions, `.zlintcache` (`cache.rs`), symbol index; end-to-end tests in `engine/tests.rs`
- `src/native/ts`: TypeScript front end; `modules.rs` project (summaries, resolution, globals)
- `src/native/ts/topics/<family>.rs`: native facts grouped by family (`FACTS` list, read via `Facts::with_file`; whole-file results via `NativeFile::cached`); legacy home of rule logic being moved to `std/ts/<family>.zlint`
- `std/`: built-in languages (`import .ts`); matchers, plus rule definitions in `define { }` (off until `rule name`), topic files under `std/<lang>/`
- `rules/`: built-in rulesets (`import rules:x`): only `rule name` lines
- `src/config/std_lib.rs` lists both

## Invariants

- Rust answers questions about code; `.zlint` makes rule decisions. Native facts are properties any rule could ask (syntax, bindings, types, code paths, tokens, lines, comments, static values), read lazily like fields; never named after a rule, never taking rule options, never building messages. Options, what is wrong, positions, messages and fixes live in `.zlint`; shared logic there is a `fn`. Missing expressiveness: extend the DSL, not a rule-specific fact

- DFA path and interpreter must agree on semantics (looks: `regex-syntax` definitions; see `grammar/text.rs`)
- matchers never match empty; no left recursion
- memoised runs are pure per (matcher, position); `Dead` states rely on it
- new built-in expression functions (generic, value-only): one entry in `FUNCTIONS`; new facts: an entry in a `topics/*.rs` `FACTS`
- new std module or ruleset: add to `std_lib.rs` and docs table
- std rules and their layers live in `define { }`; library layers parse only while needed (`engine/prune.rs`), so a needed layer's parents and pattern references must stay reachable
- with every rule on (`rule *`), std must build without errors (`engine/tests.rs` checks)
- native code reads the filesystem only through `native/ts/reads.rs` (self-lint enforces); shared project answers keep their `Reads` (`Cached`). Else `.zlintcache` replays stale results
- lint output depends only on file text, config tokens and recorded reads
