# fixtures

Rule behaviour tests. Run `dxlint fixtures [paths...]` (default `fixtures/`; `cargo test` runs the same runner). Exit 1 on any failing scenario.

## scenario
- A folder holding `.dx` is one scenario and its own project root: `.dx`, input files (nested files and project stubs allowed), `expected/`.
- Not lint inputs: config files, `expected/`, `actual/`, `.dxcache`. Lint targets follow CLI git/config ignores.
- Keep `fixtures/.ignore`: its `expected/`/`actual/` exclusions stop generated declarations becoming project globals. Roots outside `fixtures/` need the same.

## expected files
- `expected/diagnostics`: sorted reports on original inputs, `input.ts:1:1-1:4 rule-name#0`. 1-based, end exclusive (as JSON output). `#N` = 0-based source-order message leaf (`if` branches, `??` alternatives, pipeline final value; anything else one leaf); `#error` = engine/evaluation failure. Wording changes keep identity.
- Messages written in the scenario's own `.dx` (incl. library-rule overrides) also show rendered text after the identity (`\\`, `\r`, `\n` escaped). Library default messages: index only.
- `expected/fixed/<relpath>`: full fixpoint text, changed inputs only; absent = unchanged. Inputs are never rewritten.
- Optional `expected/fixed-diagnostics`: reports from re-linting fixpoint text (empty file = clean).
- Optional `expected/fix-preview/<relpath>`: one fix pass over original diagnostics (shows conflicts the fixpoint hides).
- `edits/<name>/<relpath>`: edited texts of inputs (not lint inputs). `expected/dirtied/<name>`: the other inputs, sorted, whose reads that unsaved edit breaks (what an editor re-lints); required per edit.
- `expected/suggestions`: required once any suggestion is offered. Per suggestion: report identity `> title`, then the changed lines as they read after it, placeholders in `«»`.
- A `.dxcache` marker also checks cold and warm runs agree; the runner restores its bytes.

## runs
- Every run rewrites ignored `actual/` and prints differences.
- `--bless` copies `actual/` into `expected/` (removes stale fixed files; updates `fixed-diagnostics`/`fix-preview` only where they exist, else adds previews differing from fixpoint). Bless only intentional changes; review before committing.
