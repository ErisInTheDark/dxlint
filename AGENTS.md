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
- `build.rs` embeds every file of both (`src/config/std_lib.rs` reads them); `ZLINT_LIBRARY=<checkout>` reads them from disk instead (no rebuild for .zlint edits)

## Invariants

- Rust answers questions about code; `.zlint` makes rule decisions. Native facts are properties any rule could ask (syntax, bindings, types, code paths, tokens, static values), read lazily like fields; never named after a rule, never taking rule options, never building messages. Options, what is wrong, positions, messages and fixes live in `.zlint`; shared logic there is a `fn`. Missing expressiveness: extend the DSL, not a rule-specific fact
- One owner per syntax. A native front end owns only syntax a pattern grammar cannot parse correctly (context-sensitive: regex vs division, ASI, generics, scopes, types). Regular or textual structure (lines, comments, escapes, whitespace runs, words) is a pattern layer, grafted into the native tree. Before adding native syntax, show patterns cannot own it; prefer making the pattern engine faster or more expressive
- Engine/tree queries (`descendants`, `walk`, `paired_token`...) are language-neutral; they belong to the engine, not a front end

- DFA path and interpreter must agree on semantics (looks: `regex-syntax` definitions; see `grammar/text.rs`)
- matchers never match empty; no left recursion
- memoised runs are pure per (matcher, position); `Dead` states rely on it
- new built-in expression functions (generic, value-only): one entry in `FUNCTIONS`; new facts: an entry in a `topics/*.rs` `FACTS`
- new std module or ruleset: add its file (and the docs table entry)
- std rules and their layers live in `define { }`; library layers parse only while needed (`engine/prune.rs`), so a needed layer's parents and pattern references must stay reachable
- with every rule on (`rule *`), std must build without errors (`engine/tests.rs` checks)
- native code reads the filesystem only through `native/ts/reads.rs` (self-lint enforces); shared project answers keep their `Reads` (`Cached`). Else `.zlintcache` replays stale results
- lint output depends only on file text, config tokens and recorded reads

## Scratch

- `scratch/` (gitignored) is agent workspace; `scratch/INDEX.md` lists every entry: owner, purpose, delete-when. Add the row when creating; unlisted entries are deletable
- Battle-test codebases (`wayward`, `fluff`, `transition` and derived `-z`/`-mut` copies) are permanent
- Never copy binaries or libraries: pin `target/bin/<id>.exe` + `target/build/<id>/library` by hash
- Campaigns keep summaries and changed cases, not full raw jsonl or profiles; one dir per task
- Delete only with user approval; propose a task's scratch cleanup when its work lands
