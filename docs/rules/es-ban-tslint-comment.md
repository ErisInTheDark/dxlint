# es-ban-tslint-comment

Reports obsolete TSLint enable and disable comments. Remove these once TSLint no longer runs on the project.

| | |
| --- | --- |
| Upstream | [`@typescript-eslint/ban-tslint-comment`](https://typescript-eslint.io/rules/ban-tslint-comment) |
| Needs types | no |
| Fix | autofix |
| Rulesets | `rules:style` |
| Parity | 13 / 13 corpus cases |

## Enable

```dx
import .ts
rule es-ban-tslint-comment
```

## Examples

Incorrect:

```ts
/* tslint:disable */
// tslint:disable-next-line
someCode();
```

Correct:

```ts
// This comment mentions tslint without being a directive.
someCode();
```

## Differences from upstream

- Autofixes remove directives when safe. An inline block comment containing a line break is left for manual removal to preserve JavaScript's line-break semantics.

Adapted from the [typescript-eslint](https://typescript-eslint.io/rules/ban-tslint-comment) documentation (MIT).
