# shared-frozen-file

Seal files that must not change. A comment line holding `dxlint-frozen <hash>` seals the rest of the file: the hash covers every other line, so any edit outside the seal line is reported until someone deliberately re-seals the file. Files without a seal are left alone, so the rule can be on everywhere.

| | |
| --- | --- |
| Upstream | none (dxlint only) |
| Needs types | no |
| Fix | autofix, suggestion |
| Rulesets | none |

## Enable

```dx
import .ts
rule shared-frozen-file
```

## Examples

To freeze a file, add a seal without a hash; the autofix fills it in:

```ts
// dxlint-frozen
export const answer = 42;
```

Correct (the seal matches the rest of the file):

```ts
// dxlint-frozen 467c80deaa926a10
export const answer = 42;
```

Changing any other line reports `Frozen file changed since it was sealed.` The only fix is the suggestion "Seal the file as it is now", which `--fix`, fix-all and formatting never apply: accepting it is the deliberate step, and the new hash shows up in the diff.

## The hash

- 64-bit FNV-1a (`fnv_hash`), written as 16 lowercase hex digits. It is the same on every platform and dxlint version.
- It covers the file without the seal's line, with CRLF line breaks read as LF and without a byte order mark, so line-ending conversions (git `autocrlf`, editors) keep the seal.
- A seal line is a comment line (`//`, `#`, `--`, `;`, `/*`, `<!--`) starting with `dxlint-frozen`. Only the first one counts.

## Freezing what a program produces

A seal may name a [command](../advanced.md#commands) the root config declares, plus its arguments: it then covers what that command prints, not the file's text.

```dx
command mapgen batch "node node_modules/tsx/dist/cli.mjs task/mapgen-snapshot.ts"
rule shared-frozen-file
```

A `batch` command runs once for every seal naming it and prints a section per argument list; without `batch`, each seal's arguments get a run of their own.

```ts
// dxlint-frozen mapgen beta2.15.0 8c1e…
```

The command runs again only when a file it reported reading changes (the language server runs it in the background), and the seal reports when its output no longer matches. A failing command is reported on the seal with its error.

## Limits

A seal is a speed bump, not a lock: anyone (or any agent) can accept the suggestion. It makes changes to frozen files deliberate and visible in review.
