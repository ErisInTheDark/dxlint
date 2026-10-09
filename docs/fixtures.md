# Rule fixtures

Run `zlint fixtures` (all `fixtures/`) or `zlint fixtures <paths...>`. A scenario is a folder containing `.zlint`; it is its own on-disk project root. Inputs may include nested files and project dependencies. Config files, `expected/`, `actual/` and `.zlintcache` are not lint inputs.

`expected/diagnostics` contains sorted original-input reports:

```
input.ts:1:1-1:4 rule-name#0
```

Lines/columns are 1-based, end positions exclusive, matching JSON output. Message indexes are 0-based source-order leaves: `if` branches, `??` alternatives and a pipeline's final value. Other expressions (including map results) have one leaf. Engine/evaluation failures use `#error`. Changing message wording does not change its identity.

Rules defined in the scenario's own `.zlint` also include rendered message text after the identity; backslashes, CR and newlines escape as `\\`, `\r`, `\n`. Library rules remain index-only.

`expected/fixed/<relpath>` contains complete fixpoint text only for changed inputs; absence means unchanged. Inputs are never rewritten. A `.zlintcache` marker additionally checks cold/warm diagnostics agree; the runner restores its original bytes.

Every run replaces ignored `actual/diagnostics` and `actual/fixed/`, prints scenario differences, and exits 1 on failures. `--bless` copies actual results into `expected/`, removing stale fixed files. Review changes before committing. `cargo test` runs the same scenario runner.

`actual/fixed-diagnostics` records reports from re-linting fixpoint text. Compare it by adding optional `expected/fixed-diagnostics` (an empty file asserts clean); bless updates it only when present. Lint targets follow the CLI's git/config ignores; project stubs remain available on disk.

Keep `fixtures/.ignore`: its `expected/` and `actual/` exclusions also prevent generated TypeScript declarations becoming project globals. For roots outside `fixtures/`, provide equivalent `.ignore` entries.
