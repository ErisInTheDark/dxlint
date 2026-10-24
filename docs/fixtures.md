# Rule fixtures

Run `dxlint fixtures` (all `fixtures/`) or `dxlint fixtures <paths...>`. A scenario is a folder containing `.dx`; it is its own on-disk project root. Inputs may include nested files and project dependencies. Config files, `expected/`, `actual/` and `.dxcache` are not lint inputs.

`expected/diagnostics` contains sorted original-input reports:

```
input.ts:1:1-1:4 rule-name#0
```

Lines/columns are 1-based, end positions exclusive, matching JSON output. Message indexes are 0-based source-order leaves: `if` branches, `??` alternatives and a pipeline's final value. Other expressions (including map results) have one leaf. Engine/evaluation failures use `#error`. Changing message wording does not change its identity.

Messages written in the scenario's own `.dx`, including library-rule overrides, also include rendered text after the identity; backslashes, CR and newlines escape as `\\`, `\r`, `\n`. Library default messages remain index-only. Without an explicit message, the rule name's source owns its fallback.

`expected/fixed/<relpath>` contains complete fixpoint text only for changed inputs; absence means unchanged. Inputs are never rewritten. A `.dxcache` marker additionally checks cold/warm diagnostics agree; the runner restores its original bytes.

Every run replaces ignored `actual/diagnostics` and `actual/fixed/`, prints scenario differences, and exits 1 on failures. `--bless` copies actual results into `expected/`, removing stale fixed files. Review changes before committing. `cargo test` runs the same scenario runner.

`actual/fixed-diagnostics` records reports from re-linting fixpoint text. Compare it by adding optional `expected/fixed-diagnostics` (an empty file asserts clean); bless updates it only when present. Lint targets follow the CLI's git/config ignores; project stubs remain available on disk.

`actual/fix-preview/<relpath>` records one fix pass over original diagnostics for files with fixes. Optional `expected/fix-preview/<relpath>` compares that first pass, exposing conflicts hidden by the final fixpoint. Bless updates all preview files when the expected directory already exists; otherwise it adds only previews differing from fixpoint text.

Keep `fixtures/.ignore`: its `expected/` and `actual/` exclusions also prevent generated TypeScript declarations becoming project globals. For roots outside `fixtures/`, provide equivalent `.ignore` entries.
