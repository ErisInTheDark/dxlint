# dxlint

Rust linter; rules in `.dx` (docs/advanced.md). Binary `dxlint`.

## Commands

- test: `cargo test` (stable Rust owners only: VM, parser, patterns/DFA, cache/reads, type APIs, CLI/LSP; rule behaviour lives in fixtures)
- rule fixtures: `dxlint fixtures [paths...] [--bless]` (default `fixtures/`)
- package binaries: `npm run build:binaries`; any change to `src/` or Cargo files ships with it in the same batch (publishing refuses binaries whose `package/source.sha256` stamp is stale)
- self-lint: `cargo run --release --` (uses `.dx`); must stay clean
- perf check: `dxlint --stats --no-cache <big file>`; compare before/after engine changes
- CPU profile (Windows, no admin): env `DXLINT_SAMPLE=<report>` on a build with debug line tables; `DXLINT_SAMPLE_CALLERS=a;>b` adds callers of `a`, callees of `b`. Budget steps are not time: profile before trusting them

## Layout

- `src/config`: lexer, token stream (imports + macros splice tokens), parser
- `src/pattern`: `Pat`, regex subset parser, compiler, DFA bridge, prefilters
- `src/grammar`: layers per language, `parse.rs` builds trees
- `src/expr`: rule expressions; functions registry in `functions.rs`
- `src/engine`: linter, fixes, suppressions, `.dxcache` (`cache.rs`), symbol index; end-to-end tests in `engine/tests.rs`
- `src/engine/frontend.rs`: the `Frontend` trait, the engine's only view of a native front end (kinds/fields, parse, per-file facts, project state); `lib.rs` `FRONTENDS` registers them
- `src/native/ts`: TypeScript front end, syntax and tokens only (`frontend.rs` implements `Frontend`); programs, modules and globals belong to the engine (`engine/world.rs`, `engine/program.rs`)
- `src/native/ts/topics/<family>.rs`: native facts grouped by family (`FACTS` list, read via `Facts::with_file`; whole-file results via `NativeFile::cached`); legacy home of rule logic being moved to `std/ts/<family>.dx`
- `std/`: built-in languages (`import .ts`); matchers, plus rule definitions in `define { }` (off until `rule name`), topic files under `std/<lang>/`
- `rules/`: built-in rulesets (`import rules:x`): only `rule name` lines
- `docs/`: user docs: basic.md, advanced.md (language), ts.md, rules/ (one page per rule; `npm run docs:rules` rebuilds rules/README.md)
- `package/`: the npm package `@inthedark/dxlint` (launcher + committed binaries, `npm run build:binaries`); `.github/workflows/` publish it and the VS Code extension (`editors/vscode`, `npm run package:vsix`)
- nothing is compiled in: `src/config/std_lib.rs` reads both from `DXLINT_LIBRARY=<dir>`, else the nearest ancestor of the executable holding `std/` and `rules/` (no rebuild for .dx edits; pinned binaries need `DXLINT_LIBRARY`)

## Invariants

- Rule behaviour tests: scenario folders under `fixtures/` (fixtures/AGENTS.md).

- `.dx` defines languages and rules; Rust is the language-neutral engine that runs them. Syntax, names, types, calls, modules, effects and narrowing are declared in `std/<lang>/` (docs/ts.md); rule options, what is wrong, positions, messages and fixes live in rule `.dx` (shared logic as `fn`)
- The engine knows no language and no rule: nothing in Rust is named after a rule, takes rule options or builds messages. The TS native front end is being migrated out: never add native TS semantics; native TS code only shrinks. Missing expressiveness: extend the engine/DSL generically
- One owner per syntax. Regular or textual structure (lines, comments, escapes, whitespace runs, words) is a pattern layer
- Engine/tree queries (`descendants`, `walk`, `paired_token`...) are language-neutral; they belong to the engine, not a front end

- DFA path and interpreter must agree on semantics (looks: `regex-syntax` definitions; see `grammar/text.rs`)
- matchers never match empty; no left recursion
- memoised runs are pure per (matcher, position); `Dead` states rely on it
- new built-in expression functions (generic, value-only): one entry in `FUNCTIONS`; new facts: an entry in a `topics/*.rs` `FACTS`
- new std module or ruleset: add its file (and the docs table entry)
- std rules and their layers live in `define { }`; library layers parse only while needed (`engine/prune.rs`), so a needed layer's parents and pattern references must stay reachable
- with every rule on (`rule *`), std must build without errors (`engine/tests.rs` checks)
- native code reads the filesystem only through `engine/reads.rs` (self-lint enforces); shared project answers keep their `Reads` (`Cached`); directory searches go through a registered `Discovery`. Else `.dxcache` replays stale results
- lint output depends only on file text, config tokens and recorded reads

## Scratch

- Orchestrator (parent agent): after compaction read `scratch/lily-process.md`, then thread recall, before any other action

- `scratch/` (gitignored) is agent workspace; `scratch/INDEX.md` lists every entry: owner, purpose, delete-when. Add the row when creating; unlisted entries are deletable
- Battle-test codebases (`wayward`, `fluff`, `transition` and derived `-z`/`-mut` copies) and the conformance corpus (`scratch/corpus/`) are permanent
- Keep and disposable never share a directory: permanent data gets its own top-level entry; never ask anyone to delete only part of a folder
- Never copy binaries or libraries: pin `target/bin/<id>.exe` + `target/build/<id>/library` by hash
- Campaigns keep summaries and changed cases, not full raw jsonl or profiles; one dir per task
- Never create shapes that are slow to delete: no per-case file trees or deep copies; pack many small items into one file
- Agents own cleanup: when a task lands, its owner deletes that task's scratch entries and INDEX rows. Never delete permanent entries or another agent's live task
