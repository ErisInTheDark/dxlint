# dxlint

Rust linter; rules in `.dx` (docs/advanced.md). Binary `dxlint`.

## Commands

- check a change: `npm run check [-- fixture paths...]` (one compile: builds the test binary, then runs tests, fixtures and self-lint from it; never build twice to validate)
- test: `cargo test --lib` (plain `cargo test` compiles the crate a second time for a bin with no tests; stable Rust owners only: VM, parser, patterns/DFA, cache/reads, type APIs, CLI/LSP; rule behaviour lives in fixtures)
- rule fixtures: `dxlint fixtures [paths...] [--bless]` (default `fixtures/`)
- package binaries: `npm run build:binaries` (Windows, Linux, macOS; setup in its header); any change to `src/` or Cargo files ships with it in the same batch (publishing refuses binaries whose `package/source.sha256` stamp is stale)
- Windows binary's PGO profile: `npm run build:profile <project>...` (needs `rustup component add llvm-tools-preview`; lints real projects on an instrumented build), commit `scripts/dxlint-win32-x64.profdata`, then `build:binaries`; refresh when big engine changes landed, a stale one only gains less
- self-lint: `cargo run --release --` (uses `.dx`); must stay clean
- perf check: `dxlint --stats --no-cache <big file>`; compare before/after engine changes
- declared-parser profile: env `DXLINT_SYNTAX_PHASES=1` with `--stats` prints parse phase times and work counts (scratch/tools/zbench.ps1)
- CPU profile (Windows, no admin): env `DXLINT_SAMPLE=<report>` on a build with debug line tables; `DXLINT_SAMPLE_CALLERS=a;>b` adds callers of `a`, callees of `b`; `DXLINT_SAMPLE_AFTER=<s>` skips startup (a server's scan). Budget steps are not time: profile before trusting them
- work census: `cargo build --release --features work`, then `--no-cache --jobs 1`: a work count that repeats exactly on a busy machine, each fact's own work with exact repeats, rules and `let`s by own work and own time, which sees native loops (budget steps by source line, facts by own steps; `DXLINT_WORK_SITES=1` adds each expression, slow; `DXLINT_WORK_FACT_SITES=<steps>` adds where facts that heavy spent theirs; scratch/tools/work-run.ps1; scratch/tools/quick.ps1 = same on 139 fixed files in seconds, for first answers). The work count sees native loops only through what they call: confirm wins in CPU (scratch/tools/cpu-ab.ps1)
- memory: env `DXLINT_HEAP_STATS=1` prints live heap by owner and where allocator pages sit (CLI end, LSP shutdown; LSP adds kept reads by dep kind); add `DXLINT_HEAP_SITES=1` (Windows, debug line tables) to list large blocks by call stack, `DXLINT_HEAP_FILES=1` to print each linted file's peak and final heap (stderr)
- cross-file deps (`crates/dx-core/src/parts.rs`): env `DXLINT_RELINT_CHECK=1` makes the language server lint afresh every file an edit's dep check kept, and print any that differ (slow; replays: scratch/lsp-load/hub.mjs)

## Layout

- `src/config`: lexer, token stream (imports + macros splice tokens), parser
- workspace: lower layers are crates under `crates/`, re-exported in `src/lib.rs` under their old module names (`crate::pattern::...` still works); a crate never depends upward
- `crates/dx-pattern` (`pattern`): `Pat`, regex subset parser, compiler, DFA bridge, prefilters
- `crates/dx-core`: hashing, path globs, allocator, levers, sampler, work counts, file parts, `reads` (also `engine::reads`)
- `crates/dx-tree`: `Tree`, node types and path steps, `footprint` (recorded reads), `line_index`
- `crates/dx-ir`: compiled expression and VM value types (`CExpr`, `VExpr`, `Value`, `V`, record cells, builtin/function metadata); no evaluation
- `src/grammar`: layers per language, `parse.rs` builds trees
- `src/expr`: rule expressions; functions registry in `functions.rs`
- `src/engine`: linter, fixes, suppressions, `.dxcache` (`cache.rs`), symbol index; end-to-end tests in `engine/tests.rs`
- `src/engine/frontend.rs`: the `Frontend` trait, the engine's only view of a front end (kinds/fields, parse, per-file tokens and names)
- `src/grammar/syntax`: declared parsers (`parser x in lang`, `extend parser`): compile to a program, run by `run/descend.rs` (valid input) and `run/execute.rs` (diagnosing, deep nesting); `frontend.rs` makes one a language's front end (`native x`). `std/ts/grammar.dx` is TypeScript; parse bench `scratch/tools/zbench.ps1`
- `src/engine/transform.rs`, `src/cli/map.rs`: `transform`s applied strictly once, served by `dxlint map` (TS content mapper protocol, `package/esbuild.mjs`); output depends only on the file's text and config (TS keys its cache that way); worked example `fixtures/engine/transform-state`
- `std/`: built-in languages (`import .ts`); matchers, plus rule definitions in `define { }` (off until `rule name`), topic files under `std/<lang>/`
- `rules/`: built-in rulesets (`import rules:x`): only `rule name` lines
- `docs/`: user docs: basic.md, advanced.md (language), ts.md, rules/ (one page per rule; `npm run docs:rules` rebuilds rules/README.md)
- `package/`: the npm package `@inthedark/dxlint` (launcher + committed binaries, `npm run build:binaries`); `.github/workflows/` publish it and the VS Code extension (`editors/vscode`, `npm run package:vsix`)
- nothing is compiled in: `src/config/std_lib.rs` reads both from `DXLINT_LIBRARY=<dir>`, else the nearest ancestor of the executable holding `std/` and `rules/` (no rebuild for .dx edits; pinned binaries need `DXLINT_LIBRARY`)

## Invariants

- Rule behaviour tests: scenario folders under `fixtures/` (fixtures/AGENTS.md).

- `.dx` defines languages and rules; Rust is the language-neutral engine that runs them. Syntax, names, types, calls, modules, effects and narrowing are declared in `std/<lang>/` (docs/ts.md); rule options, what is wrong, positions, messages and fixes live in rule `.dx` (shared logic as `fn`)
- No language in Rust: the engine knows no language and no rule. Nothing in Rust is named after a language or rule, parses a language, takes rule options or builds messages; syntax is a declared parser or pattern layers. Missing expressiveness: extend the engine/DSL generically
- One owner per syntax. Regular or textual structure (lines, comments, escapes, whitespace runs, words) is a pattern layer
- Engine/tree queries (`descendants`, `walk`, `paired_token`...) are language-neutral; they belong to the engine, not a front end

- DFA path and interpreter must agree on semantics (looks: `regex-syntax` definitions; see `grammar/text.rs`)
- matchers never match empty; no left recursion
- memoised runs are pure per (matcher, position); `Dead` states rely on it
- new built-in expression functions (generic, value-only): one entry in `FUNCTIONS`
- new std module or ruleset: add its file (and the docs table entry)
- std rules and their layers live in `define { }`; library layers parse only while needed (`engine/prune.rs`), so a needed layer's parents and pattern references must stay reachable
- with every rule on (`rule *`), std must build without errors (`engine/tests.rs` checks)
- Rust reads the filesystem only through `crates/dx-core/src/reads.rs` (self-lint enforces); shared project answers keep their `Reads` (`Cached`); directory searches go through a registered `Discovery`. Else `.dxcache` replays stale results
- lint output depends only on file text, config tokens and recorded reads
- units (other files' views) have no nodes inside `opaque` bodies (`engine/units.rs`): a language's `opaque` item promises no rule or fact needs them from another file

## Scratch

- Orchestrator (parent agent): after compaction read `scratch/lily-process.md`, then thread recall, before any other action

- `scratch/` (gitignored) is agent workspace; `scratch/INDEX.md` lists every entry: owner, purpose, delete-when. Add the row when creating; unlisted entries are deletable
- Battle-test codebases (`wayward`, `fluff`, `transition` and derived `-z`/`-mut` copies) and the conformance corpus (`scratch/corpus/`) are permanent
- Keep and disposable never share a directory: permanent data gets its own top-level entry; never ask anyone to delete only part of a folder
- Never copy binaries or libraries: pin `target/bin/<id>.exe` + `target/build/<id>/library` by hash
- Campaigns keep summaries and changed cases, not full raw jsonl or profiles; one dir per task
- Never create shapes that are slow to delete: no per-case file trees or deep copies; pack many small items into one file
- Agents own cleanup: when a task lands, its owner deletes that task's scratch entries and INDEX rows. Never delete permanent entries or another agent's live task
